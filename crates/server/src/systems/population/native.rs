//! Native ambient population queries and bounded decision algorithms.
use super::*;
use crate::systems::population::Observer;
use crate::{
    ecs::state::{Collision, Content},
    ecs::{GameId, TickOutcome},
    systems::planning::Planning,
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DirectorActors<'w, 's> {
    cars: Query<'w, 's, (Entity, &'static GameId, &'static Vehicle)>,
    brains: Query<'w, 's, (Entity, &'static GameId, &'static mut Traffic)>,
    peds: Query<'w, 's, (Entity, &'static GameId, &'static mut Pedestrian)>,
    commands: Commands<'w, 's>,
}
pub(crate) fn direct_actors(
    population: &mut PopulationSystem,
    tick: u32,
    observers: &[Observer],
    manifest: &WorldManifest,
    collision: &mut CollisionWorld,
    next_id: &mut u32,
    actors: &mut DirectorActors,
) -> Result<(), String> {
    if !tick.is_multiple_of(30) {
        return Ok(());
    }
    // Per-invocation geometry includes queued spawns, so later candidates cannot overlap.
    let mut cars: BTreeMap<_, _> = actors
        .cars
        .iter()
        .map(|(e, id, c)| (id.0.0, (e, c.clone())))
        .collect();
    let mut count = actors.brains.iter().count();
    let mut brains: Vec<_> = actors.brains.iter_mut().collect();
    brains.sort_by_key(|(_, id, _)| id.0.0);
    for (entity, id, mut brain) in brains {
        let Some((_, car)) = cars.get(&id.0.0) else {
            continue;
        };
        let p = car.state.motor.position.0.map(|v| v.0);
        if car.state.driver_id.is_some() || car.state.disabled || car.state.health.0 == 0 {
            brain.far_since = None;
            continue;
        }
        let far = aged_far(tick, p, observers, &mut brain.far_since);
        let stuck =
            tick.saturating_sub(brain.progress_at) >= 450 && brain.phase != TrafficPhase::Available;
        if (far || stuck) && hidden(p, observers, collision) {
            crate::systems::planning::despawn(&mut actors.commands, id.0.0, entity);
            cars.remove(&id.0.0);
            count -= 1;
            population.intersections.release(id.0.0);
            population.metrics.despawned += 1;
        }
    }
    let mut positions = BTreeMap::new();
    let mut peds: Vec<_> = actors.peds.iter_mut().collect();
    peds.sort_by_key(|(_, id, _)| id.0.0);
    for (entity, id, mut ped) in peds {
        let p = ped.state.motor.position.0.map(|v| v.0);
        let remove = ped.state.health.0 > 0 && {
            let far = aged_far(tick, p, observers, &mut ped.far_since);
            (far || tick.saturating_sub(ped.progress_at) >= 450) && hidden(p, observers, collision)
        };
        if remove {
            crate::systems::planning::despawn(&mut actors.commands, id.0.0, entity);
            population.metrics.despawned += 1;
        } else {
            positions.insert(id.0.0, p);
        }
    }
    collision.set_vehicle_proxies(
        &cars
            .values()
            .map(|(_, c)| crate::systems::vehicle::proxy(c))
            .collect::<Vec<_>>(),
    )?;
    if observers.is_empty() {
        return Ok(());
    }
    for _ in 0..population.road.candidates.len().min(128) {
        if count >= TRAFFIC_CAP || population.road.candidates.is_empty() {
            break;
        }
        let (_, to, p, yaw) =
            population.road.candidates[population.car_cursor % population.road.candidates.len()];
        population.car_cursor += 1;
        if !spawn_region(p, observers)
            || !hidden(p, observers, collision)
            || cars
                .values()
                .any(|(_, c)| distance(p, c.state.motor.position.0.map(|v| v.0)) < 12.0)
        {
            continue;
        }
        let template = manifest.vehicle_templates[count % manifest.vehicle_templates.len()].clone();
        if collision.overlaps_vehicle(p, yaw, template.dimensions.map(|v| v as f32))? {
            population.metrics.blocked_spawns += 1;
            continue;
        }
        let id = allocate(next_id)?;
        let car = Vehicle {
            id: EntityId(id),
            state: VehicleState {
                motor: sim_core::spawn_vehicle(p, yaw)?,
                health: VehicleHealth(template.health),
                driver_id: None,
                disabled: false,
            },
            template: template.into(),
            idle_ticks: 0,
        };
        let brain = Traffic {
            phase: TrafficPhase::Driving,
            route: VecDeque::from([to]),
            input: sim_core::neutral_input(tick, Yaw(yaw)),
            target_speed: 0.0,
            next_plan: tick,
            progress_at: tick,
            progress_position: p,
            far_since: None,
            parking: None,
            born: tick,
        };
        crate::systems::planning::spawn(
            &mut actors.commands,
            id,
            (
                car.clone(),
                brain,
                crate::components::PoolMembership(VehiclePool::Traffic),
            ),
        );
        cars.insert(id, (Entity::PLACEHOLDER, car));
        count += 1;
        population.metrics.spawned += 1;
        collision.set_vehicle_proxies(
            &cars
                .values()
                .map(|(_, c)| crate::systems::vehicle::proxy(c))
                .collect::<Vec<_>>(),
        )?;
    }
    for _ in 0..population.sidewalk.candidates.len().min(256) {
        if positions.len() >= PEDESTRIAN_CAP || population.sidewalk.candidates.is_empty() {
            break;
        }
        let (from, to, mut p, yaw) = population.sidewalk.candidates
            [population.pedestrian_cursor % population.sidewalk.candidates.len()];
        population.pedestrian_cursor += 1;
        p[1] = 0.02;
        if !spawn_region(p, observers)
            || !hidden(p, observers, collision)
            || positions.values().any(|q| distance(p, *q) < 3.0)
        {
            continue;
        }
        if collision.overlaps_character(p, QueryFilter::MOVEMENT)? {
            population.metrics.blocked_spawns += 1;
            continue;
        }
        let id = allocate(next_id)?;
        let ped = Pedestrian {
            id: EntityId(id),
            state: CharacterState {
                equipment: None,
                motor: sim_core::spawn_character(p, yaw)?,
                health: CharacterHealth(100),
                mode: CharacterMode::OnFoot,
                vehicle_id: None,
            },
            phase: PedestrianPhase::Walking,
            from,
            to,
            input: sim_core::neutral_input(tick, Yaw(yaw)),
            wait_until: 0,
            far_since: None,
            progress_at: tick,
            progress_position: p,
            flee_until: 0,
            threat: p,
        };
        crate::systems::planning::spawn(&mut actors.commands, id, ped);
        positions.insert(id, p);
        population.metrics.spawned += 1;
    }
    Ok(())
}
pub(crate) fn direct(
    mut population: ResMut<PopulationSystem>,
    mut actors: DirectorActors,
    planning: Res<Planning>,
    content: Res<Content>,
    mut collision: NonSendMut<Collision>,
    mut next: ResMut<crate::ecs::state::EntityAllocator>,
    mut outcome: ResMut<TickOutcome>,
) {
    if let Err(e) = direct_actors(
        &mut population,
        planning.tick,
        &planning.observers,
        &content.0.manifest,
        &mut collision.0,
        &mut next.0,
        &mut actors,
    ) {
        outcome.fail(e);
    }
}
