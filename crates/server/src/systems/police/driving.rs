//! Ordered per-unit driving decisions, split into focused responsibilities.
use super::*;
use crate::systems::population::Observer;
use crate::{
    ecs::state::Collision,
    ecs::{GameId, TickOutcome},
    systems::planning::Planning,
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct DriveActors<'w, 's> {
    cars: Query<'w, 's, (&'static GameId, &'static Vehicle)>,
    units: Query<'w, 's, (&'static GameId, &'static mut Unit)>,
    officers: Query<'w, 's, (&'static GameId, &'static Officer)>,
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent)>,
    wanted: Query<'w, 's, (&'static GameId, &'static Wanted)>,
    peds: Query<'w, 's, (&'static GameId, &'static PedestrianComponent)>,
}
pub(crate) fn drive_actors(
    police: &mut PoliceSystem,
    tick: u32,
    observers: &[Observer],
    sessions: &Sessions,
    collision: &mut CollisionWorld,
    actors: &mut DriveActors,
) -> Result<(), String> {
    let cars: BTreeMap<_, _> = actors.cars.iter().map(|(id, v)| (id.0.0, v)).collect();
    let mut units: BTreeMap<_, _> = actors
        .units
        .iter_mut()
        .map(|(id, v)| (id.0.0, v.into_inner()))
        .collect();
    let officers: BTreeMap<_, _> = actors.officers.iter().map(|(id, v)| (id.0.0, v)).collect();
    let players: BTreeMap<_, _> = actors.players.iter().map(|(id, v)| (id.0.0, v)).collect();
    let wanted: BTreeMap<_, _> = actors.wanted.iter().map(|(id, v)| (id.0.0, v)).collect();
    let peds: BTreeMap<_, _> = actors.peds.iter().map(|(id, v)| (id.0.0, v)).collect();

    let targets: BTreeMap<_, _> = sessions
        .peers
        .values()
        .filter_map(|s| s.player.map(|id| (id.0, (*wanted[&s.player_id()]).clone())))
        .collect();
    let mut batch = DriveBatch::default();
    let unit_cars: BTreeMap<_, _> = cars.iter().map(|(&id, v)| (id, (*v).clone())).collect();
    let unit_officers: Vec<_> = officers
        .values()
        .map(|o| (o.unit, o.mounted, o.state.clone()))
        .collect();
    let unit_players: BTreeMap<_, _> = players
        .iter()
        .map(|(&id, p)| (id, p.state.clone()))
        .collect();
    let unit_pedestrians: Vec<_> = peds.values().map(|p| p.state.clone()).collect();
    let observations = Observations {
        targets: &targets,
        cars: &unit_cars,
        officers: &unit_officers,
        players: &unit_players,
        pedestrians: &unit_pedestrians,
        observers,
    };
    let result = (|| {
        for id in units.keys().copied().collect::<Vec<_>>() {
            let unit = units.get_mut(&id).expect("police unit");
            let car = &unit_cars[&id];
            let mut frame = DriveFrame {
                id,
                tick,
                car,
                unit,
            };
            let Some(goal) = select_goal(&mut frame, police, collision, &observations, &mut batch)
            else {
                continue;
            };
            plan_route(&mut frame, police, collision, &goal, &mut batch);
            let mut motion = steer(&mut frame, police, &goal);
            avoid_obstacles(&mut frame, collision, &observations, &mut motion)?;
            recover(&mut frame, &goal, &motion);
            if motion.speed.abs() <= 0.1 {
                motion.input.buttons = Buttons(4);
            }
            frame.unit.target_speed = motion.speed;
            frame.unit.input = motion.input;
        }
        Ok(())
    })();
    collision.exclude_vehicle(None);
    result
}
pub(crate) fn drive(
    mut police: ResMut<PoliceSystem>,
    mut actors: DriveActors,
    planning: Res<Planning>,
    sessions: Res<Sessions>,
    mut collision: NonSendMut<Collision>,
    mut outcome: ResMut<TickOutcome>,
) {
    let result = drive_actors(
        &mut police,
        planning.tick,
        &planning.observers,
        &sessions,
        &mut collision.0,
        &mut actors,
    );
    collision.0.exclude_vehicle(None);
    if let Err(e) = result {
        outcome.fail(e);
    }
}

// Invocation-local coordination only: earlier cars reserve goals and consume the budget.
struct DriveBatch {
    plans_remaining: u32,
    target_counts: BTreeMap<u32, usize>,
    claimed_goals: BTreeSet<(Option<u32>, u32)>,
}
impl Default for DriveBatch {
    fn default() -> Self {
        Self {
            plans_remaining: 2,
            target_counts: BTreeMap::new(),
            claimed_goals: BTreeSet::new(),
        }
    }
}
struct DriveFrame<'a> {
    id: u32,
    tick: u32,
    car: &'a Vehicle,
    unit: &'a mut Unit,
}
struct Observations<'a> {
    targets: &'a BTreeMap<u32, Wanted>,
    cars: &'a BTreeMap<u32, Vehicle>,
    officers: &'a [(u32, bool, CharacterState)],
    players: &'a BTreeMap<u32, CharacterState>,
    pedestrians: &'a [CharacterState],
    observers: &'a [Observer],
}
type Contact = (u32, [f32; 3], f32, Option<[f32; 3]>);
struct Goal<'a> {
    known: Option<[f32; 3]>,
    wanted: Option<&'a Wanted>,
    searching: bool,
    prediction: Option<(u32, f32)>,
    holding: bool,
    direct: Option<[f32; 3]>,
    contact: Option<Contact>,
}
struct Motion {
    input: InputFrame,
    speed: f32,
    target: Option<[f32; 3]>,
    contact: Option<Contact>,
    reversing: bool,
}
// Start with a search point in the direction the car is already facing.
const LEGS: [[f32; 3]; 4] = [
    [-48.0, 0.0, -48.0],
    [48.0, 0.0, -48.0],
    [48.0, 0.0, 48.0],
    [-48.0, 0.0, 48.0],
];

fn select_goal<'a>(
    frame: &mut DriveFrame,
    police: &PoliceSystem,
    collision: &mut CollisionWorld,
    observations: &Observations<'a>,
    batch: &mut DriveBatch,
) -> Option<Goal<'a>> {
    let id = frame.id;
    let tick = frame.tick;
    let car = frame.car;
    let unit = &mut *frame.unit;
    let targets = observations.targets;
    let unit_cars = observations.cars;
    let unit_officers = observations.officers;
    let unit_players = observations.players;
    let unit_pedestrians = observations.pedestrians;
    if car.state.health.0 == 0 {
        return None;
    }
    if unit_officers
        .iter()
        .any(|officer| officer.0 == id && !officer.1 && officer.2.health.0 > 0)
    {
        unit.target_speed = 0.0;
        unit.input = sim_core::neutral_input(tick, car.state.motor.yaw);
        unit.input.buttons = Buttons(4);
        return None;
    }
    let role = unit.target.map_or(Role::Patrol, |target| {
        let count = batch.target_counts.entry(target).or_insert(0);
        let role = match *count {
            0 => Role::Lead,
            1 => Role::Intercept,
            _ => Role::Contain,
        };
        *count += 1;
        role
    });
    if unit.role != role {
        unit.role = role;
        unit.route.clear();
        unit.lane_entry = None;
        unit.search_leg = None;
        unit.intercept_retry = 0;
        unit.next_plan = tick;
    }
    let p = car.state.motor.position.0.map(|v| v.0);
    let yaw = car.state.motor.yaw.0;
    if unit
        .lane_entry
        .is_some_and(|point| distance(p, point) < 3.0)
    {
        unit.lane_entry = None;
    }
    let reached_goal =
        unit.route.len() == 1 && distance(p, police.road.points[&unit.route[0]]) < 4.0;
    while unit
        .route
        .front()
        .is_some_and(|n| distance(p, police.road.points[n]) < 4.0)
    {
        unit.route.pop_front();
    }
    let wanted = unit.target.and_then(|id| targets.get(&id));
    let known = wanted.map(|target| target.last_known);
    let deploy = unit.target.zip(wanted).is_some_and(|(target, wanted)| {
        wanted.heat >= 2
            && tick.saturating_sub(wanted.last_seen) <= 6
            && unit_players
                .get(&target)
                .is_some_and(|player| player.mode == CharacterMode::OnFoot)
            && distance(p, wanted.last_known) <= 14.0
            && sight(&collision, p, wanted.last_known, 14.0)
            && (unit_officers
                .iter()
                .filter(|officer| officer.0 == id)
                .count()
                < 2
                || unit_officers
                    .iter()
                    .any(|officer| officer.0 == id && officer.1 && officer.2.health.0 > 0))
    });
    if deploy {
        unit.target_speed = 0.0;
        unit.input = sim_core::neutral_input(tick, car.state.motor.yaw);
        unit.input.buttons = Buttons(4);
        return None;
    }
    let searching = wanted.is_some_and(|target| {
        let unseen = tick.saturating_sub(target.last_seen) >= 30;
        unseen
            && (unit.search_leg.is_some()
                || target.pursuit == Pursuit::Searching
                || distance(p, target.last_known) <= 12.0)
    });
    if searching != unit.search_leg.is_some() {
        unit.search_leg = searching.then(|| {
            (0..LEGS.len())
                .min_by(|&a, &b| {
                    let error = |i| wrap(yaw_to([0.0; 3], LEGS[i]) - yaw).abs();
                    error(a).total_cmp(&error(b))
                })
                .map_or(0, |leg| (leg + role.search_offset()) % LEGS.len())
        });
        unit.route.clear();
        unit.lane_entry = None;
        unit.next_plan = tick;
    } else if reached_goal {
        if let Some(leg) = &mut unit.search_leg {
            *leg = (*leg + 1) % LEGS.len();
        } else if known.is_none() {
            unit.patrol_leg = (unit.patrol_leg + 1) % LEGS.len();
        }
    }
    collision.exclude_vehicle(Some(id));
    // Road routing gets us into the area. Leave the lane once the
    // whole car has a clear approach to the last observed position.
    let prediction = wanted
        .and_then(|target| target.vehicle_sighting)
        .filter(|_| !searching && tick >= unit.intercept_retry)
        .and_then(|sighting| sighting.prediction(tick, role))
        .and_then(|(point, time)| {
            let node = police.road.nearest(point)?;
            let position = police.road.points[&node];
            // Reject a snapped point too far from the projected approach,
            // or one already covered by another car on this pursuit.
            (distance(position, point) <= 32.0
                && distance(p, position) <= INTERCEPT_TRAVEL_SPEED * time
                && !batch.claimed_goals.contains(&(unit.target, node)))
            .then_some((node, time))
        });
    if unit.intercepting != prediction.is_some() {
        unit.intercepting = prediction.is_some();
        unit.route.clear();
        unit.lane_entry = None;
        unit.next_plan = tick;
    }
    let holding = prediction.is_some_and(|(node, _)| distance(p, police.road.points[&node]) < 4.0);
    if let Some((node, _)) = prediction {
        batch.claimed_goals.insert((unit.target, node));
    }
    let direct = known
        .filter(|target| !searching && prediction.is_none() && distance(p, *target) <= 45.0)
        .filter(|target| {
            let d = distance(p, *target);
            let stop = car.template.dimensions[2] as f32 / 2.0 + 2.8;
            let delta = [
                (target[0] - p[0]) * (1.0 - stop / d.max(stop)),
                0.0,
                (target[2] - p[2]) * (1.0 - stop / d.max(stop)),
            ];
            collision
                .cast_vehicle(
                    p,
                    yaw_to(p, *target),
                    car.template.dimensions.map(|v| v as f32),
                    delta,
                )
                .is_ok_and(|cast| {
                    cast.contacts.is_empty()
                        && cast
                            .applied
                            .iter()
                            .zip(delta)
                            .all(|(a, b)| (a - b).abs() < 0.05)
                })
        });
    // A low-speed nudge is only available to the lead car behind a
    // confirmed occupied vehicle. Every other collider still stops it.
    let contact = wanted
        .filter(|target| target.heat >= 2 && role == Role::Lead && !searching)
        .and_then(|target| target.vehicle_sighting)
        .filter(|sighting| tick.saturating_sub(sighting.tick) <= 6)
        .and_then(|sighting| {
            let target = unit_cars.get(&sighting.id)?;
            let driver = target.state.driver_id?;
            if Some(driver.0) != unit.target || target.state.disabled {
                return None;
            }
            let motor = &target.state.motor;
            let position = motor.position.0.map(|v| v.0);
            let distance_to_target = distance(p, position);
            let safe_geometry = distance_to_target <= 11.0
                && sight(&collision, p, position, 11.0)
                && wrap(yaw_to(p, position) - yaw).abs() <= 0.15
                && wrap(motor.yaw.0 - yaw).abs() <= 0.25
                && (0.0..=5.0).contains(&motor.speed.0)
                && (0.0..=5.5).contains(&car.state.motor.speed.0)
                && car.state.motor.speed.0 - motor.speed.0 <= 3.0;
            let nearby = |point| distance(p, point) < 10.0 || distance(position, point) < 8.0;
            let bystander = unit_players.iter().any(|(&id, player)| {
                id != driver.0 && nearby(player.motor.position.0.map(|v| v.0))
            }) || unit_pedestrians
                .iter()
                .any(|pedestrian| nearby(pedestrian.motor.position.0.map(|v| v.0)));
            (safe_geometry && !bystander).then(|| {
                (
                    sighting.id,
                    position,
                    (motor.speed.0 + 2.5).min(5.5),
                    unit_players
                        .get(&driver.0)
                        .map(|player| player.motor.position.0.map(|v| v.0)),
                )
            })
        });
    Some(Goal {
        known,
        wanted,
        searching,
        prediction,
        holding,
        direct,
        contact,
    })
}
fn plan_route(
    frame: &mut DriveFrame,
    police: &mut PoliceSystem,
    collision: &mut CollisionWorld,
    goal: &Goal,
    batch: &mut DriveBatch,
) {
    let id = frame.id;
    let tick = frame.tick;
    let car = frame.car;
    let unit = &mut *frame.unit;
    let p = car.state.motor.position.0.map(|v| v.0);
    let yaw = car.state.motor.yaw.0;
    let dimensions = car.template.dimensions.map(|v| v as f32);
    let Goal {
        known,
        prediction,
        holding,
        direct,
        ..
    } = *goal;
    let reversing = tick < unit.reverse_until;
    if !reversing
        && !holding
        && direct.is_none()
        && tick >= unit.next_plan
        && batch.plans_remaining > 0
    {
        let goal = match (known, unit.search_leg) {
            (Some(last), Some(leg)) => std::array::from_fn(|i| last[i] + LEGS[leg][i]),
            (Some(last), None) => last,
            (None, _) => std::array::from_fn(|i| unit.home[i] + LEGS[unit.patrol_leg][i]),
        };
        let end = prediction
            .map(|(node, _)| node)
            .or_else(|| police.road.nearest(goal));
        if unit.route.is_empty() || end != unit.route_goal {
            police.path_jobs += 1;
            batch.plans_remaining -= 1;
            // Attach to a reachable forward point on a directed lane.
            let mut candidates: Vec<_> = police
                .road
                .candidates
                .iter()
                .filter_map(|&(from, to, _, heading)| {
                    let a = police.road.points[&from];
                    let b = police.road.points[&to];
                    let length = distance(a, b);
                    let along =
                        ((p[0] - a[0]) * (b[0] - a[0]) + (p[2] - a[2]) * (b[2] - a[2])) / length;
                    let t = ((along + 6.0) / length).clamp(0.0, 1.0);
                    let point = std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
                    let gap = distance(p, point);
                    (gap < 35.0).then_some((gap + 6.0 * wrap(heading - yaw).abs(), from, to, point))
                })
                .collect();
            candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
            candidates.dedup_by_key(|c| (c.1, c.2));
            let entry = candidates.into_iter().take(8).find(|&(_, _, _, point)| {
                let delta = [point[0] - p[0], 0.0, point[2] - p[2]];
                collision
                    .cast_vehicle(p, yaw_to(p, point), dimensions, delta)
                    .is_ok_and(|cast| cast.contacts.is_empty())
            });
            if let Some((_, _, start, point)) = entry
                && let Some(end) = end
            {
                let avoided = unit.avoided_node.filter(|_| tick < unit.avoid_until);
                let route = police.road.path_avoiding(start, end, avoided);
                let route_distance = route
                    .iter()
                    .fold((p, 0.0), |(previous, total), node| {
                        let point = police.road.points[node];
                        (point, total + distance(previous, point))
                    })
                    .1;
                if prediction.is_some_and(|(_, time)| {
                    route.is_empty() || route_distance > time * INTERCEPT_TRAVEL_SPEED
                }) {
                    unit.intercept_retry = tick + 30;
                    unit.route.clear();
                    unit.lane_entry = None;
                } else if !route.is_empty() {
                    unit.route_goal = Some(end);
                    unit.route = route;
                    unit.lane_entry = (distance(p, point) >= 3.0).then_some(point);
                }
            } else if prediction.is_some() {
                unit.intercept_retry = tick + 30;
                unit.route.clear();
                unit.lane_entry = None;
            }
        }
        unit.next_plan = tick.saturating_add(15 + id % 5);
    }
}
fn steer(frame: &mut DriveFrame, police: &PoliceSystem, goal: &Goal) -> Motion {
    let tick = frame.tick;
    let car = frame.car;
    let unit = &mut *frame.unit;
    let p = car.state.motor.position.0.map(|v| v.0);
    let yaw = car.state.motor.yaw.0;
    let dimensions = car.template.dimensions.map(|v| v as f32);
    let Goal {
        known,
        wanted,
        searching,
        holding,
        direct,
        contact,
        ..
    } = *goal;
    let reversing = tick < unit.reverse_until;
    let target = contact
        .map(|(_, position, _, _)| position)
        .or(direct)
        .or(unit.lane_entry)
        .or_else(|| unit.route.front().map(|n| police.road.points[n]));
    let mut input = sim_core::neutral_input(tick, car.state.motor.yaw);
    let mut speed = 0.0;
    unit.blocked = false;
    if holding {
        // Cover the predicted exit until the sighting changes.
    } else if reversing {
        speed = -2.0;
    } else if let Some(target) = target {
        let approach = yaw_to(p, target);
        let next = (contact.is_none() && direct.is_none() && unit.lane_entry.is_none())
            .then(|| unit.route.get(1).map(|id| police.road.points[id]))
            .flatten();
        let corner = next.map_or(0.0, |point| wrap(yaw_to(target, point) - approach).abs());
        let lookahead = (car.state.motor.speed.0.abs() * 0.6).clamp(4.0, 12.0);
        let aim = next.filter(|_| corner < 0.35).map_or(target, |point| {
            let t = ((lookahead - distance(p, target)) / distance(target, point).max(0.1))
                .clamp(0.0, 1.0);
            std::array::from_fn(|i| target[i] + (point[i] - target[i]) * t)
        });
        let error = wrap(yaw_to(p, aim) - yaw);
        input.move_x = InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
        let pursuing = known.is_some() && !searching;
        let cruise = if pursuing { PURSUIT_SPEED } else { 12.0 };
        let turn_penalty = if pursuing { 3.0 } else { 5.0 };
        let turning = (cruise / (1.0 + turn_penalty * error.abs())).max(2.0);
        let exit_speed = if next.is_some() && corner < 0.35 {
            cruise
        } else {
            4.0
        };
        speed = turning.min(
            (exit_speed.powi(2)
                + 2.0 * car.template.brake as f32 * (distance(p, target) - 4.0).max(0.0))
            .sqrt(),
        );
        if direct.is_some() {
            let gap = (distance(p, target) - dimensions[2] / 2.0 - 2.8).max(0.0);
            // Follow a moving vehicle using its recent observed motion.
            // Stationary and on-foot targets still use a stopping approach.
            let lead_speed = wanted
                .and_then(|target| target.vehicle_sighting)
                .filter(|sighting| {
                    tick.saturating_sub(sighting.tick) <= 6
                        && wrap(sighting.yaw - approach).abs() < 0.35
                })
                .map_or(0.0, |sighting| sighting.speed.max(0.0));
            speed = turning.min(approach_speed(gap, car.template.brake as f32, lead_speed));
        }
    }
    let contact = contact.filter(|_| !reversing && !holding);
    if let Some((_, _, approach_speed, _)) = contact {
        speed = approach_speed;
    }
    Motion {
        input,
        speed,
        target,
        contact,
        reversing,
    }
}
fn avoid_obstacles(
    frame: &mut DriveFrame,
    collision: &mut CollisionWorld,
    observations: &Observations,
    motion: &mut Motion,
) -> Result<(), String> {
    let car = frame.car;
    let unit = &mut *frame.unit;
    let p = car.state.motor.position.0.map(|v| v.0);
    let yaw = car.state.motor.yaw.0;
    let observers = observations.observers;
    let unit_pedestrians = observations.pedestrians;
    let mut speed = motion.speed;
    let contact = motion.contact;
    let reversing = motion.reversing;
    if speed.abs() > 0.1 || car.state.motor.speed.0.abs() > 0.1 {
        let current_speed = car.state.motor.speed.0;
        let direction = if current_speed.abs() >= 0.25 {
            current_speed.signum()
        } else if reversing {
            -1.0
        } else {
            1.0
        };
        let forward = [-yaw.sin() * direction, 0.0, -yaw.cos() * direction];
        let sensor = (car.state.motor.speed.0.powi(2) / (2.0 * car.template.brake as f32) + 0.1)
            .max(if reversing { 0.8 } else { 0.2 });
        let cast = collision.cast_vehicle(
            p,
            car.state.motor.yaw.0,
            car.template.dimensions.map(|v| v as f32),
            forward.map(|v| v * sensor),
        )?;
        if cast.contacts.iter().any(|hit| {
            !hit.vehicle || contact.is_none_or(|(target, _, _, _)| target != hit.collider_id)
        }) {
            unit.blocked = true;
            speed = 0.0;
        }
        for actor in observers.iter().map(|o| o.position).chain(
            unit_pedestrians
                .iter()
                .map(|p| p.motor.position.0.map(|v| v.0)),
        ) {
            if contact.is_some_and(|(_, _, _, driver)| driver == Some(actor)) {
                continue;
            }
            let delta = [actor[0] - p[0], actor[2] - p[2]];
            let along = delta[0] * forward[0] + delta[1] * forward[2];
            let across = (delta[0] * forward[2] - delta[1] * forward[0]).abs();
            if along > 0.0
                && along < car.template.dimensions[2] as f32 / 2.0 + sensor
                && across < car.template.dimensions[0] as f32 / 2.0 + 0.6
            {
                unit.blocked = true;
                speed = 0.0;
            }
        }
    }
    motion.speed = speed;
    Ok(())
}
fn recover(frame: &mut DriveFrame, goal: &Goal, motion: &Motion) {
    let tick = frame.tick;
    let car = frame.car;
    let unit = &mut *frame.unit;
    let p = car.state.motor.position.0.map(|v| v.0);
    let yaw = car.state.motor.yaw.0;
    let holding = goal.holding;
    let direct = goal.direct;
    let reversing = motion.reversing;
    let target = motion.target;
    if reversing && unit.blocked && car.state.motor.speed.0 < 0.25 {
        unit.reverse_until = tick;
    }
    let struggling = !holding
        && direct.is_none()
        && (unit.blocked
            || target.is_none()
            || target.is_some_and(|target| wrap(yaw_to(p, target) - yaw).abs() > 1.7));
    if !reversing && struggling {
        let since = *unit.blocked_since.get_or_insert(tick);
        if tick.saturating_sub(since) >= 45 && tick >= unit.retry_after {
            unit.avoided_node = unit.route.front().copied();
            unit.avoid_until = tick + 300;
            unit.route.clear();
            unit.lane_entry = None;
            unit.next_plan = tick;
            unit.reverse_until = tick + 45;
            unit.retry_after = tick + 150;
            unit.blocked_since = None;
        }
    } else {
        unit.blocked_since = None;
    }
}
