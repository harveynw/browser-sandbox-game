//! Player respawn and protection timers, with independent consequence readers.
use crate::{
    components::PlayerMission,
    components::{Pedestrian, Player},
    ecs::state::{Clock, Collision, Content},
    ecs::{EntityIndex, GameId},
    gameplay_messages::{ControlTransitionRequested, PlayerRespawned},
    sessions::Sessions,
};
use bevy_ecs::prelude::*;
use protocol::*;
use sim_core::{CollisionQueries, QueryFilter};

#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct RespawnActors<'w, 's> {
    players: Query<'w, 's, (&'static GameId, &'static mut Player)>,
    pedestrians: Query<'w, 's, &'static Pedestrian>,
    index: Res<'w, EntityIndex>,
    content: Res<'w, Content>,
    collision: NonSend<'w, Collision>,
}
impl RespawnActors<'_, '_> {
    fn safe_spawn(&self, id: EntityId, near: [f32; 3]) -> Option<[f32; 3]> {
        let distance = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
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
            !self
                .collision
                .0
                .overlaps_character(*p, QueryFilter::MOVEMENT)
                .unwrap_or(true)
                && self.players.iter().all(|(other, player)| {
                    other.0 == id
                        || player.state.mode == CharacterMode::Downed
                        || distance(*p, player.state.motor.position.0.map(|v| v.0)) > 1.0
                })
                && self.pedestrians.iter().all(|ped| {
                    ped.state.health.0 == 0
                        || distance(*p, ped.state.motor.position.0.map(|v| v.0)) > 1.0
                })
                && self
                    .collision
                    .0
                    .raycast(
                        [p[0], p[1] + 0.5, p[2]],
                        [0.0, -1.0, 0.0],
                        1.0,
                        QueryFilter::MOVEMENT,
                    )
                    .is_ok_and(|hit| hit.is_some_and(|hit| hit.normal[1] > 0.7))
        })
    }
}
pub(crate) fn respawn_players(
    mut actors: RespawnActors,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    mut respawned: MessageWriter<PlayerRespawned>,
    mut controls: MessageWriter<ControlTransitionRequested>,
) {
    // Retain peer order, and make each committed spawn visible to the next player's search.
    for (&peer, session) in &mut sessions.peers {
        let Some(id) = session.player else {
            continue;
        };
        let Some(&entity) = actors.index.0.get(&id.0) else {
            continue;
        };
        let Ok((game_id, mut player)) = actors.players.get_mut(entity) else {
            continue;
        };
        if game_id.0 != id {
            continue;
        }
        if player
            .lifecycle
            .protected_until
            .is_some_and(|end| clock.0 >= end)
        {
            player.lifecycle.protected_until = None;
            session.status_dirty = true;
        }
        if player.lifecycle.respawn.is_none_or(|end| clock.0 < end) {
            continue;
        }
        let near = player.state.motor.position.0.map(|v| v.0);
        let Some(position) = actors.safe_spawn(id, near) else {
            continue;
        };
        let (_, mut player) = actors.players.get_mut(entity).expect("validated player");
        let sequence = player.weapon.shot_sequence;
        player.state.motor = sim_core::spawn_character(position, 0.0).expect("safe spawn");
        player.state.health = CharacterHealth(100);
        player.state.mode = CharacterMode::OnFoot;
        player.state.vehicle_id = None;
        player.weapon = crate::systems::weapons::state::loadout();
        player.weapon.shot_sequence = sequence;
        player.lifecycle.protected_until = Some(clock.0.saturating_add(90));
        player.lifecycle.respawn = None;
        session.status_dirty = true;
        respawned.write(PlayerRespawned {
            player: id,
            entity,
            tick: clock.0,
        });
        controls.write(ControlTransitionRequested {
            peer,
            player: id,
            player_entity: entity,
            changed: vec![id.0],
            tick: clock.0,
        });
    }
}
pub(crate) fn reset_respawned_missions(
    mut messages: MessageReader<PlayerRespawned>,
    mut missions: Query<(&GameId, &mut PlayerMission)>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        if let Ok((id, mut mission)) = missions.get_mut(message.entity)
            && id.0 == message.player
        {
            mission.0 = None;
        }
    }
}

pub(crate) fn down_players(
    mut deaths: MessageReader<crate::gameplay_messages::PlayerDeathRequested>,
    mut players: Query<(&GameId, &mut Player)>,
    mut sessions: ResMut<Sessions>,
    clock: Res<Clock>,
    mut downed: MessageWriter<crate::gameplay_messages::PlayerDowned>,
    mut controls: MessageWriter<ControlTransitionRequested>,
) {
    for death in deaths.read() {
        if death.tick != clock.0 {
            continue;
        }
        let Ok((id, mut player)) = players.get_mut(death.entity) else {
            continue;
        };
        if id.0 != death.player
            || player.state.health.0 != 0
            || player.state.mode != CharacterMode::OnFoot
            || player.lifecycle.respawn.is_some()
        {
            continue;
        }
        player.state.mode = CharacterMode::Downed;
        player.state.motor.velocity = Vec3([Scalar(0.0); 3]);
        crate::systems::weapons::state::cancel(&mut player.weapon);
        player.lifecycle.protected_until = None;
        player.lifecycle.respawn = Some(death.tick.saturating_add(150));
        downed.write(crate::gameplay_messages::PlayerDowned(*death));
        if let Some((&peer, session)) = sessions
            .peers
            .iter_mut()
            .find(|(_, s)| s.player == Some(death.player))
        {
            session.status_dirty = true;
            controls.write(ControlTransitionRequested {
                peer,
                player: death.player,
                player_entity: death.entity,
                changed: vec![death.player.0],
                tick: death.tick,
            });
        }
    }
}
pub(crate) fn fail_downed_missions(
    mut downed: MessageReader<crate::gameplay_messages::PlayerDowned>,
    mut missions: Query<(&GameId, &mut PlayerMission)>,
    clock: Res<Clock>,
) {
    for event in downed.read() {
        let death = event.0;
        if death.tick != clock.0 {
            continue;
        }
        if let Ok((id, mut mission)) = missions.get_mut(death.entity)
            && id.0 == death.player
            && let Some(mission) = &mut mission.0
            && mission.phase == crate::systems::missions::Phase::Active
        {
            mission.fail(death.tick);
        }
    }
}
pub(crate) fn advance_weapon_timers(
    mut players: Query<&mut Player>,
    index: Res<EntityIndex>,
    clock: Res<Clock>,
    mut sessions: ResMut<Sessions>,
) {
    for session in sessions.peers.values_mut() {
        let Some(entity) = session.player.and_then(|id| index.0.get(&id.0)) else {
            continue;
        };
        if let Ok(mut player) = players.get_mut(*entity)
            && crate::systems::weapons::state::advance(&mut player.weapon, clock.0)
        {
            session.status_dirty = true;
        }
    }
}
/// Retire lifecycle messages after their checkpoint readers have run.
pub(crate) fn retire_death_messages(
    mut deaths: ResMut<Messages<crate::gameplay_messages::PlayerDeathRequested>>,
    mut downed: ResMut<Messages<crate::gameplay_messages::PlayerDowned>>,
    mut occupants: ResMut<Messages<crate::gameplay_messages::DisabledVehicleOccupant>>,
    mut ejected: ResMut<Messages<crate::gameplay_messages::CrashEjected>>,
) {
    deaths.clear();
    downed.clear();
    occupants.clear();
    ejected.clear();
}
