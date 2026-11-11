//! Component-owned session lifecycle and packet-ordered possession actions.
use super::*;
use crate::{admission::Profiles, replication::Replication};
use crate::{
    components::{PlayerMission, RecoveryReservation, SecuredVehicle},
    ecs::state::{Clock, Collision, WorldIdentity},
    ecs::{EntityIndex, GameId},
    gameplay_messages::{ControlTransitionRequested, CrimeReported},
    systems::{
        police::{CrimeId, CrimeKind},
        population::{Traffic, TrafficPhase},
    },
};
use bevy_ecs::prelude::*;
use std::result::Result;
#[derive(Message, Clone)]
pub(crate) struct PossessionRequested {
    pub peer: PeerKey,
    pub request: ActionRequest,
    pub entity: Option<Entity>,
    pub tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct PossessionResult {
    peer: PeerKey,
    result: ActionResult,
    tick: u32,
}
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct PossessionActors<'w, 's> {
    players: Query<
        'w,
        's,
        (
            &'static GameId,
            &'static mut Player,
            &'static WantedComponent,
        ),
    >,
    cars: Query<
        'w,
        's,
        (
            &'static GameId,
            &'static mut VehicleComponent,
            Option<&'static PoliceUnitComponent>,
            Option<&'static Traffic>,
            Option<&'static RecoveryReservation>,
            Option<&'static SecuredVehicle>,
        ),
    >,
    index: Res<'w, EntityIndex>,
    collision: NonSendMut<'w, Collision>,
    controls: MessageWriter<'w, ControlTransitionRequested>,
    crimes: MessageWriter<'w, CrimeReported>,
}
impl PossessionActors<'_, '_> {
    fn apply(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        entity: Entity,
        action: &Action,
        tick: u32,
    ) -> ActionCode {
        let Some(id) = s.player else {
            return ActionCode::NotReady;
        };
        if self.index.0.get(&id.0) != Some(&entity) {
            return ActionCode::NotReady;
        }
        let Ok((actual, p, w)) = self.players.get(entity) else {
            return ActionCode::NotReady;
        };
        if actual.0 != id {
            return ActionCode::NotReady;
        }
        if w.arrest_pending
            || p.state.health.0 == 0
            || matches!(
                p.state.mode,
                CharacterMode::Downed | CharacterMode::Arrested
            )
        {
            return ActionCode::Dead;
        }
        let car_id;
        match action {
            Action::EnterVehicle(r) => {
                if p.state.mode != CharacterMode::OnFoot {
                    return ActionCode::Unavailable;
                }
                car_id = r.vehicle_id;
                let Some(&car_entity) = self.index.0.get(&car_id.0) else {
                    return ActionCode::InvalidTarget;
                };
                let Ok((_, car, unit, traffic, reservation, secured)) = self.cars.get(car_entity)
                else {
                    return ActionCode::InvalidTarget;
                };
                if !s.visible.contains_key(&car_id.0) {
                    return ActionCode::OutOfRange;
                }
                if reservation.is_some_and(|r| r.0.owner != id.0)
                    || car.state.driver_id.is_some()
                    || unit.is_some()
                    || traffic.is_some_and(|b| b.phase != TrafficPhase::Available)
                {
                    return ActionCode::Occupied;
                }
                if car.state.disabled || car.state.health.0 == 0 {
                    return ActionCode::Unavailable;
                }
                if car.state.motor.speed.0.abs() >= 0.5 {
                    return ActionCode::TooFast;
                }
                let feet = p.state.motor.position.0.map(|v| v.0);
                let entry = crate::systems::vehicle::entry(car);
                if feet
                    .iter()
                    .zip(entry)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>()
                    .sqrt()
                    > 2.0
                {
                    return ActionCode::OutOfRange;
                }
                let secured = secured.is_some();
                self.collision.0.exclude_vehicle(Some(car_id.0));
                let clear = crate::systems::vehicle::clear_segment(
                    &self.collision.0,
                    [feet[0], feet[1] + 1.45, feet[2]],
                    [entry[0], entry[1] + 0.9, entry[2]],
                );
                self.collision.0.exclude_vehicle(None);
                if !clear {
                    return ActionCode::OutOfRange;
                }
                if secured {
                    let police: Vec<_> = self
                        .cars
                        .iter()
                        .filter(|(_, _, u, _, _, _)| u.is_some())
                        .map(|(id, c, _, _, _, _)| {
                            (
                                id.0.0,
                                c.state.health.0,
                                c.state.motor.position.0.map(|v| v.0),
                            )
                        })
                        .collect();
                    if let Some(report) = crate::systems::police::observe_crime_snapshot(
                        CrimeKind::Theft,
                        feet,
                        feet,
                        &police,
                        &self.collision.0,
                    ) {
                        self.crimes.write(CrimeReported {
                            actor: id,
                            actor_entity: entity,
                            cause: CrimeId::Theft(tick, car_id.0),
                            report,
                        });
                    }
                }
                let (_, mut car, _, _, _, _) = self.cars.get_mut(car_entity).unwrap();
                let motor = sim_core::spawn_character(
                    crate::systems::vehicle::seat(&car),
                    car.state.motor.yaw.0,
                )
                .expect("validated seat");
                car.state.driver_id = Some(id);
                car.idle_ticks = 0;
                let (_, mut p, _) = self.players.get_mut(entity).unwrap();
                crate::systems::weapons::state::cancel(&mut p.weapon);
                p.lifecycle.protected_until = None;
                s.status_dirty = true;
                p.state.mode = CharacterMode::Driving;
                p.state.vehicle_id = Some(car_id);
                p.state.motor = motor;
            }
            Action::ExitVehicle(_) => {
                let Some(id) = p.state.vehicle_id else {
                    return ActionCode::InvalidTarget;
                };
                car_id = id;
                let Some(&car_entity) = self.index.0.get(&id.0) else {
                    return ActionCode::InvalidTarget;
                };
                let Ok((_, car, _, _, _, _)) = self.cars.get(car_entity) else {
                    return ActionCode::InvalidTarget;
                };
                if car.state.driver_id != s.player {
                    return ActionCode::InvalidTarget;
                }
                if car.state.motor.speed.0.abs() >= 2.0 {
                    return ActionCode::TooFast;
                }
                self.collision.0.exclude_vehicle(Some(id.0));
                let exit = crate::systems::vehicle::safe_exit(car, &self.collision.0);
                self.collision.0.exclude_vehicle(None);
                let Some(exit) = exit else {
                    return ActionCode::ExitBlocked;
                };
                let yaw = car.state.motor.yaw.0;
                let (_, mut car, _, _, _, _) = self.cars.get_mut(car_entity).unwrap();
                car.state.driver_id = None;
                crate::systems::vehicle::stop(&mut car);
                let (_, mut p, _) = self.players.get_mut(entity).unwrap();
                p.state.mode = CharacterMode::OnFoot;
                p.state.vehicle_id = None;
                p.state.motor = sim_core::spawn_character(exit, yaw).expect("validated exit");
            }
            _ => return ActionCode::Unavailable,
        }
        self.controls.write(ControlTransitionRequested {
            peer,
            player: id,
            player_entity: entity,
            changed: vec![id.0, car_id.0],
            tick,
        });
        ActionCode::Ok
    }
}
pub(crate) fn possess(
    mut requests: MessageReader<PossessionRequested>,
    mut actors: PossessionActors,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    mut results: MessageWriter<PossessionResult>,
) {
    for m in requests.read() {
        if m.tick != clock.0 {
            continue;
        }
        let overflow = sessions
            .peers
            .values()
            .any(|s| s.visibility_counter > u32::MAX - 2);
        let Some(mut s) = sessions.peers.remove(&m.peer) else {
            continue;
        };
        let r = &m.request;
        let result = if let Some(cached) = s.actions.get(&r.request_id.0) {
            cached.clone()
        } else {
            let code = if s.phase != SessionPhase::Playing {
                ActionCode::NotReady
            } else if r.control_epoch.0 != s.epoch {
                ActionCode::StaleEpoch
            } else if r.request_id.0 <= s.last_request {
                ActionCode::StaleRequest
            } else if overflow || s.epoch == u32::MAX {
                ActionCode::Unavailable
            } else if let Some(e) = m.entity {
                actors.apply(m.peer, &mut s, e, &r.action, clock.0)
            } else {
                ActionCode::NotReady
            };
            let affected = if code == ActionCode::Ok {
                s.player
                    .and_then(|id| actors.index.0.get(&id.0))
                    .and_then(|e| actors.players.get(*e).ok())
                    .map(|(id, p, _)| p.state.vehicle_id.unwrap_or(id.0))
            } else {
                None
            };
            let result = ActionResult {
                request_id: r.request_id,
                applied_tick: clock.0,
                ok: code == ActionCode::Ok,
                code,
                affected_entity_id: affected,
            };
            if r.request_id.0 > s.last_request {
                s.last_request = r.request_id.0;
                s.actions.insert(r.request_id.0, result.clone());
                while s.actions.len() > 32 {
                    s.actions.pop_first();
                }
            }
            result
        };
        sessions.peers.insert(m.peer, s);
        results.write(PossessionResult {
            peer: m.peer,
            result,
            tick: clock.0,
        });
    }
}
pub(crate) fn reply_possession(
    mut messages: MessageReader<PossessionResult>,
    mut sessions: ResMut<Sessions>,
    identity: Res<WorldIdentity>,
    clock: Res<Clock>,
) {
    for m in messages.read() {
        if m.tick != clock.0 {
            continue;
        }
        let Some(s) = sessions.peers.get(&m.peer) else {
            continue;
        };
        let sid = s.id.clone();
        Replication::queue(
            m.peer,
            sid.as_deref(),
            Delivery::Reliable,
            ServerMessage::ActionResult(m.result.clone()),
            &identity.0,
            &mut sessions,
        );
    }
}
pub(crate) fn retire_possession(
    mut requests: ResMut<Messages<PossessionRequested>>,
    mut replies: ResMut<Messages<PossessionResult>>,
) {
    requests.clear();
    replies.clear();
}

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct LifecycleActors<'w, 's> {
    players: Query<'w, 's, (Entity, &'static GameId, &'static mut Player)>,
    cars: Query<
        'w,
        's,
        (
            Entity,
            &'static GameId,
            &'static mut VehicleComponent,
            Option<&'static RecoveryReservation>,
        ),
    >,
    units: Query<'w, 's, &'static mut PoliceUnitComponent>,
    commands: Commands<'w, 's>,
}
pub(crate) fn remove_player(id: EntityId, actors: &mut LifecycleActors) {
    for (entity, car_id, mut car, reservation) in &mut actors.cars {
        if reservation.is_some_and(|r| r.0.owner == id.0) {
            crate::systems::planning::despawn(&mut actors.commands, car_id.0.0, entity);
        } else if car.state.driver_id == Some(id) {
            car.state.driver_id = None;
            crate::systems::vehicle::stop(&mut car);
        }
    }
    for mut unit in actors.units.iter_mut().filter(|u| u.target == Some(id.0)) {
        crate::systems::police::state::release_target(&mut unit);
    }
    if let Some((entity, _, _)) = actors.players.iter().find(|(_, actual, _)| actual.0 == id) {
        crate::systems::planning::despawn(&mut actors.commands, id.0, entity);
    }
}
pub(crate) fn create_player(
    profile: &ProfileView,
    content: &crate::content_loader::WorldContent,
    next: &mut u32,
    actors: &mut LifecycleActors,
) -> Result<EntityId, &'static str> {
    if actors.players.iter().count() >= MAX_PLAYERS {
        return Err("shard full");
    }
    let slot = (0..MAX_PLAYERS)
        .find(|slot| actors.players.iter().all(|(_, _, p)| p.slot != *slot))
        .ok_or("spawn slots full")?;
    let id = EntityId(*next);
    *next = next.checked_add(1).ok_or("entity IDs exhausted")?;
    let position = spawn_position(content, slot).map_err(|_| "spawn unavailable")?;
    let motor = sim_core::spawn_character(position, content.manifest.spawns[0].yaw as f32)
        .map_err(|_| "spawn unavailable")?;
    crate::systems::planning::spawn(
        &mut actors.commands,
        id.0,
        (
            Player {
                tshirt_colour: profile.tshirt_colour,
                weapon: crate::systems::weapons::state::loadout(),
                lifecycle: Default::default(),
                id,
                display_name: profile.display_name.clone(),
                slot,
                state: CharacterState {
                    equipment: None,
                    motor,
                    health: CharacterHealth(100),
                    mode: CharacterMode::OnFoot,
                    vehicle_id: None,
                },
            },
            crate::systems::police::state::clear_wanted(),
            PlayerMission(None),
            crate::components::ChatParticipant::default(),
        ),
    );
    Ok(id)
}

pub(crate) fn protect_on_ready(
    id: EntityId,
    tick: u32,
    actors: &mut LifecycleActors,
) -> Result<(), &'static str> {
    let Some((_, _, mut p)) = actors
        .players
        .iter_mut()
        .find(|(_, actual, _)| actual.0 == id)
    else {
        return Err("ready player missing");
    };
    p.lifecycle.protected_until = Some(tick.saturating_add(90));
    Ok(())
}
pub(crate) fn freeze_on_resync(
    id: EntityId,
    actors: &mut LifecycleActors,
) -> Result<(), &'static str> {
    let Some((_, _, mut p)) = actors
        .players
        .iter_mut()
        .find(|(_, actual, _)| actual.0 == id)
    else {
        return Ok(());
    };
    p.weapon.trigger = None;
    p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
    if let Some(id) = p.state.vehicle_id {
        let Some((_, _, mut car, _)) = actors
            .cars
            .iter_mut()
            .find(|(_, actual, _, _)| actual.0 == id)
        else {
            return Err("missing car");
        };
        crate::systems::vehicle::stop(&mut car);
    }
    Ok(())
}
pub(crate) fn apply_appearance(
    sessions: &mut Sessions,
    profiles: &Profiles,
    actors: &mut LifecycleActors,
) -> Vec<u32> {
    let mut changed = Vec::new();
    for s in sessions.peers.values_mut() {
        let (Some(id), Some(profile)) = (s.player, s.profile.as_mut()) else {
            continue;
        };
        let Some(committed) = profiles.committed(&profile.profile_id.0) else {
            continue;
        };
        let Some((_, _, mut p)) = actors
            .players
            .iter_mut()
            .find(|(_, actual, _)| actual.0 == id)
        else {
            continue;
        };
        if p.tshirt_colour != committed.tshirt_colour {
            p.tshirt_colour = committed.tshirt_colour;
            changed.push(id.0);
        }
        profile.tshirt_colour = committed.tshirt_colour;
    }
    changed
}

/// PeerKey includes the connection generation; stale failures cannot remove a
/// replacement connection. Tick scoping also prevents failed-tick replay.
#[derive(Message, Clone)]
pub(crate) struct DisconnectRequested {
    pub peer: PeerKey,
    pub reason: &'static str,
    pub tick: u32,
}
pub(crate) fn disconnect_requested(
    mut requests: MessageReader<DisconnectRequested>,
    players: Query<(Entity, &GameId, &Player)>,
    mut sessions: ResMut<Sessions>,
    mut io: NonSendMut<crate::ecs::runtime::TickIo>,
    mut leaving: MessageWriter<crate::gameplay_messages::PlayerLeaving>,
    clock: Res<Clock>,
) {
    for request in requests.read().filter(|r| r.tick == clock.0) {
        if let Some(session) = sessions.peers.remove(&request.peer) {
            if let Some(id) = session.player {
                let player = players.iter().find(|(_, actual, _)| actual.0 == id);
                leaving.write(crate::gameplay_messages::PlayerLeaving {
                    peer: request.peer,
                    player: id,
                    entity: player.map(|(entity, _, _)| entity),
                    name: player.map(|(_, _, p)| p.display_name.clone()),
                    tick: clock.0,
                });
            }
            if let (Some(profile), Some(sid)) = (&session.profile, &session.id) {
                io.profiles.deactivate(&profile.profile_id.0, sid);
            }
        }
        sessions.closing.insert(request.peer, request.reason);
    }
}
/// Commit actor retirement only after all leave subscribers have run.
pub(crate) fn finish_leaving(
    mut leaving: MessageReader<crate::gameplay_messages::PlayerLeaving>,
    mut actors: LifecycleActors,
    clock: Res<Clock>,
) {
    for message in leaving.read().filter(|m| m.tick == clock.0) {
        if let Some(entity) = message.entity
            && !actors
                .players
                .get(entity)
                .is_ok_and(|(_, id, _)| id.0 == message.player)
        {
            continue;
        }
        remove_player(message.player, &mut actors);
    }
}
pub(crate) fn retire_leaving(
    mut messages: ResMut<Messages<crate::gameplay_messages::PlayerLeaving>>,
) {
    messages.clear();
}
pub(crate) fn retire_disconnects(mut requests: ResMut<Messages<DisconnectRequested>>) {
    requests.clear();
}
pub(crate) fn failed_motors(
    mut scratch: ResMut<crate::ecs::runtime::Scratch>,
    mut outcome: ResMut<crate::ecs::TickOutcome>,
    clock: Res<Clock>,
    mut requests: MessageWriter<DisconnectRequested>,
) {
    if !scratch.enter(crate::ecs::TickPhase::FailedPeers, &mut outcome) {
        return;
    }
    match scratch.frame() {
        Ok(frame) => {
            for peer in std::mem::take(&mut frame.failed) {
                requests.write(DisconnectRequested {
                    peer,
                    reason: "unsafe motor input/state",
                    tick: clock.0,
                });
            }
        }
        Err(error) => outcome.fail(error),
    }
}
