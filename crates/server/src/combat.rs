//! Authoritative pistol, damage, and safe lifecycle transitions. No client hit claims.
use crate::{
    transport::PeerKey,
    world::{Session, SessionPhase, World},
};
use protocol::*;
use sim_core::{CollisionQueries, QueryFilter};
use std::collections::BTreeMap;
#[derive(Clone)]
pub struct Weapon {
    pub magazine: u8,
    pub next_fire: u32,
    pub reload_end: Option<u32>,
    pub respawn: Option<u32>,
    pub protected_until: Option<u32>,
}
impl Default for Weapon {
    fn default() -> Self {
        Self {
            magazine: 12,
            next_fire: 0,
            reload_end: None,
            respawn: None,
            protected_until: None,
        }
    }
}
#[derive(Default)]
pub struct Combat {
    pub homes: BTreeMap<u32, ([f32; 3], f32)>,
    pub disabled: BTreeMap<u32, (u32, u32)>,
    pub incapacitated: BTreeMap<u32, u32>,
    pairs: BTreeMap<(u32, u32, bool), u32>,
    event: u32,
}
impl World {
    fn combat_event(
        &mut self,
        event: WorldEvent,
        position: [f32; 3],
        source: Option<(PeerKey, &Session)>,
    ) {
        let recipients: Vec<_> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .filter_map(|(&peer, s)| {
                s.player
                    .and_then(|id| self.players.get(&id.0))
                    .filter(|p| distance(p.state.motor.position.0.map(|v| v.0), position) < 192.0)
                    .map(|_| (peer, s.id.clone()))
            })
            .collect();
        for (peer, sid) in recipients {
            self.send(
                peer,
                sid.as_deref(),
                if matches!(event, WorldEvent::Impact(_)) {
                    Delivery::Unreliable
                } else {
                    Delivery::Reliable
                },
                ServerMessage::WorldEvent(event.clone()),
            );
        }
        if let Some((peer, s)) = source {
            self.reliable(peer, s.id.as_deref(), ServerMessage::WorldEvent(event));
        }
    }
    fn event_id(&mut self) -> EventId {
        self.combat.event = self
            .combat
            .event
            .checked_add(1)
            .expect("combat event IDs exhausted");
        EventId(self.combat.event)
    }
    pub(crate) fn weapon_action(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        action: &Action,
    ) -> ActionCode {
        let id = s.player.expect("playing owner");
        if self.players[&id.0].state.mode != CharacterMode::OnFoot {
            return ActionCode::Unavailable;
        }
        match action {
            Action::Reload(_) => {
                if s.weapon.reload_end.is_some() || s.weapon.magazine == 12 {
                    return ActionCode::Cooldown;
                }
                s.weapon.reload_end = Some(self.tick.saturating_add(45));
                s.status_dirty = true;
                ActionCode::Ok
            }
            Action::Fire(aim) => {
                if s.weapon.reload_end.is_some() || self.tick < s.weapon.next_fire {
                    return ActionCode::Cooldown;
                }
                if s.weapon.magazine == 0 {
                    return ActionCode::NoAmmo;
                }
                s.weapon.magazine -= 1;
                s.weapon.next_fire = self.tick.saturating_add(9);
                s.weapon.protected_until = None;
                s.status_dirty = true;
                let p = self.players[&id.0].state.motor.position.0.map(|v| v.0);
                let origin = [p[0], p[1] + 1.45, p[2]];
                let (sin, cos) = aim.aim_yaw.0.sin_cos();
                let (sp, cp) = aim.aim_pitch.0.sin_cos();
                let direction = [-sin * cp, sp, -cos * cp];
                // Query from the chest through the muzzle offset as part of the same
                // segment: a nearby wall cannot be skipped by the visual muzzle.
                // MOVEMENT includes car proxies. Resolve their exact hit independently
                // so static geometry wins equal-distance ties without trusting IDs.
                let static_hit = self
                    .collision
                    .raycast(origin, direction, 80.0, QueryFilter::VISION)
                    .ok()
                    .flatten();
                let mut nearest = static_hit.map_or(80.0, |h| h.distance);
                // Ground is solid for shots as well.
                if direction[1] < 0.0 {
                    nearest = nearest.min((-origin[1] / direction[1]).max(0.0));
                }
                let mut target = None;
                for (&car_id, car) in &self.vehicles {
                    if let Some(d) = sim_core::collision::ray_vehicle(
                        origin,
                        direction,
                        car.state.motor.position.0.map(|v| v.0),
                        car.state.motor.yaw.0,
                        car.template.dimensions.map(|v| v as f32),
                        80.0,
                    ) && d < nearest
                    {
                        nearest = d;
                        target = Some((car_id, true));
                    }
                }
                for (&ped_id, ped) in &self.population.pedestrians {
                    if ped.state.health.0 == 0 {
                        continue;
                    }
                    if let Some(d) = sim_core::collision::ray_character(
                        origin,
                        direction,
                        ped.state.motor.position.0.map(|v| v.0),
                        80.0,
                    ) && d < nearest
                    {
                        nearest = d;
                        target = Some((ped_id, false));
                    }
                }
                let hit = match target {
                    Some((target, true)) => self
                        .damage_vehicle(target, 25, Some(id))
                        .then_some(EntityId(target)),
                    Some((target, false)) => {
                        self.damage_pedestrian(target, 25);
                        Some(EntityId(target))
                    }
                    None => None,
                };
                let end = std::array::from_fn(|i| origin[i] + direction[i] * nearest);
                self.population.frighten(origin, self.tick);
                self.population.frighten(end, self.tick);
                let event = WorldEvent::Shot(Shot {
                    event_id: self.event_id(),
                    server_tick: self.tick,
                    shooter_id: id,
                    origin: Vec3(std::array::from_fn(|i| {
                        Scalar(origin[i] + direction[i] * nearest.min(0.2))
                    })),
                    end: Vec3(end.map(Scalar)),
                    hit_entity_id: hit,
                });
                self.combat_event(event, origin, Some((peer, s)));
                // A shooter cannot down another guest through an occupied vehicle.
                self.process_disabled();
                ActionCode::Ok
            }
            _ => ActionCode::Unavailable,
        }
    }
    fn damage_pedestrian(&mut self, id: u32, amount: u8) {
        if let Some(p) = self.population.pedestrians.get_mut(&id) {
            p.state.health.0 = p.state.health.0.saturating_sub(u16::from(amount));
            if p.state.health.0 == 0 {
                p.phase = crate::population::PedestrianPhase::Incapacitated;
                p.state.mode = CharacterMode::Downed;
                p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
                self.combat
                    .incapacitated
                    .entry(id)
                    .or_insert(self.tick.saturating_add(300));
            }
        }
    }
    fn damage_vehicle(&mut self, id: u32, amount: u16, attacker: Option<EntityId>) -> bool {
        let Some(car) = self.vehicles.get_mut(&id) else {
            return false;
        };
        if car.state.disabled
            || car.state.health.0 == 0
            || attacker.is_some_and(|a| car.state.driver_id.is_some_and(|driver| driver != a))
        {
            return false;
        }
        car.state.health.0 = car.state.health.0.saturating_sub(amount);
        car.idle_ticks = 0;
        if car.state.health.0 == 0 {
            car.state.disabled = car.state.driver_id.is_none();
            car.stop();
            self.combat
                .disabled
                .entry(id)
                .or_insert((self.tick.saturating_add(900), 0));
        }
        true
    }
    pub(crate) fn register_vehicle_homes(&mut self) {
        self.combat
            .homes
            .retain(|id, _| self.vehicles.contains_key(id));
        for (&id, car) in &self.vehicles {
            let home = if self.population.pools.get(&id)
                == Some(&crate::population::VehiclePool::Traffic)
            {
                self.content
                    .manifest
                    .parking
                    .iter()
                    .filter(|p| !p.mission_reserved && p.template_id == car.template.id)
                    .min_by(|a, b| {
                        distance(
                            a.position.map(|v| v as f32),
                            car.state.motor.position.0.map(|v| v.0),
                        )
                        .total_cmp(&distance(
                            b.position.map(|v| v as f32),
                            car.state.motor.position.0.map(|v| v.0),
                        ))
                    })
                    .map(|p| (p.position.map(|v| v as f32), p.yaw as f32))
                    .unwrap_or((
                        car.state.motor.position.0.map(|v| v.0),
                        car.state.motor.yaw.0,
                    ))
            } else {
                (
                    car.state.motor.position.0.map(|v| v.0),
                    car.state.motor.yaw.0,
                )
            };
            self.combat.homes.entry(id).or_insert(home);
        }
    }
    pub(crate) fn vehicle_impacts(
        &mut self,
        previous: &BTreeMap<u32, protocol::VehicleMotor>,
        impacts: Vec<(u32, f32, Vec<sim_core::Contact>)>,
        ped_previous: &BTreeMap<u32, [f32; 3]>,
    ) {
        self.combat.pairs.retain(|_, until| *until > self.tick);
        for (id, delta, contacts) in impacts {
            for contact in contacts {
                let key = (id, contact.collider_id, contact.vehicle);
                if self.combat.pairs.contains_key(&key) {
                    continue;
                }
                let damage = ((delta - 6.0) * 5.0).clamp(0.0, 50.0) as u16;
                if damage == 0 {
                    continue;
                }
                self.combat.pairs.insert(key, self.tick.saturating_add(15));
                let own_driver = self.vehicles.get(&id).and_then(|v| v.state.driver_id);
                let attacker = if contact.vehicle {
                    self.vehicles
                        .get(&contact.collider_id)
                        .and_then(|v| v.state.driver_id)
                } else {
                    None
                };
                if contact.vehicle {
                    self.combat.pairs.insert(
                        (contact.collider_id, id, true),
                        self.tick.saturating_add(15),
                    );
                    self.damage_vehicle(contact.collider_id, damage, own_driver);
                }
                if self.damage_vehicle(id, damage, attacker) {
                    let position = self.vehicles[&id].state.motor.position.0.map(|v| v.0);
                    let event = WorldEvent::Impact(Impact {
                        event_id: self.event_id(),
                        server_tick: self.tick,
                        entity_id: EntityId(id),
                        position: Vec3(position.map(Scalar)),
                        severity: Severity(f32::from(damage) / 50.0),
                    });
                    self.combat_event(event, position, None);
                    self.population.frighten(position, self.tick);
                }
            }
        }
        let mut hits = Vec::new();
        for (&id, before) in previous {
            let Some(car) = self.vehicles.get(&id) else {
                continue;
            };
            let speed = before.speed.0.abs().max(car.state.motor.speed.0.abs());
            if speed <= 4.0 || car.state.disabled {
                continue;
            }
            for (&ped, p) in &self.population.pedestrians {
                if p.state.health.0 == 0 || self.combat.pairs.contains_key(&(id, ped, true)) {
                    continue;
                }
                let from = ped_previous
                    .get(&ped)
                    .copied()
                    .unwrap_or_else(|| p.state.motor.position.0.map(|v| v.0));
                if sim_core::collision::vehicle_hits_character(
                    before.position.0.map(|v| v.0),
                    car.state.motor.position.0.map(|v| v.0),
                    [before.yaw.0, car.state.motor.yaw.0],
                    car.template.dimensions.map(|v| v as f32),
                    from,
                    p.state.motor.position.0.map(|v| v.0),
                ) {
                    hits.push((id, ped, (speed * 8.0).min(100.0) as u8));
                }
            }
        }
        for (car, ped, damage) in hits {
            self.combat
                .pairs
                .insert((car, ped, true), self.tick.saturating_add(15));
            self.damage_pedestrian(ped, damage);
            let position = self.population.pedestrians[&ped]
                .state
                .motor
                .position
                .0
                .map(|v| v.0);
            self.population.frighten(position, self.tick);
            let event = WorldEvent::Impact(Impact {
                event_id: self.event_id(),
                server_tick: self.tick,
                entity_id: EntityId(ped),
                position: Vec3(position.map(Scalar)),
                severity: Severity(f32::from(damage) / 100.0),
            });
            self.combat_event(event, position, None);
        }
        self.process_disabled();
    }
    fn clear_spawn(&self, position: [f32; 3], except: Option<u32>) -> bool {
        !self
            .collision
            .overlaps_character(position, QueryFilter::MOVEMENT)
            .unwrap_or(true)
            && self.players.iter().all(|(&id, p)| {
                Some(id) == except
                    || p.state.mode == CharacterMode::Downed
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
            && self.population.pedestrians.values().all(|p| {
                p.state.health.0 == 0
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
    }
    fn safe_spawn(&self, near: [f32; 3], except: Option<u32>) -> Option<[f32; 3]> {
        let mut candidates = Vec::new();
        for spawn in &self.content.manifest.spawns {
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
            self.clear_spawn(*p, except)
                && self
                    .collision
                    .raycast(
                        [p[0], p[1] + 0.5, p[2]],
                        [0.0, -1.0, 0.0],
                        1.0,
                        QueryFilter::MOVEMENT,
                    )
                    .is_ok_and(|h| h.is_some_and(|h| h.normal[1] > 0.7))
        })
    }
    fn process_disabled(&mut self) {
        let peers: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(&peer, s)| {
                s.player
                    .and_then(|id| self.players.get(&id.0))
                    .filter(|p| {
                        p.state
                            .vehicle_id
                            .is_some_and(|id| self.vehicles[&id.0].state.health.0 == 0)
                    })
                    .map(|_| peer)
            })
            .collect();
        for peer in peers {
            let mut s = self.sessions.remove(&peer).expect("driver session");
            let id = s.player.expect("driver");
            let car_id = self.players[&id.0]
                .state
                .vehicle_id
                .expect("disabled car")
                .0;
            self.collision.exclude_vehicle(Some(car_id));
            let exit =
                crate::vehicles::safe_exit_with(&self.vehicles[&car_id], &self.collision, |p| {
                    self.clear_spawn(p, Some(id.0))
                });
            self.collision.exclude_vehicle(None);
            let exit = exit.or_else(|| {
                self.safe_spawn(
                    self.vehicles[&car_id].state.motor.position.0.map(|v| v.0),
                    Some(id.0),
                )
            });
            // Keep the wire state coherent while a safe exit is unavailable: a
            // zero-health seated car is frozen, then becomes disabled on ejection.
            if self.players[&id.0].state.health.0 > 0 {
                self.players.get_mut(&id.0).expect("driver").state.health = CharacterHealth(0);
                s.weapon.reload_end = None;
                s.weapon.respawn = Some(self.tick.saturating_add(150));
                s.weapon.protected_until = None;
                s.status_dirty = true;
            }
            if let Some(exit) = exit {
                self.vehicles
                    .get_mut(&car_id)
                    .expect("disabled car")
                    .state
                    .driver_id = None;
                self.vehicles
                    .get_mut(&car_id)
                    .expect("disabled car")
                    .state
                    .disabled = true;
                let player = self.players.get_mut(&id.0).expect("driver");
                player.state.health = CharacterHealth(0);
                player.state.mode = CharacterMode::Downed;
                player.state.vehicle_id = None;
                player.state.motor = sim_core::spawn_character(exit, 0.0).expect("safe exit");
                s.weapon.reload_end = None;
                s.weapon
                    .respawn
                    .get_or_insert(self.tick.saturating_add(150));
                s.weapon.protected_until = None;
                s.status_dirty = true;
                if let Some(m) = &mut s.mission
                    && m.phase == crate::missions::Phase::Active
                {
                    m.fail(self.tick);
                }
                self.transition(peer, &mut s, &[id.0, car_id])
                    .expect("control epoch available");
                self.reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::ControlChanged(self.owner(&s).expect("downed owner")),
                );
            }
            self.sessions.insert(peer, s);
        }
    }
    pub(crate) fn advance_combat(&mut self) {
        self.process_disabled();
        for id in self
            .combat
            .incapacitated
            .iter()
            .filter(|(_, until)| **until <= self.tick)
            .map(|(&id, _)| id)
            .collect::<Vec<_>>()
        {
            self.population.pedestrians.remove(&id);
            self.combat.incapacitated.remove(&id);
        }
        let peers: Vec<_> = self.sessions.keys().copied().collect();
        for peer in peers {
            let mut s = self.sessions.remove(&peer).expect("session");
            if let Some(id) = s.player {
                if s.weapon.reload_end.is_some_and(|end| self.tick >= end) {
                    s.weapon.reload_end = None;
                    s.weapon.magazine = 12;
                    s.status_dirty = true;
                }
                if s.weapon.protected_until.is_some_and(|end| self.tick >= end) {
                    s.weapon.protected_until = None;
                    s.status_dirty = true;
                }
                if s.weapon.respawn.is_some_and(|end| self.tick >= end) {
                    let near = self.players[&id.0].state.motor.position.0.map(|v| v.0);
                    if let Some(position) = self.safe_spawn(near, Some(id.0)) {
                        let player = self.players.get_mut(&id.0).expect("respawn player");
                        player.state.motor =
                            sim_core::spawn_character(position, 0.0).expect("safe spawn");
                        player.state.health = CharacterHealth(100);
                        player.state.mode = CharacterMode::OnFoot;
                        player.state.vehicle_id = None;
                        s.mission = None;
                        s.weapon = Weapon {
                            protected_until: Some(self.tick.saturating_add(90)),
                            ..Default::default()
                        };
                        s.status_dirty = true;
                        self.transition(peer, &mut s, &[id.0])
                            .expect("control epoch available");
                        self.reliable(
                            peer,
                            s.id.as_deref(),
                            ServerMessage::ControlChanged(self.owner(&s).expect("respawn owner")),
                        );
                    }
                }
            }
            self.sessions.insert(peer, s);
        }
        let disabled: Vec<_> = self
            .combat
            .disabled
            .iter()
            .filter(|(_, (tick, _))| *tick <= self.tick)
            .map(|(&id, &state)| (id, state))
            .collect();
        for (id, (due, retries)) in disabled {
            let Some(car) = self.vehicles.get(&id) else {
                self.combat.disabled.remove(&id);
                continue;
            };
            if car.state.driver_id.is_some() {
                continue;
            }
            let template = car.template.clone();
            let home = self.combat.homes[&id];
            let mut candidates = vec![home];
            if retries >= 30 {
                candidates.extend(
                    self.content
                        .manifest
                        .parking
                        .iter()
                        .filter(|p| !p.mission_reserved && p.template_id == template.id)
                        .map(|p| (p.position.map(|v| v as f32), p.yaw as f32)),
                );
            }
            self.collision.exclude_vehicle(Some(id));
            let spot = candidates.into_iter().find(|(p, yaw)| {
                !self
                    .collision
                    .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                    .unwrap_or(true)
                    && self
                        .players
                        .values()
                        .all(|a| distance(*p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                    && self
                        .population
                        .pedestrians
                        .values()
                        .all(|a| distance(*p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
            });
            self.collision.exclude_vehicle(None);
            if let Some((position, yaw)) = spot {
                let car = self.vehicles.get_mut(&id).expect("respawn car");
                car.state.motor = sim_core::spawn_vehicle(position, yaw).expect("flat parking");
                car.state.health = VehicleHealth(template.health);
                car.state.disabled = false;
                car.idle_ticks = 30;
                self.population.release_ai(id);
                self.combat.disabled.remove(&id);
            } else {
                self.combat
                    .disabled
                    .insert(id, (due.saturating_add(30), retries.saturating_add(1)));
            }
        }
        self.combat
            .incapacitated
            .retain(|id, _| self.population.pedestrians.contains_key(id));
    }
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
