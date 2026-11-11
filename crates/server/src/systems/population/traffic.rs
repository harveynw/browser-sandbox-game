//! Traffic route planning and driving decisions have separate query access.
use super::Observer;
use super::*;
use crate::{
    ecs::state::{Collision, Content},
    ecs::{GameId, TickOutcome},
    systems::planning::Planning,
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RouteActors<'w, 's> {
    cars: Query<'w, 's, (&'static GameId, &'static Vehicle)>,
    brains: Query<'w, 's, (&'static GameId, &'static mut Traffic)>,
}
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DrivingActors<'w, 's> {
    cars: Query<'w, 's, (&'static GameId, &'static Vehicle)>,
    brains: Query<'w, 's, (&'static GameId, &'static mut Traffic)>,
    peds: Query<'w, 's, &'static Pedestrian>,
}
pub(crate) fn plan_actors(population: &mut PopulationSystem, tick: u32, actors: &mut RouteActors) {
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let mut brains: BTreeMap<_, _> = actors
        .brains
        .iter_mut()
        .map(|(id, b)| (id.0.0, b.into_inner()))
        .collect();
    let ids: Vec<_> = brains.keys().copied().collect();
    for id in &ids {
        let car = &cars[id];
        let b = brains.get_mut(id).expect("traffic brain");
        if car.state.driver_id.is_some()
            || car.state.disabled
            || car.state.health.0 == 0
            || b.phase == TrafficPhase::Available
        {
            continue;
        }
        let p = car.state.motor.position.0.map(|v| v.0);
        if distance(p, b.progress_position) >= 2.0 {
            b.progress_position = p;
            b.progress_at = tick;
        }
        if tick >= b.next_plan
            && (b.route.len() <= 1 || tick.saturating_sub(b.progress_at) >= 150)
            && !population.queue.contains(id)
        {
            population.queue.push_back(*id);
        }
    }
    let mut jobs = 0;
    while jobs < PATHS_PER_TICK {
        let Some(id) = population.queue.pop_front() else {
            break;
        };
        let Some(brain) = brains.get(&id) else {
            continue;
        };
        if tick < brain.next_plan
            || brain.phase == TrafficPhase::Available
            || cars[&id].state.driver_id.is_some()
        {
            continue;
        }
        let p = cars[&id].state.motor.position.0.map(|v| v.0);
        let start = brain
            .route
            .front()
            .copied()
            .or_else(|| population.road.nearest(p));
        let nodes: Vec<_> = population
            .road
            .edges
            .keys()
            .copied()
            .filter(|n| distance(p, population.road.points[n]) > 60.0)
            .collect();
        let random = population.random();
        let goal = nodes.get(random % nodes.len().max(1)).copied();
        let route = start
            .zip(goal)
            .map_or_else(VecDeque::new, |(a, b)| population.road.path(a, b));
        let brain = brains.get_mut(&id).expect("traffic brain");
        brain.route = route;
        brain.next_plan = tick.saturating_add(15);
        if tick.saturating_sub(brain.progress_at) >= 150 {
            population.metrics.replans += 1;
        }
        jobs += 1;
        population.metrics.path_jobs += 1;
    }
    population.metrics.max_path_jobs_per_tick = population.metrics.max_path_jobs_per_tick.max(jobs);
}
pub(crate) fn drive_actors(
    population: &mut PopulationSystem,
    tick: u32,
    observers: &[Observer],
    manifest: &WorldManifest,
    collision: &mut CollisionWorld,
    actors: &mut DrivingActors,
) -> Result<(), String> {
    if !tick.is_multiple_of(6) {
        return Ok(());
    }
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, c)| (id.0.0, c)).collect();
    let mut brains: BTreeMap<_, _> = actors
        .brains
        .iter_mut()
        .map(|(id, b)| (id.0.0, b.into_inner()))
        .collect();
    let ped_observations: Vec<_> = actors
        .peds
        .iter()
        .map(|p| p.state.motor.position.0.map(|v| v.0))
        .collect();
    let ids: Vec<_> = brains.keys().copied().collect();
    let result = (|| -> Result<(), String> {
        // IDs decide simultaneous reservation ties, independent of network arrival.
        let mut parking_claims: BTreeSet<_> = brains
            .values()
            .filter(|b| b.phase == TrafficPhase::Parking)
            .filter_map(|b| b.parking)
            .collect();
        for id in ids {
            let car = &cars[&id];
            let b = brains.get_mut(&id).expect("traffic brain");
            if car.state.driver_id.is_some()
                || car.state.disabled
                || car.state.health.0 == 0
                || b.phase == TrafficPhase::Available
            {
                population.intersections.release(id);
                continue;
            }
            let p = car.state.motor.position.0.map(|v| v.0);
            while b
                .route
                .front()
                .is_some_and(|n| distance(p, population.road.points[n]) < 5.0)
            {
                b.route.pop_front();
            }
            let mut target = b.route.front().map(|n| population.road.points[n]);
            // Vacated authored public spots are the only parking destinations.
            if b.parking.is_none() && tick.saturating_sub(b.born) > 900 {
                b.parking = manifest
                    .parking
                    .iter()
                    .enumerate()
                    .find(|(index, slot)| {
                        !slot.mission_reserved
                            && distance(p, slot.position.map(|v| v as f32)) < 18.0
                            && cars.values().all(|v| {
                                v.id.0 == id
                                    || distance(
                                        slot.position.map(|v| v as f32),
                                        v.state.motor.position.0.map(|v| v.0),
                                    ) > 8.0
                            })
                            && !parking_claims.contains(index)
                    })
                    .map(|(index, _)| index);
            }
            if let Some(index) = b.parking {
                parking_claims.insert(index);
                target = Some(manifest.parking[index].position.map(|v| v as f32));
                if distance(p, target.expect("parking target")) < 2.0
                    && car.state.motor.speed.0.abs() < 0.3
                {
                    b.phase = TrafficPhase::Available;
                    population.intersections.release(id);
                    continue;
                }
            }
            let mut input = sim_core::neutral_input(tick, car.state.motor.yaw);
            let Some(target) = target else {
                input.buttons = Buttons(4);
                b.phase = TrafficPhase::Stopped;
                b.input = input;
                continue;
            };
            let error = wrap(yaw_to(p, target) - car.state.motor.yaw.0);
            input.move_x = InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
            let mut target_speed = if error.abs() > 0.2 || distance(p, target) < 25.0 {
                4.0
            } else {
                12.0
            };
            if b.parking.is_some() {
                target_speed = (distance(p, target) - 1.8).clamp(0.0, 3.0);
            }
            let mut yield_now = false;
            // All four lane corners of an intersection share a reservation key.
            if b.parking.is_none() {
                let key = (
                    (target[0] / 16.0).round() as i32,
                    (target[2] / 16.0).round() as i32,
                );
                let center = [key.0 as f32 * 16.0, 0.0, key.1 as f32 * 16.0];
                let d = distance(p, center);
                if d < car.state.motor.speed.0.abs() * 1.5 + 16.0 {
                    yield_now = !population.intersections.request(key, id, tick);
                }
                // Release past intersections, retaining only the current approach.
                population
                    .intersections
                    .reservations
                    .retain(|k, (owner, _)| {
                        *owner != id
                            || *k == key
                            || distance(p, [k.0 as f32 * 16.0, 0.0, k.1 as f32 * 16.0]) < 11.0
                    });
            }
            collision.exclude_vehicle(Some(id));
            let sensor = car.state.motor.speed.0.abs().mul_add(1.5, 0.0).max(4.0);
            let forward = [
                -car.state.motor.yaw.0.sin(),
                0.0,
                -car.state.motor.yaw.0.cos(),
            ];
            for lateral in [-0.45, 0.0, 0.45] {
                let origin = sim_core::anchor(
                    &car.state.motor,
                    [
                        car.template.dimensions[0] * lateral,
                        0.75,
                        -car.template.dimensions[2] / 2.0 - 0.05,
                    ],
                );
                if collision
                    .raycast(origin, forward, sensor, QueryFilter::MOVEMENT)?
                    .is_some()
                {
                    yield_now = true;
                }
            }
            // Players and pedestrians are not car collision proxies; sense them explicitly.
            for actor in observers
                .iter()
                .map(|o| o.position)
                .chain(ped_observations.iter().copied())
            {
                let delta = [actor[0] - p[0], actor[2] - p[2]];
                let along = delta[0] * forward[0] + delta[1] * forward[2];
                let across = (delta[0] * forward[2] - delta[1] * forward[0]).abs();
                if along > 0.0
                    && along < sensor + car.template.dimensions[2] as f32 / 2.0
                    && across < car.template.dimensions[0] as f32 / 2.0 + 0.6
                {
                    yield_now = true;
                }
            }
            if b.parking.is_some() && distance(p, target) < 2.0 {
                input.buttons = Buttons(4);
            }
            b.target_speed = target_speed;
            if yield_now {
                input.buttons = Buttons(4);
                b.phase = TrafficPhase::Yielding;
            } else {
                input.move_z = InputAxis(if car.state.motor.speed.0 > target_speed + 0.2 {
                    -32767
                } else if car.state.motor.speed.0 < target_speed {
                    32767
                } else {
                    0
                });
                b.phase = if b.parking.is_some() {
                    TrafficPhase::Parking
                } else {
                    TrafficPhase::Driving
                };
            }
            b.input = input;
        }
        Ok(())
    })();
    // Always release the temporary self-exclusion, including collision-query errors.
    collision.exclude_vehicle(None);
    result
}
pub(crate) fn plan_routes(
    mut population: ResMut<PopulationSystem>,
    planning: Res<Planning>,
    mut actors: RouteActors,
) {
    plan_actors(&mut population, planning.tick, &mut actors);
}
pub(crate) fn drive(
    mut population: ResMut<PopulationSystem>,
    planning: Res<Planning>,
    content: Res<Content>,
    mut collision: NonSendMut<Collision>,
    mut actors: DrivingActors,
    mut outcome: ResMut<TickOutcome>,
) {
    if let Err(error) = drive_actors(
        &mut population,
        planning.tick,
        &planning.observers,
        &content.0.manifest,
        &mut collision.0,
        &mut actors,
    ) {
        outcome.fail(error);
    }
}
