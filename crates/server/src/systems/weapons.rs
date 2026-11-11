//! Native weapon intent, ordered shot execution, damage and presentation.
use crate::{
    ecs::state::Clock,
    ecs::{EntityIndex, GameId},
    entities::player::Player,
    gameplay_messages::AutomaticShotRequested,
    sessions::{SessionPhase, Sessions},
    systems::police::Wanted,
};
use bevy_ecs::prelude::*;
pub(crate) fn request_automatic_shots(
    mut players: Query<(&GameId, &mut Player, &Wanted)>,
    sessions: Res<Sessions>,
    index: Res<EntityIndex>,
    clock: Res<Clock>,
    mut shots: MessageWriter<AutomaticShotRequested>,
) {
    for (&peer, session) in &sessions.peers {
        let Some(id) = session.player else {
            continue;
        };
        let Some(&entity) = index.0.get(&id.0) else {
            continue;
        };
        let Ok((game_id, mut player, wanted)) = players.get_mut(entity) else {
            continue;
        };
        if game_id.0 != id {
            continue;
        }
        if player.state.mode != protocol::CharacterMode::OnFoot
            || player.state.health.0 == 0
            || wanted.arrest_pending
            || session.phase != SessionPhase::Playing
        {
            player.weapon.trigger = None;
            continue;
        }
        if let Some(aim) = player.weapon.automatic_aim(clock.0, session.epoch)
            && player.weapon.can_fire(&aim, true, clock.0).is_ok()
        {
            shots.write(AutomaticShotRequested {
                peer,
                player: id,
                entity,
                tick: clock.0,
                epoch: session.epoch,
                aim,
            });
        }
    }
}

pub(crate) fn present_shots(
    mut shots: MessageReader<crate::gameplay_messages::ShotEmitted>,
    mut sessions: ResMut<Sessions>,
    players: Query<&Player>,
    index: Res<EntityIndex>,
    identity: Res<crate::ecs::state::WorldIdentity>,
    clock: Res<Clock>,
) {
    for shot in shots.read() {
        if shot.tick != clock.0 {
            continue;
        }
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(peer, session)| {
                Some(**peer) == shot.source
                    || (session.phase == SessionPhase::Playing
                        && session
                            .player
                            .and_then(|id| index.0.get(&id.0))
                            .and_then(|entity| players.get(*entity).ok())
                            .is_some_and(|player| {
                                let p = player.state.motor.position.0.map(|v| v.0);
                                (p[0] - shot.position[0]).hypot(p[2] - shot.position[2]) < 192.0
                            }))
            })
            .map(|(&peer, session)| (peer, session.id.clone()))
            .collect();
        for (peer, sid) in recipients {
            crate::replication::Replication::queue(
                peer,
                sid.as_deref(),
                protocol::Delivery::Reliable,
                protocol::ServerMessage::WorldEvent(protocol::WorldEvent::Shot(shot.event.clone())),
                &identity.0,
                &mut sessions,
            );
        }
    }
}
pub(crate) fn retire_shots(mut shots: ResMut<Messages<crate::gameplay_messages::ShotEmitted>>) {
    shots.clear();
}

use crate::{
    ecs::state::{Collision, WorldIdentity},
    entities::{pedestrian::Pedestrian, vehicle::Vehicle},
    gameplay_messages::{
        ActorEliminated, CrimeReported, NoiseEmitted, PlayerDeathRequested, ShotEmitted,
    },
    systems::{
        combat::CombatSystem,
        police::{CrimeId, CrimeKind, Officer, OfficerShot, Unit},
    },
    transport::PeerKey,
};
use protocol::*;
use sim_core::{CollisionQueries, QueryFilter};

#[derive(Clone, Copy)]
enum ShotTarget {
    Vehicle(u32),
    Pedestrian(u32),
    Player(u32),
    Officer(u32),
}

#[derive(Message, Clone)]
pub(crate) struct WeaponActionRequested {
    pub peer: PeerKey,
    pub request: ActionRequest,
    pub entity: Option<Entity>,
    pub tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct OfficerShotRequested {
    pub shot: OfficerShot,
    pub shooter: Entity,
    pub target: Entity,
    pub tick: u32,
}

/// Ordered shot execution reads current components for every ray and applies
/// damage before the next ray. No cached actor state crosses a shot boundary.
#[derive(bevy_ecs::system::SystemParam)]
pub(crate) struct Weapons<'w, 's> {
    players: Query<'w, 's, (Entity, &'static GameId, &'static mut Player)>,
    cars: Query<
        'w,
        's,
        (
            Entity,
            &'static GameId,
            &'static mut Vehicle,
            Option<&'static Unit>,
        ),
    >,
    peds: Query<'w, 's, (Entity, &'static GameId, &'static mut Pedestrian)>,
    officers: Query<'w, 's, (Entity, &'static GameId, &'static mut Officer)>,
    wanted: Query<'w, 's, &'static mut Wanted>,
    index: Res<'w, EntityIndex>,
    clock: Res<'w, Clock>,
    collision: NonSend<'w, Collision>,
    sessions: ResMut<'w, Sessions>,
    identity: Res<'w, WorldIdentity>,
    events: ResMut<'w, CombatSystem>,
    commands: Commands<'w, 's>,
    deaths: MessageWriter<'w, PlayerDeathRequested>,
    eliminations: MessageWriter<'w, ActorEliminated>,
    crimes: MessageWriter<'w, CrimeReported>,
    noises: MessageWriter<'w, NoiseEmitted>,
    shots: MessageWriter<'w, ShotEmitted>,
}
impl Weapons<'_, '_> {
    fn trace(
        &self,
        shooter: EntityId,
        origin: [f32; 3],
        direction: [f32; 3],
        range: f32,
    ) -> (f32, Option<(Entity, ShotTarget)>) {
        let mut nearest = self
            .collision
            .0
            .raycast(origin, direction, range, QueryFilter::VISION)
            .ok()
            .flatten()
            .map_or(range, |h| h.distance);
        if direction[1] < 0.0 {
            nearest = nearest.min((-origin[1] / direction[1]).max(0.0));
        }
        let mut target = None;
        let mut cars: Vec<_> = self.cars.iter().collect();
        cars.sort_by_key(|(_, id, _, _)| id.0.0);
        for (entity, id, car, _) in cars {
            if let Some(d) = sim_core::collision::ray_vehicle(
                origin,
                direction,
                car.state.motor.position.0.map(|v| v.0),
                car.state.motor.yaw.0,
                car.template.dimensions.map(|v| v as f32),
                range,
            ) && d < nearest
            {
                nearest = d;
                target = Some((entity, ShotTarget::Vehicle(id.0.0)));
            }
        }
        let mut peds: Vec<_> = self.peds.iter().collect();
        peds.sort_by_key(|(_, id, _)| id.0.0);
        for (entity, id, p) in peds {
            if p.state.health.0 > 0
                && let Some(d) = sim_core::collision::ray_character(
                    origin,
                    direction,
                    p.state.motor.position.0.map(|v| v.0),
                    range,
                )
                && d < nearest
            {
                nearest = d;
                target = Some((entity, ShotTarget::Pedestrian(id.0.0)));
            }
        }
        let mut players: Vec<_> = self.players.iter().collect();
        players.sort_by_key(|(_, id, _)| id.0.0);
        for (entity, id, p) in players {
            if id.0 != shooter
                && p.state.health.0 > 0
                && p.state.mode == CharacterMode::OnFoot
                && let Some(d) = sim_core::collision::ray_character(
                    origin,
                    direction,
                    p.state.motor.position.0.map(|v| v.0),
                    range,
                )
                && d < nearest
            {
                nearest = d;
                target = Some((entity, ShotTarget::Player(id.0.0)));
            }
        }
        let mut officers: Vec<_> = self.officers.iter().collect();
        officers.sort_by_key(|(_, id, _)| id.0.0);
        for (entity, id, p) in officers {
            if id.0 != shooter
                && p.state.health.0 > 0
                && !p.mounted
                && let Some(d) = sim_core::collision::ray_character(
                    origin,
                    direction,
                    p.state.motor.position.0.map(|v| v.0),
                    range,
                )
                && d < nearest
            {
                nearest = d;
                target = Some((entity, ShotTarget::Officer(id.0.0)));
            }
        }
        (nearest, target)
    }
    fn damage(
        &mut self,
        entity: Entity,
        target: ShotTarget,
        amount: u16,
        attacker: Option<EntityId>,
    ) -> Option<EntityId> {
        match target {
            ShotTarget::Vehicle(id) => {
                let (_, game_id, mut car, _) = self.cars.get_mut(entity).ok()?;
                if game_id.0 != EntityId(id) || !car.apply_damage(amount, attacker) {
                    return None;
                }
                if car.state.health.0 == 0 {
                    self.commands
                        .entity(entity)
                        .insert_if_new(crate::metadata::DisabledVehicle((
                            self.clock.0.saturating_add(900),
                            0,
                        )));
                }
                Some(EntityId(id))
            }
            ShotTarget::Pedestrian(id) => {
                let (_, game_id, mut p) = self.peds.get_mut(entity).ok()?;
                if game_id.0 != EntityId(id) {
                    return None;
                }
                p.apply_damage(amount as u8);
                if p.state.health.0 == 0 {
                    self.commands
                        .entity(entity)
                        .insert_if_new(crate::metadata::Incapacitated(
                            self.clock.0.saturating_add(300),
                        ));
                }
                Some(EntityId(id))
            }
            ShotTarget::Officer(id) => {
                let (_, game_id, mut p) = self.officers.get_mut(entity).ok()?;
                if game_id.0 != EntityId(id) {
                    return None;
                }
                p.state.health.0 = p.state.health.0.saturating_sub(amount);
                if p.state.health.0 == 0 {
                    p.state.mode = CharacterMode::Downed;
                    p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
                    p.state.equipment = None;
                }
                Some(EntityId(id))
            }
            ShotTarget::Player(id) => {
                let killer_name = attacker
                    .and_then(|id| self.index.0.get(&id.0))
                    .and_then(|e| self.players.get(*e).ok())
                    .map(|(_, _, p)| p.display_name.clone())
                    .unwrap_or_else(|| DisplayName("Police".into()));
                let (_, game_id, mut p) = self.players.get_mut(entity).ok()?;
                if game_id.0 != EntityId(id)
                    || p.lifecycle
                        .protected_until
                        .is_some_and(|end| self.clock.0 < end)
                {
                    return None;
                }
                p.state.health.0 = p.state.health.0.saturating_sub(amount);
                if p.state.health.0 == 0 {
                    self.deaths.write(PlayerDeathRequested {
                        player: game_id.0,
                        entity,
                        tick: self.clock.0,
                    });
                    self.eliminations.write(ActorEliminated {
                        killer: attacker,
                        killer_name,
                        victim: game_id.0,
                        victim_name: p.display_name.clone(),
                        tick: self.clock.0,
                    });
                }
                Some(EntityId(id))
            }
        }
    }
    fn crime(
        &mut self,
        actor: EntityId,
        entity: Entity,
        cause: CrimeId,
        kind: CrimeKind,
        at: [f32; 3],
    ) {
        let Ok((_, _, p)) = self.players.get(entity) else {
            return;
        };
        let police: Vec<_> = self
            .cars
            .iter()
            .filter(|(_, _, _, unit)| unit.is_some())
            .map(|(_, id, car, _)| {
                (
                    id.0.0,
                    car.state.health.0,
                    car.state.motor.position.0.map(|v| v.0),
                )
            })
            .collect();
        if let Some(report) = crate::systems::police::observe_crime_snapshot(
            kind,
            at,
            p.state.motor.position.0.map(|v| v.0),
            &police,
            &self.collision.0,
        ) {
            self.crimes.write(CrimeReported {
                actor,
                actor_entity: entity,
                cause,
                report,
            });
        }
    }
    fn player_action(
        &mut self,
        peer: PeerKey,
        entity: Entity,
        action: &Action,
        automatic: bool,
    ) -> ActionCode {
        let Some(session) = self.sessions.peers.get(&peer) else {
            return ActionCode::NotReady;
        };
        let Ok((_, id, p)) = self.players.get(entity) else {
            return ActionCode::NotReady;
        };
        let id = id.0;
        if session.player != Some(id) || self.index.0.get(&id.0) != Some(&entity) {
            return ActionCode::NotReady;
        }
        if !automatic
            && (session.epoch == u32::MAX
                || self
                    .sessions
                    .peers
                    .values()
                    .any(|s| s.visibility_counter > u32::MAX - 2))
        {
            return ActionCode::Unavailable;
        }
        if p.state.health.0 == 0
            || matches!(
                p.state.mode,
                CharacterMode::Downed | CharacterMode::Arrested
            )
            || self.wanted.get(entity).is_ok_and(|w| w.arrest_pending)
        {
            return ActionCode::Dead;
        }
        if p.state.mode != CharacterMode::OnFoot {
            return ActionCode::Unavailable;
        }
        match action {
            Action::EquipWeapon(request) => {
                let code = self
                    .players
                    .get_mut(entity)
                    .unwrap()
                    .2
                    .weapon
                    .equip(request.weapon_id, self.clock.0);
                self.sessions.peers.get_mut(&peer).unwrap().status_dirty = true;
                code
            }
            Action::Reload(_) => {
                let code = self
                    .players
                    .get_mut(entity)
                    .unwrap()
                    .2
                    .weapon
                    .reload(self.clock.0);
                self.sessions.peers.get_mut(&peer).unwrap().status_dirty = true;
                code
            }
            Action::Fire(aim) => {
                let (origin, direction, definition, sequence, weapon) = {
                    let (_, _, mut player) = self.players.get_mut(entity).unwrap();
                    let definition = match player.weapon.begin_fire(aim, automatic, self.clock.0) {
                        Ok(d) => d,
                        Err(code) => return code,
                    };
                    player.lifecycle.protected_until = None;
                    let (origin, direction) = player.shot_ray(aim, self.clock.0);
                    player.weapon.bloom = (player.weapon.bloom + definition.bloom_per_shot as f32)
                        .min(definition.bloom_max as f32);
                    player.weapon.shot_sequence += 1;
                    (
                        origin,
                        direction,
                        definition,
                        player.weapon.shot_sequence,
                        player.weapon.equipped,
                    )
                };
                self.sessions.peers.get_mut(&peer).unwrap().status_dirty = true;
                if let Ok(mut wanted) = self.wanted.get_mut(entity) {
                    wanted.surrendering = false;
                }
                let (distance, target) = self.trace(id, origin, direction, definition.range as f32);
                let hit = target.and_then(|(e, t)| self.damage(e, t, definition.damage, Some(id)));
                let end = std::array::from_fn(|i| origin[i] + direction[i] * distance);
                let event_id = self.events.event_id();
                let cause = CrimeId::Shot(event_id.0);
                self.crime(id, entity, cause, CrimeKind::Shot, origin);
                if let Some((target_entity, target)) = target
                    && hit.is_some()
                {
                    let kind = match target {
                        ShotTarget::Vehicle(car)
                            if self
                                .cars
                                .get(target_entity)
                                .is_ok_and(|(_, _, _, unit)| unit.is_some()) =>
                        {
                            Some(CrimeKind::PoliceCar(car))
                        }
                        ShotTarget::Vehicle(_) => None,
                        ShotTarget::Officer(_) => Some(CrimeKind::Officer),
                        _ => Some(CrimeKind::Harm),
                    };
                    if let Some(kind) = kind {
                        self.crime(id, entity, cause, kind, end);
                    }
                }
                for position in [origin, end] {
                    self.noises.write(NoiseEmitted {
                        position,
                        tick: self.clock.0,
                    });
                }
                self.shots.write(ShotEmitted {
                    event: Shot {
                        weapon_id: weapon,
                        shot_sequence: sequence,
                        event_id,
                        server_tick: self.clock.0,
                        shooter_id: id,
                        origin: Vec3(std::array::from_fn(|i| {
                            Scalar(origin[i] + direction[i] * distance.min(0.2))
                        })),
                        end: Vec3(end.map(Scalar)),
                        hit_entity_id: hit,
                    },
                    position: origin,
                    source: Some(peer),
                    tick: self.clock.0,
                });
                ActionCode::Ok
            }
            _ => ActionCode::Unavailable,
        }
    }
}

pub(crate) fn execute_actions(
    mut requests: MessageReader<WeaponActionRequested>,
    mut weapons: Weapons,
) {
    for message in requests.read() {
        if message.tick != weapons.clock.0 {
            continue;
        }
        let Some(session) = weapons.sessions.peers.get(&message.peer) else {
            continue;
        };
        let request = &message.request;
        let result = if let Some(cached) = session.actions.get(&request.request_id.0) {
            cached.clone()
        } else {
            let code = if session.phase != SessionPhase::Playing {
                ActionCode::NotReady
            } else if request.control_epoch.0 != session.epoch {
                ActionCode::StaleEpoch
            } else if request.request_id.0 <= session.last_request {
                ActionCode::StaleRequest
            } else if let Some(entity) = message.entity {
                weapons.player_action(message.peer, entity, &request.action, false)
            } else {
                ActionCode::NotReady
            };
            let s = weapons.sessions.peers.get_mut(&message.peer).unwrap();
            let result = ActionResult {
                request_id: request.request_id,
                applied_tick: weapons.clock.0,
                ok: code == ActionCode::Ok,
                code,
                affected_entity_id: if code == ActionCode::Ok {
                    s.player
                } else {
                    None
                },
            };
            if request.request_id.0 > s.last_request {
                s.last_request = request.request_id.0;
                s.actions.insert(request.request_id.0, result.clone());
                while s.actions.len() > 32 {
                    s.actions.pop_first();
                }
            }
            result
        };
        let sid = weapons.sessions.peers[&message.peer].id.clone();
        crate::replication::Replication::queue(
            message.peer,
            sid.as_deref(),
            Delivery::Reliable,
            ServerMessage::ActionResult(result),
            &weapons.identity.0,
            &mut weapons.sessions,
        );
    }
}
pub(crate) fn retire_actions(
    mut messages: ResMut<Messages<WeaponActionRequested>>,
    mut triggers: ResMut<Messages<WeaponTriggerRequested>>,
) {
    messages.clear();
    triggers.clear();
}

pub(crate) fn execute_automatic(
    mut requests: MessageReader<AutomaticShotRequested>,
    mut weapons: Weapons,
) {
    for request in requests.read() {
        if request.tick != weapons.clock.0
            || weapons.index.0.get(&request.player.0) != Some(&request.entity)
        {
            continue;
        }
        let Some(s) = weapons.sessions.peers.get(&request.peer) else {
            continue;
        };
        if s.epoch != request.epoch || s.player != Some(request.player) {
            continue;
        }
        if s.phase != SessionPhase::Playing {
            continue;
        }
        let code = weapons.player_action(
            request.peer,
            request.entity,
            &Action::Fire(request.aim.clone()),
            true,
        );
        if matches!(code, ActionCode::Dead | ActionCode::Unavailable)
            && let Ok((_, _, mut p)) = weapons.players.get_mut(request.entity)
        {
            p.weapon.trigger = None;
        }
    }
}

pub(crate) fn execute_officer_shots(
    mut requests: MessageReader<OfficerShotRequested>,
    mut weapons: Weapons,
) {
    for request in requests.read() {
        let shot = &request.shot;
        if request.tick != weapons.clock.0
            || weapons.index.0.get(&shot.shooter) != Some(&request.shooter)
            || weapons.index.0.get(&shot.target) != Some(&request.target)
        {
            continue;
        }
        // AI authorizes engagement; only this system commits ammunition and cooldown.
        let Ok((_, id, mut officer)) = weapons.officers.get_mut(request.shooter) else {
            continue;
        };
        if id.0 != EntityId(shot.shooter)
            || officer.mounted
            || officer.state.health.0 == 0
            || officer.weapon.reload_end.is_some()
            || officer.weapon.magazine == 0
            || weapons.clock.0 < officer.weapon.next_fire
            || officer.weapon.shot_sequence.checked_add(1) != Some(shot.sequence)
        {
            continue;
        }
        let definition = officer.weapon.definition();
        officer.weapon.shot_sequence = shot.sequence;
        officer.weapon.magazine -= 1;
        officer.weapon.next_fire = weapons.clock.0.saturating_add(
            definition.fire_ticks.max(12)
                + if shot.sequence.is_multiple_of(3) {
                    24
                } else {
                    0
                },
        );
        let Ok((_, _, player)) = weapons.players.get(request.target) else {
            continue;
        };
        if player.state.health.0 == 0
            || !matches!(
                player.state.mode,
                CharacterMode::OnFoot | CharacterMode::Driving
            )
            || player
                .lifecycle
                .protected_until
                .is_some_and(|end| weapons.clock.0 < end)
        {
            continue;
        }
        let (distance, target) = weapons.trace(
            EntityId(shot.shooter),
            shot.origin,
            shot.direction,
            shot.range,
        );
        let hit = match target {
            Some((e, t @ ShotTarget::Player(id))) if id == shot.target => {
                weapons.damage(e, t, shot.damage, None)
            }
            Some((e, t @ ShotTarget::Vehicle(_)))
                if weapons.cars.get(e).is_ok_and(|(_, _, car, _)| {
                    car.state.driver_id == Some(EntityId(shot.target))
                }) =>
            {
                weapons.damage(e, t, shot.damage, None)
            }
            Some(_) => continue,
            None => None,
        };
        let end = std::array::from_fn(|i| shot.origin[i] + shot.direction[i] * distance);
        weapons.noises.write(NoiseEmitted {
            position: shot.origin,
            tick: weapons.clock.0,
        });
        let event_id = weapons.events.event_id();
        weapons.shots.write(ShotEmitted {
            event: Shot {
                weapon_id: shot.weapon,
                shot_sequence: shot.sequence,
                event_id,
                server_tick: weapons.clock.0,
                shooter_id: EntityId(shot.shooter),
                origin: Vec3(shot.origin.map(Scalar)),
                end: Vec3(end.map(Scalar)),
                hit_entity_id: hit,
            },
            position: shot.origin,
            source: None,
            tick: weapons.clock.0,
        });
    }
}

#[derive(Message, Clone)]
pub(crate) struct WeaponTriggerRequested {
    pub peer: PeerKey,
    pub intent: Trigger,
    pub tick: u32,
    pub entity: Option<Entity>,
}
pub(crate) fn receive_triggers(
    mut messages: MessageReader<WeaponTriggerRequested>,
    mut players: Query<(&GameId, &mut Player)>,
    sessions: Res<Sessions>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        let (Some(entity), Some(session)) = (message.entity, sessions.peers.get(&message.peer))
        else {
            continue;
        };
        if session.phase != SessionPhase::Playing || message.intent.control_epoch.0 != session.epoch
        {
            continue;
        }
        let Ok((id, mut player)) = players.get_mut(entity) else {
            continue;
        };
        if session.player == Some(id.0)
            && player.state.mode == CharacterMode::OnFoot
            && player.state.health.0 > 0
        {
            player.weapon.trigger_intent(&message.intent, clock.0);
        }
    }
}
#[derive(Message, Clone)]
pub(crate) struct OfficerWeaponTick {
    pub officer: EntityId,
    pub entity: Entity,
    pub tick: u32,
    pub aiming: bool,
    pub pitch: Pitch,
}
pub(crate) fn update_officer_weapons(
    mut messages: MessageReader<OfficerWeaponTick>,
    mut officers: Query<(&GameId, &mut Officer)>,
    clock: Res<Clock>,
) {
    for message in messages.read() {
        if message.tick != clock.0 {
            continue;
        }
        let Ok((id, mut officer)) = officers.get_mut(message.entity) else {
            continue;
        };
        if id.0 != message.officer || officer.mounted || officer.state.health.0 == 0 {
            continue;
        }
        officer.weapon.advance(clock.0);
        if officer.weapon.magazine == 0 {
            officer.weapon.reload(clock.0);
        }
        officer.state.equipment = Some(EquipmentView {
            weapon_id: officer.weapon.equipped,
            aiming: message.aiming,
            aim_pitch: message.pitch,
            reload_end_tick: officer.weapon.reload_end,
            equip_end_tick: None,
        });
    }
}
