//! Native consumers for impact presentation, occupant recovery, and crash missions.
use crate::{
    ecs::state::{Clock, Collision, Content, WorldIdentity},
    ecs::{EntityIndex, GameId},
    gameplay_messages::{
        ControlTransitionRequested, CrashEjected, DisabledVehicleOccupant, ImpactApplied,
    },
    replication::Replication,
    sessions::{SessionPhase, Sessions},
    systems::combat::CombatSystem,
};
use bevy_ecs::prelude::*;
use protocol::*;
use sim_core::{CollisionQueries, QueryFilter};
/// Returns horizontal distance between two world positions in metres.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}

/// Publishes impact effects to eligible observing sessions.
pub(crate) fn present_impacts(
    mut messages: MessageReader<ImpactApplied>,
    mut combat: ResMut<CombatSystem>,
    mut sessions: ResMut<Sessions>,
    players: Query<
        crate::component::queries::PlayerQueryReadOnly,
        crate::component::queries::PlayerFilter,
    >,
    index: Res<EntityIndex>,
    identity: Res<WorldIdentity>,
    clock: Res<Clock>,
) {
    for impact in messages.read() {
        if impact.tick != clock.0 {
            continue;
        }
        let event = WorldEvent::Impact(Impact {
            event_id: combat.event_id(),
            server_tick: impact.tick,
            entity_id: impact.target,
            position: Vec3(impact.position.map(Scalar)),
            severity: Severity(impact.severity),
        });
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .filter(|(_, s)| {
                s.player
                    .and_then(|id| index.0.get(&id.0))
                    .and_then(|e| players.get(*e).ok())
                    .is_some_and(|p| {
                        distance(p.state.motor.position.0.map(|v| v.0), impact.position) < 192.0
                    })
            })
            .map(|(peer, s)| (*peer, s.id.clone()))
            .collect();
        for (peer, sid) in recipients {
            Replication::queue(
                peer,
                sid.as_deref(),
                Delivery::Unreliable,
                ServerMessage::WorldEvent(event.clone()),
                &identity.0,
                &mut sessions,
            );
        }
    }
}

/// Requests safe recovery for players inside disabled vehicles.
pub(crate) fn find_disabled_occupants(
    sessions: Res<Sessions>,
    players: Query<
        crate::component::queries::PlayerQueryReadOnly,
        crate::component::queries::PlayerFilter,
    >,
    cars: Query<
        crate::component::queries::VehicleQueryReadOnly,
        crate::component::queries::VehicleFilter,
    >,
    index: Res<EntityIndex>,
    clock: Res<Clock>,
    mut messages: MessageWriter<DisabledVehicleOccupant>,
) {
    // Reissue while seated: a one-shot death event would strand players with blocked exits.
    for (&peer, session) in &sessions.peers {
        let Some(player) = session.player else {
            continue;
        };
        let Some(&player_entity) = index.0.get(&player.0) else {
            continue;
        };
        let Ok(p) = players.get(player_entity) else {
            continue;
        };
        let Some(car) = p.state.vehicle_id else {
            continue;
        };
        let Some(&car_entity) = index.0.get(&car.0) else {
            continue;
        };
        if cars.get(car_entity).is_ok_and(|v| v.state.health.0 == 0) {
            messages.write(DisabledVehicleOccupant {
                peer,
                player,
                player_entity,
                car,
                car_entity,
                tick: clock.0,
            });
        }
    }
}
/// Borrows actors and services needed to release occupants from disabled vehicles.
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RecoveryActors<'w, 's> {
    /// Queries players used by this system.
    pub(crate) players: Query<
        'w,
        's,
        (&'static GameId, crate::component::queries::PlayerQuery),
        crate::component::queries::PlayerFilter,
    >,
    /// Queries vehicles used by this system.
    pub(crate) cars: Query<
        'w,
        's,
        (&'static GameId, crate::component::queries::VehicleQuery),
        crate::component::queries::VehicleFilter,
    >,
    /// Queries pedestrians used by this system.
    pub(crate) peds: Query<
        'w,
        's,
        crate::component::queries::PedestrianQueryReadOnly,
        crate::component::queries::PedestrianFilter,
    >,
    /// Collision queries used to validate movement and placement.
    pub(crate) collision: NonSendMut<'w, Collision>,
    /// Validated world content used by this operation.
    pub(crate) content: Res<'w, Content>,
    /// Live session state used by this system.
    pub(crate) sessions: ResMut<'w, Sessions>,
    /// Current simulation tick resource.
    pub(crate) clock: Res<'w, Clock>,
}
impl RecoveryActors<'_, '_> {
    /// Clears transient state after its current processing batch.
    pub(crate) fn clear(&self, position: [f32; 3], except: EntityId) -> bool {
        !self
            .collision
            .0
            .overlaps_character(position, QueryFilter::MOVEMENT)
            .unwrap_or(true)
            && self.players.iter().all(|(id, p)| {
                id.0 == except
                    || p.state.mode == CharacterMode::Downed
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
            && self.peds.iter().all(|p| {
                p.state.health.0 == 0
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
    }
    /// Finds a clear fallback position when ordinary vehicle exits are blocked.
    pub(crate) fn fallback(&self, near: [f32; 3], except: EntityId) -> Option<[f32; 3]> {
        let mut candidates = Vec::new();
        for spawn in &self.content.0.manifest.spawns {
            for x in -3..=3 {
                for z in -3..=3 {
                    candidates.push([
                        spawn.position[0] as f32 + x as f32 * 1.5,
                        spawn.position[1] as f32,
                        spawn.position[2] as f32 + z as f32 * 1.5,
                    ]);
                }
            }
        }
        candidates.sort_by(|a, b| distance(*a, near).total_cmp(&distance(*b, near)));
        candidates.into_iter().find(|p| {
            self.clear(*p, except)
                && self
                    .collision
                    .0
                    .raycast(
                        [p[0], p[1] + 0.5, p[2]],
                        [0.0, -1.0, 0.0],
                        1.0,
                        QueryFilter::MOVEMENT,
                    )
                    .is_ok_and(|h| h.is_some_and(|h| h.normal[1] > 0.7))
        })
    }
}
/// Safely releases disabled vehicle occupants and updates their control state.
pub(crate) fn recover_occupants(
    mut messages: MessageReader<DisabledVehicleOccupant>,
    mut actors: RecoveryActors,
    mut ejected: MessageWriter<CrashEjected>,
    mut controls: MessageWriter<ControlTransitionRequested>,
) {
    for message in messages.read() {
        if message.tick != actors.clock.0
            || !actors
                .sessions
                .peers
                .get(&message.peer)
                .is_some_and(|s| s.player == Some(message.player))
        {
            continue;
        }
        let Ok((id, player)) = actors.players.get(message.player_entity) else {
            continue;
        };
        if id.0 != message.player || player.state.vehicle_id != Some(message.car) {
            continue;
        }
        let Ok((id, car)) = actors.cars.get(message.car_entity) else {
            continue;
        };
        if id.0 != message.car || car.state.health.0 != 0 {
            continue;
        }
        let near = car.state.motor.position.0.map(|v| v.0);
        actors.collision.0.exclude_vehicle(Some(message.car.0));
        let exit = crate::systems::vehicle::safe_exit_with(
            &actors.cars.get(message.car_entity).unwrap().1,
            &actors.collision.0,
            |p| actors.clear(p, message.player),
        );
        actors.collision.0.exclude_vehicle(None);
        let exit = exit.or_else(|| actors.fallback(near, message.player));
        let (_, mut player) = actors.players.get_mut(message.player_entity).unwrap();
        let session = actors.sessions.peers.get_mut(&message.peer).unwrap();
        if player.state.health.0 > 0 {
            player.state.health.0 = 0;
            crate::systems::weapons::state::cancel(&mut player.weapon);
            player.lifecycle.respawn = Some(message.tick.saturating_add(150));
            player.lifecycle.protected_until = None;
            session.status_dirty = true;
        }
        if let Some(exit) = exit {
            let (_, mut car) = actors.cars.get_mut(message.car_entity).unwrap();
            car.state.driver_id = None;
            car.state.disabled = true;
            player.state.health.0 = 0;
            player.state.mode = CharacterMode::Downed;
            player.state.vehicle_id = None;
            player.state.motor.0 = sim_core::spawn_character(exit, 0.0).expect("safe exit");
            crate::systems::weapons::state::cancel(&mut player.weapon);
            player
                .lifecycle
                .respawn
                .get_or_insert(message.tick.saturating_add(150));
            player.lifecycle.protected_until = None;
            session.status_dirty = true;
            ejected.write(CrashEjected {
                player: message.player,
                player_entity: message.player_entity,
                tick: message.tick,
            });
            controls.write(ControlTransitionRequested {
                peer: message.peer,
                player: message.player,
                player_entity: message.player_entity,
                changed: vec![message.player.0, message.car.0],
                tick: message.tick,
            });
        }
    }
}
/// Fails missions for players ejected by a crash.
pub(crate) fn fail_crash_missions(
    mut messages: MessageReader<CrashEjected>,
    mut missions: Query<(&GameId, &mut crate::component::PlayerMission)>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        if let Ok((id, mut mission)) = missions.get_mut(message.player_entity)
            && id.0 == message.player
            && let Some(mission) = &mut mission.0
            && mission.phase == crate::systems::missions::Phase::Active
        {
            mission.fail(message.tick);
        }
    }
}
