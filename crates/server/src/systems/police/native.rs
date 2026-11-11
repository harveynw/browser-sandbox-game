//! Native police assignment, routing, perception and arrest systems.
use super::*;
use crate::systems::planning::Pending;
use crate::systems::population::Observer;
use crate::{
    ecs::state::{Clock, Collision, Content},
    ecs::{GameId, TickOutcome},
    systems::planning::Planning,
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RosterActors<'w, 's> {
    cars: Query<'w, 's, (Entity, &'static GameId, &'static Vehicle)>,
    units: Query<'w, 's, (&'static GameId, &'static mut Unit)>,
    officers: Query<'w, 's, (&'static GameId, &'static Officer)>,
    players: Query<'w, 's, (&'static GameId, &'static PlayerComponent, &'static Wanted)>,
    commands: Commands<'w, 's>,
}
pub(crate) fn roster_actors(
    police: &mut PoliceSystem,
    tick: u32,
    observers: &[Observer],
    sessions: &Sessions,
    manifest: &content::WorldManifest,
    collision: &CollisionWorld,
    next_id: &mut u32,
    actors: &mut RosterActors,
) -> Result<(), String> {
    let identities: BTreeMap<_, _> = actors.cars.iter().map(|(e, id, _)| (id.0.0, e)).collect();
    let mut cars: BTreeMap<_, _> = actors
        .cars
        .iter()
        .map(|(_, id, c)| (id.0.0, c.clone()))
        .collect();
    let mut units: BTreeMap<_, _> = actors
        .units
        .iter_mut()
        .map(|(id, u)| (id.0.0, Pending::Live(u.into_inner())))
        .collect();
    let officers: BTreeMap<_, _> = actors.officers.iter().map(|(id, o)| (id.0.0, o)).collect();
    let players: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, p, _)| (id.0.0, p))
        .collect();
    let wanted_view: BTreeMap<_, _> = actors
        .players
        .iter()
        .map(|(id, _, w)| (id.0.0, w))
        .collect();
    let mut removed = Vec::new();

    if !police.enabled {
        return Ok(());
    }
    let initializing = !police.initialized;
    police.initialized = true;
    let wanted: Vec<_> = sessions
        .peers
        .values()
        .filter(|s| s.phase == SessionPhase::Playing)
        .filter_map(|s| {
            s.player.map(|id| {
                (
                    id.0,
                    wanted_view
                        .get(&(s.player_id()))
                        .expect("admitted wanted state")
                        .heat,
                )
            })
        })
        .filter(|(id, heat)| {
            *heat > 0
                && matches!(
                    players[id].state.mode,
                    CharacterMode::OnFoot | CharacterMode::Driving
                )
        })
        .map(|(id, heat)| (id, RESPONSE[usize::from(heat - 1)].units))
        .collect();
    // Rotate the first recipient every five seconds only when demand exceeds
    // supply. Allocate one unit per player before assigning second/third units.
    if tick.is_multiple_of(150)
        && wanted
            .iter()
            .map(|(_, heat)| usize::from(*heat))
            .sum::<usize>()
            > POLICE_CAP
    {
        police.cursor = (police.cursor + 1) % wanted.len();
    }
    let mut assignments = Vec::new();
    for level in 0..3 {
        for offset in 0..wanted.len() {
            let (id, heat) = wanted[(offset + police.cursor) % wanted.len()];
            if heat > level && assignments.len() < POLICE_CAP {
                assignments.push(id);
            }
        }
    }
    let desired = assignments.len().max(PATROL_HOMES.len());
    let mut remove = Vec::new();
    let unit_cars: BTreeMap<_, _> = cars.iter().map(|(&id, v)| (id, v.clone())).collect();
    let unit_officers: Vec<_> = officers
        .values()
        .map(|o| (o.unit, o.mounted, o.state.clone()))
        .collect();
    for id in units.keys().copied().collect::<Vec<_>>() {
        let u = units.get_mut(&id).expect("police unit");
        let car = &unit_cars[&id];
        let p = car.state.motor.position.0.map(|v| v.0);
        if distance(p, u.progress_position) >= 2.0 {
            u.progress_position = p;
            u.progress_at = tick;
        }
        if car.state.health.0 == 0 {
            let until = *u.wreck_until.get_or_insert(tick.saturating_add(90));
            if tick >= until
                && hidden(p, observers, &collision)
                && unit_officers
                    .iter()
                    .filter(|officer| officer.0 == id && !officer.1)
                    .all(|officer| {
                        hidden(
                            officer.2.motor.position.0.map(|v| v.0),
                            observers,
                            &collision,
                        )
                    })
            {
                remove.push(id);
            }
        }
    }
    for id in remove {
        units.remove(&id);
        removed.push(id);
        cars.remove(&id);
    }
    if initializing || tick.is_multiple_of(30) {
        let mut template = manifest
            .vehicle_templates
            .iter()
            .find(|t| t.id == "compact")
            .ok_or("police compact template missing")?
            .clone();
        template.top_speed = f64::from(PURSUIT_SPEED);
        template.acceleration = 10.0;
        template.brake = 16.0;
        while units.len() < desired {
            let index = units.len();
            // Fill uncovered districts before assigning another car to a home.
            let home = *PATROL_HOMES
                .iter()
                .min_by_key(|home| units.values().filter(|unit| unit.home == **home).count())
                .expect("patrol homes");
            let near = assignments
                .get(index)
                .and_then(|id| {
                    sessions
                        .peers
                        .values()
                        .find(|s| s.player == Some(EntityId(*id)))
                })
                .map_or(home, |s| {
                    wanted_view
                        .get(&(s.player_id()))
                        .expect("admitted wanted state")
                        .last_known
                });
            let mut candidates = police.road.candidates.clone();
            candidates.sort_by(|a, b| distance(a.2, near).total_cmp(&distance(b.2, near)));
            let spot = candidates.into_iter().find(|(_, _, p, yaw)| {
                (initializing
                    || (observers.iter().all(|o| distance(*p, o.position) >= 40.0)
                        && hidden(*p, observers, &collision)))
                    && distance(*p, near) < 180.0
                    && cars
                        .values()
                        .all(|v| distance(*p, v.state.motor.position.0.map(|v| v.0)) > 12.0)
                    && !collision
                        .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                        .unwrap_or(true)
            });
            let Some((_, to, p, yaw)) = spot else {
                break;
            };
            let id = *next_id;
            *next_id = id.checked_add(1).ok_or("police entity IDs exhausted")?;
            cars.insert(
                id,
                Vehicle {
                    id: EntityId(id),
                    template: template.clone().into(),
                    idle_ticks: 0,
                    state: VehicleState {
                        motor: sim_core::spawn_vehicle(p, yaw)?,
                        health: VehicleHealth(template.health),
                        driver_id: None,
                        disabled: false,
                    },
                },
            );
            units.insert(
                id,
                Pending::Spawn(Unit {
                    target: None,
                    role: Role::Patrol,
                    intercept_retry: 0,
                    intercepting: false,
                    home,
                    route: VecDeque::from([to]),
                    input: sim_core::neutral_input(tick, Yaw(yaw)),
                    target_speed: 0.0,
                    blocked: false,
                    blocked_since: None,
                    reverse_until: 0,
                    retry_after: 0,
                    avoided_node: None,
                    avoid_until: 0,
                    route_goal: None,
                    lane_entry: None,
                    next_plan: tick,
                    progress_at: tick,
                    progress_position: p,
                    wreck_until: None,
                    patrol_leg: 0,
                    search_leg: None,
                }),
            );
        }
    }
    // Keep existing assignments whenever the new quota can accommodate them.
    let mut free = Vec::new();
    for (&id, unit) in &units {
        if cars[&id].state.health.0 > 0
            && let Some(index) = assignments
                .iter()
                .position(|target| Some(*target) == unit.target)
        {
            assignments.remove(index);
        } else {
            free.push(id);
        }
    }
    let mut pending = assignments.into_iter();
    for id in free {
        let target = if cars[&id].state.health.0 > 0 {
            pending.next()
        } else {
            None
        };
        let unit = units.get_mut(&id).expect("listed police unit");
        if target != unit.target {
            police.assignment_changes += 1;
            unit.target = target;
            unit.route.clear();
            unit.lane_entry = None;
            unit.blocked_since = None;
            unit.reverse_until = 0;
            unit.avoided_node = None;
            unit.search_leg = None;
            unit.intercept_retry = 0;
            unit.next_plan = tick;
            unit.progress_at = tick;
        }
    }
    // Retire surplus units only outside every player's static clear view.
    let surplus: Vec<_> = units
        .keys()
        .copied()
        .filter(|id| units[id].target.is_none())
        .filter(|id| {
            officers
                .values()
                .filter(|officer| officer.unit == *id && !officer.mounted)
                .all(|officer| {
                    hidden(
                        officer.state.motor.position.0.map(|v| v.0),
                        observers,
                        &collision,
                    )
                })
        })
        .filter(|id| {
            hidden(
                cars[id].state.motor.position.0.map(|v| v.0),
                observers,
                &collision,
            )
        })
        .take(units.len().saturating_sub(desired))
        .collect();
    for id in surplus {
        units.remove(&id);
        removed.push(id);
        cars.remove(&id);
    }
    for id in removed {
        if let Some(&entity) = identities.get(&id) {
            crate::systems::planning::despawn(&mut actors.commands, id, entity);
        }
    }
    for (id, unit) in units {
        if let Pending::Spawn(unit) = unit {
            crate::systems::planning::spawn(
                &mut actors.commands,
                id,
                (cars.remove(&id).expect("spawn car"), unit),
            );
        }
    }
    Ok(())
}
pub(crate) fn roster(
    mut police: ResMut<PoliceSystem>,
    mut actors: RosterActors,
    planning: Res<Planning>,
    sessions: Res<Sessions>,
    content: Res<Content>,
    collision: NonSend<Collision>,
    mut next: ResMut<crate::ecs::state::EntityAllocator>,
    mut outcome: ResMut<TickOutcome>,
) {
    if let Err(e) = roster_actors(
        &mut police,
        planning.tick,
        &planning.observers,
        &sessions,
        &content.0.manifest,
        &collision.0,
        &mut next.0,
        &mut actors,
    ) {
        outcome.fail(e);
    }
}

#[derive(Message, Clone, Copy)]
pub(crate) struct ArrestRequested {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub entity: Entity,
    pub tick: u32,
}
#[derive(Message, Clone, Copy)]
pub(crate) struct ArrestStarted {
    pub player: EntityId,
    pub entity: Entity,
    pub tick: u32,
}
pub(crate) fn arrest(
    mut requests: MessageReader<ArrestRequested>,
    mut actors: crate::systems::impact_consequences::RecoveryActors,
    mut wanted: Query<&mut Wanted>,
    mut units: Query<&mut Unit>,
    index: Res<crate::ecs::EntityIndex>,
    mut police: ResMut<PoliceSystem>,
    mut started: MessageWriter<ArrestStarted>,
    mut controls: MessageWriter<crate::gameplay_messages::ControlTransitionRequested>,
    mut outcome: ResMut<TickOutcome>,
) {
    for message in requests.read() {
        if message.tick != actors.clock.0
            || index.0.get(&message.player.0) != Some(&message.entity)
            || !actors
                .sessions
                .peers
                .get(&message.peer)
                .is_some_and(|s| s.player == Some(message.player))
        {
            continue;
        }
        let Ok((id, player)) = actors.players.get(message.entity) else {
            continue;
        };
        if id.0 != message.player
            || player.state.health.0 == 0
            || matches!(
                player.state.mode,
                CharacterMode::Arrested | CharacterMode::Downed
            )
        {
            continue;
        }
        let Ok(w) = wanted.get(message.entity) else {
            continue;
        };
        if !w.arrest_pending && w.arrest_ticks < 90 {
            continue;
        }
        let car_id = player.state.vehicle_id;
        let mut position = Some(player.state.motor.position.0.map(|v| v.0));
        let car_entity = car_id.and_then(|id| index.0.get(&id.0).copied());
        if let Some(car_id) = car_id {
            let Some(car_entity) = car_entity else {
                outcome.fail("arrest car missing".into());
                return;
            };
            let Ok((actual, car)) = actors.cars.get(car_entity) else {
                outcome.fail("arrest car component missing".into());
                return;
            };
            if actual.0 != car_id {
                continue;
            }
            let near = car.state.motor.position.0.map(|v| v.0);
            actors.collision.0.exclude_vehicle(Some(car_id.0));
            position = crate::systems::vehicle::safe_exit_with(
                actors.cars.get(car_entity).unwrap().1,
                &actors.collision.0,
                |p| actors.clear(p, message.player),
            );
            actors.collision.0.exclude_vehicle(None);
            position = position.or_else(|| actors.fallback(near, message.player));
        }
        let mut changed = vec![message.player.0];
        if let Some(id) = car_id {
            changed.push(id.0);
        }
        let first = if let Some(position) = position {
            let motor = match sim_core::spawn_character(position, 0.0) {
                Ok(m) => m,
                Err(e) => {
                    outcome.fail(e);
                    return;
                }
            };
            if let Some(e) = car_entity {
                let (_, mut car) = actors.cars.get_mut(e).unwrap();
                car.state.driver_id = None;
                crate::systems::vehicle::stop(&mut car);
            }
            let (_, mut player) = actors.players.get_mut(message.entity).unwrap();
            player.state.mode = CharacterMode::Arrested;
            player.state.vehicle_id = None;
            player.state.motor = motor;
            crate::systems::weapons::state::cancel(&mut player.weapon);
            player.lifecycle.protected_until = None;
            player.lifecycle.respawn = Some(message.tick.saturating_add(150));
            *wanted.get_mut(message.entity).unwrap() =
                crate::systems::police::state::clear_wanted();
            for mut unit in units
                .iter_mut()
                .filter(|u| u.target == Some(message.player.0))
            {
                crate::systems::police::state::release_target(&mut unit);
            }
            police.crimes.retain(|(id, _), _| *id != message.player.0);
            true
        } else {
            if let Some(e) = car_entity {
                crate::systems::vehicle::stop(&mut actors.cars.get_mut(e).unwrap().1);
            }
            actors
                .sessions
                .peers
                .get_mut(&message.peer)
                .unwrap()
                .inputs
                .clear();
            let mut w = wanted.get_mut(message.entity).unwrap();
            let first = !w.arrest_pending;
            w.arrest_pending = true;
            if first {
                let (_, mut p) = actors.players.get_mut(message.entity).unwrap();
                crate::systems::weapons::state::cancel(&mut p.weapon);
                p.lifecycle.protected_until = None;
            }
            first
        };
        if first {
            actors
                .sessions
                .peers
                .get_mut(&message.peer)
                .unwrap()
                .status_dirty = true;
            started.write(ArrestStarted {
                player: message.player,
                entity: message.entity,
                tick: message.tick,
            });
            controls.write(crate::gameplay_messages::ControlTransitionRequested {
                peer: message.peer,
                player: message.player,
                player_entity: message.entity,
                changed,
                tick: message.tick,
            });
        }
    }
}
pub(crate) fn fail_arrested_missions(
    mut messages: MessageReader<ArrestStarted>,
    mut missions: Query<(&GameId, &mut crate::components::PlayerMission)>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        if let Ok((id, mut mission)) = missions.get_mut(message.entity)
            && id.0 == message.player
            && let Some(m) = &mut mission.0
            && m.phase == crate::systems::missions::Phase::Active
        {
            m.fail(clock.0);
        }
    }
}
