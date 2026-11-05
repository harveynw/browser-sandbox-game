use crate::transport::*;
use crate::{
    replication::Replication,
    sessions::{Session, SessionPhase, Sessions},
    systems::{missions::MissionSystem, police::PoliceSystem, population::PopulationSystem},
    world::World,
};
use protocol::*;
use sim_core::{CollisionQueries, QueryFilter};
use std::collections::BTreeMap;
#[derive(Clone, Copy)]
enum ShotTarget {
    Vehicle(u32),
    Pedestrian(u32),
    Player(u32),
    Officer(u32),
}

#[derive(Default)]
pub struct CombatSystem {
    pub homes: BTreeMap<u32, ([f32; 3], f32)>,
    pub disabled: BTreeMap<u32, (u32, u32)>,
    pub incapacitated: BTreeMap<u32, u32>,
    pairs: BTreeMap<(u32, u32, bool), u32>,
    event: u32,
}
use crate::entities::weapon::Weapon;

impl CombatSystem {
    pub(crate) fn combat_event(
        event: WorldEvent,
        position: [f32; 3],
        source: Option<(PeerKey, &Session)>,
        world: &mut World,
        sessions: &mut Sessions,
    ) {
        let recipients: Vec<_> = sessions
            .peers
            .iter()
            .filter(|(_, s)| s.phase == SessionPhase::Playing)
            .filter_map(|(&peer, s)| {
                s.player
                    .and_then(|id| world.players.get(&id.0))
                    .filter(|p| distance(p.state.motor.position.0.map(|v| v.0), position) < 192.0)
                    .map(|_| (peer, s.id.clone()))
            })
            .collect();
        for (peer, sid) in recipients {
            Replication::send(
                peer,
                sid.as_deref(),
                if matches!(event, WorldEvent::Impact(_)) {
                    Delivery::Unreliable
                } else {
                    Delivery::Reliable
                },
                ServerMessage::WorldEvent(event.clone()),
                world,
                sessions,
            );
        }
        if let Some((peer, s)) = source {
            Replication::reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::WorldEvent(event),
                world,
                sessions,
            );
        }
    }
    pub(crate) fn event_id(&mut self) -> EventId {
        self.event = self
            .event
            .checked_add(1)
            .expect("combat event IDs exhausted");
        EventId(self.event)
    }
    fn trace_shot(
        world: &World,
        population: &PopulationSystem,
        police: &PoliceSystem,
        shooter: u32,
        origin: [f32; 3],
        direction: [f32; 3],
        range: f32,
    ) -> (f32, Option<ShotTarget>) {
        // Query from the chest through the muzzle offset as part of the same
        // segment: a nearby wall cannot be skipped by the visual muzzle.
        // MOVEMENT includes car proxies. Resolve their exact hit independently
        // so static geometry wins equal-distance ties without trusting IDs.
        let static_hit = world
            .collision
            .raycast(origin, direction, range, QueryFilter::VISION)
            .ok()
            .flatten();
        let mut nearest = static_hit.map_or(range, |h| h.distance);
        // Ground is solid for shots as well.
        if direction[1] < 0.0 {
            nearest = nearest.min((-origin[1] / direction[1]).max(0.0));
        }
        let mut target = None;
        for (&car_id, car) in &world.vehicles {
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
                target = Some(ShotTarget::Vehicle(car_id));
            }
        }
        for (&ped_id, ped) in &population.pedestrians {
            if ped.state.health.0 == 0 {
                continue;
            }
            if let Some(d) = sim_core::collision::ray_character(
                origin,
                direction,
                ped.state.motor.position.0.map(|v| v.0),
                range,
            ) && d < nearest
            {
                nearest = d;
                target = Some(ShotTarget::Pedestrian(ped_id));
            }
        }
        for (&player_id, player) in &world.players {
            if player_id == shooter
                || player.state.health.0 == 0
                || player.state.mode != CharacterMode::OnFoot
            {
                continue;
            }
            if let Some(d) = sim_core::collision::ray_character(
                origin,
                direction,
                player.state.motor.position.0.map(|v| v.0),
                range,
            ) && d < nearest
            {
                nearest = d;
                target = Some(ShotTarget::Player(player_id));
            }
        }
        for (&id, officer) in &police.officers {
            if id != shooter
                && !officer.mounted
                && officer.state.health.0 > 0
                && let Some(d) = sim_core::collision::ray_character(
                    origin,
                    direction,
                    officer.state.motor.position.0.map(|v| v.0),
                    range,
                )
                && d < nearest
            {
                nearest = d;
                target = Some(ShotTarget::Officer(id));
            }
        }
        (nearest, target)
    }

    pub(crate) fn weapon_action(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        action: &Action,
        automatic_tick: bool,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
        chat: &mut crate::systems::chat::ChatSystem,
    ) -> ActionCode {
        let id = s.player.expect("playing owner");
        if world.players[&id.0].state.mode != CharacterMode::OnFoot {
            return ActionCode::Unavailable;
        }
        match action {
            Action::EquipWeapon(request) => {
                let code = world
                    .players
                    .get_mut(&id.0)
                    .unwrap()
                    .weapon
                    .equip(request.weapon_id, world.tick);
                s.status_dirty = true;
                code
            }
            Action::Reload(_) => {
                let code = world
                    .players
                    .get_mut(&id.0)
                    .unwrap()
                    .weapon
                    .reload(world.tick);
                s.status_dirty = true;
                code
            }
            Action::Fire(aim) => {
                let weapon = &world.players[&id.0].weapon;
                if weapon.definition().automatic && !automatic_tick {
                    return ActionCode::Unavailable;
                }
                if aim.weapon_id != weapon.equipped {
                    return ActionCode::Unavailable;
                }
                if weapon.equip_end.is_some() {
                    return ActionCode::Cooldown;
                }
                if weapon.shot_sequence == u32::MAX {
                    return ActionCode::Unavailable;
                }
                let definition = weapon.definition();

                if world.players[&s.player_id()].weapon.reload_end.is_some()
                    || world.tick < world.players[&s.player_id()].weapon.next_fire
                {
                    return ActionCode::Cooldown;
                }
                if world.players[&s.player_id()].weapon.magazine == 0 {
                    return ActionCode::NoAmmo;
                }
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .magazine -= 1;
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .next_fire = world.tick.saturating_add(definition.fire_ticks);
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .protected_until = None;
                s.status_dirty = true;
                police.wanted_mut(id.0).surrendering = false;
                let p = world.players[&id.0].state.motor.position.0.map(|v| v.0);
                let origin = [p[0], p[1] + 1.45, p[2]];
                let moving = world.players[&id.0].state.motor.velocity.0[0]
                    .0
                    .hypot(world.players[&id.0].state.motor.velocity.0[2].0)
                    > 0.5;
                let weapon = &mut world.players.get_mut(&id.0).unwrap().weapon;
                let spread = (definition.spread_degrees as f32
                    * if moving {
                        definition.moving_multiplier as f32
                    } else {
                        1.0
                    }
                    + weapon.bloom)
                    .to_radians();
                let seed = id.0.wrapping_mul(73856093)
                    ^ weapon.shot_sequence.wrapping_mul(19349663)
                    ^ world.tick;
                let mix = |v: u32| {
                    let v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
                    (v ^ (v >> 15)).wrapping_mul(0x846ca68b)
                };
                let u = mix(seed) as f32 / u32::MAX as f32;
                let angle =
                    (mix(seed ^ 0x12345678) as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                let radius = u.sqrt() * spread;
                weapon.bloom = (weapon.bloom + definition.bloom_per_shot as f32)
                    .min(definition.bloom_max as f32);
                let (sin, cos) = (aim.aim_yaw.0 + angle.cos() * radius).sin_cos();
                let (sp, cp) = (aim.aim_pitch.0 + angle.sin() * radius)
                    .clamp(-1.5, 1.5)
                    .sin_cos();
                let direction = [-sin * cp, sp, -cos * cp];
                let (nearest, target) = Self::trace_shot(
                    world,
                    population,
                    police,
                    id.0,
                    origin,
                    direction,
                    definition.range as f32,
                );
                let hit = match target {
                    Some(ShotTarget::Vehicle(target)) => self
                        .damage_vehicle(target, definition.damage, Some(id), world)
                        .then_some(EntityId(target)),
                    Some(ShotTarget::Pedestrian(target)) => {
                        self.damage_pedestrian(target, definition.damage as u8, world, population);
                        Some(EntityId(target))
                    }
                    Some(ShotTarget::Player(target)) => {
                        let player = world.players.get_mut(&target).unwrap();
                        if player
                            .lifecycle
                            .protected_until
                            .is_some_and(|end| world.tick < end)
                        {
                            None
                        } else {
                            player.state.health.0 =
                                player.state.health.0.saturating_sub(definition.damage);
                            if player.state.health.0 == 0 {
                                Self::down_player(
                                    target, world, sessions, population, police, missions,
                                );
                                chat.elimination(
                                    id.0,
                                    target,
                                    (peer, s.id.clone()),
                                    world,
                                    sessions,
                                );
                            }
                            Some(EntityId(target))
                        }
                    }
                    Some(ShotTarget::Officer(target)) => {
                        police.damage_officer(target, definition.damage);
                        Some(EntityId(target))
                    }
                    None => None,
                };
                let end = std::array::from_fn(|i| origin[i] + direction[i] * nearest);
                population.frighten(origin, world.tick);
                population.frighten(end, world.tick);
                let weapon = &mut world.players.get_mut(&id.0).unwrap().weapon;
                weapon.shot_sequence += 1;
                let event = WorldEvent::Shot(Shot {
                    weapon_id: weapon.equipped,
                    shot_sequence: weapon.shot_sequence,
                    event_id: self.event_id(),
                    server_tick: world.tick,
                    shooter_id: id,
                    origin: Vec3(std::array::from_fn(|i| {
                        Scalar(origin[i] + direction[i] * nearest.min(0.2))
                    })),
                    end: Vec3(end.map(Scalar)),
                    hit_entity_id: hit,
                });
                let cause = crate::systems::police::CrimeId::Shot(self.event);
                police.report_crime(
                    id.0,
                    cause,
                    crate::systems::police::CrimeKind::Shot,
                    origin,
                    world,
                );
                if let Some(target) = target
                    && hit.is_some()
                    && (!matches!(target, ShotTarget::Vehicle(car) if !police.units.contains_key(&car)))
                {
                    police.report_crime(
                        id.0,
                        cause,
                        match target {
                            ShotTarget::Vehicle(car) => {
                                crate::systems::police::CrimeKind::PoliceCar(car)
                            }
                            ShotTarget::Officer(_) => crate::systems::police::CrimeKind::Officer,
                            _ => crate::systems::police::CrimeKind::Harm,
                        },
                        end,
                        world,
                    );
                }
                CombatSystem::combat_event(event, origin, Some((peer, s)), world, sessions);
                // A shooter cannot down another guest through an occupied vehicle.
                CombatSystem::process_disabled(world, sessions, population, police, missions);
                ActionCode::Ok
            }
            _ => ActionCode::Unavailable,
        }
    }
    fn down_player(
        id: u32,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) {
        let player = world.players.get_mut(&id).expect("shot target");
        if player.state.mode != CharacterMode::OnFoot || player.lifecycle.respawn.is_some() {
            return;
        }
        player.state.mode = CharacterMode::Downed;
        player.state.motor.velocity = Vec3([Scalar(0.0); 3]);
        player.weapon.cancel();
        player.lifecycle.protected_until = None;
        player.lifecycle.respawn = Some(world.tick.saturating_add(150));
        if let Some(mission) = missions.mission_mut(id)
            && mission.phase == crate::systems::missions::Phase::Active
        {
            mission.fail(world.tick);
        }
        let peer = sessions
            .peers
            .iter()
            .find_map(|(&peer, s)| (s.player == Some(EntityId(id))).then_some(peer));
        if let Some(peer) = peer {
            let mut session = sessions.peers.remove(&peer).expect("target session");
            session.status_dirty = true;
            Replication::transition(
                peer,
                &mut session,
                &[id],
                world,
                sessions,
                population,
                police,
            )
            .expect("control epoch available");
            Replication::reliable(
                peer,
                session.id.as_deref(),
                ServerMessage::ControlChanged(
                    Replication::owner(&session, world).expect("downed owner"),
                ),
                world,
                sessions,
            );
            sessions.peers.insert(peer, session);
        }
    }

    pub(crate) fn damage_pedestrian(
        &mut self,
        id: u32,
        amount: u8,
        world: &mut World,
        population: &mut PopulationSystem,
    ) {
        if let Some(p) = population.pedestrians.get_mut(&id) {
            p.state.health.0 = p.state.health.0.saturating_sub(u16::from(amount));
            if p.state.health.0 == 0 {
                p.phase = crate::systems::population::PedestrianPhase::Incapacitated;
                p.state.mode = CharacterMode::Downed;
                p.state.motor.velocity = Vec3([Scalar(0.0); 3]);
                self.incapacitated
                    .entry(id)
                    .or_insert(world.tick.saturating_add(300));
            }
        }
    }
    pub(crate) fn damage_vehicle(
        &mut self,
        id: u32,
        amount: u16,
        attacker: Option<EntityId>,
        world: &mut World,
    ) -> bool {
        let Some(car) = world.vehicles.get_mut(&id) else {
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
            self.disabled
                .entry(id)
                .or_insert((world.tick.saturating_add(900), 0));
        }
        true
    }
    pub(crate) fn register_vehicle_homes(
        &mut self,
        world: &mut World,
        population: &mut PopulationSystem,
    ) {
        self.homes.retain(|id, _| world.vehicles.contains_key(id));
        for (&id, car) in &world.vehicles {
            let home = if population.pools.get(&id)
                == Some(&crate::systems::population::VehiclePool::Traffic)
            {
                world
                    .content
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
            self.homes.entry(id).or_insert(home);
        }
    }
    pub(crate) fn vehicle_impacts(
        &mut self,
        previous: &BTreeMap<u32, protocol::VehicleMotor>,
        impacts: Vec<(u32, f32, Vec<sim_core::Contact>)>,
        ped_previous: &BTreeMap<u32, [f32; 3]>,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) {
        self.pairs.retain(|_, until| *until > world.tick);
        for (id, delta, contacts) in impacts {
            for contact in contacts {
                let key = (id, contact.collider_id, contact.vehicle);
                if self.pairs.contains_key(&key) {
                    continue;
                }
                let damage = ((delta - 6.0) * 5.0).clamp(0.0, 50.0) as u16;
                if damage == 0 {
                    continue;
                }
                self.pairs.insert(key, world.tick.saturating_add(15));
                let own_driver = world.vehicles.get(&id).and_then(|v| v.state.driver_id);
                let attacker = if contact.vehicle {
                    world
                        .vehicles
                        .get(&contact.collider_id)
                        .and_then(|v| v.state.driver_id)
                } else {
                    None
                };
                if contact.vehicle {
                    self.pairs.insert(
                        (contact.collider_id, id, true),
                        world.tick.saturating_add(15),
                    );
                    if self.damage_vehicle(contact.collider_id, damage, own_driver, world)
                        && police.units.contains_key(&contact.collider_id)
                        && let Some(actor) = own_driver
                    {
                        let p = world.vehicles[&contact.collider_id]
                            .state
                            .motor
                            .position
                            .0
                            .map(|v| v.0);
                        police.report_crime(
                            actor.0,
                            crate::systems::police::CrimeId::Impact(world.tick, id),
                            crate::systems::police::CrimeKind::PoliceCar(contact.collider_id),
                            p,
                            world,
                        );
                    }
                }
                if self.damage_vehicle(id, damage, attacker, world) {
                    if police.units.contains_key(&id)
                        && let Some(actor) = attacker
                    {
                        let p = world.vehicles[&id].state.motor.position.0.map(|v| v.0);
                        police.report_crime(
                            actor.0,
                            crate::systems::police::CrimeId::Impact(
                                world.tick,
                                contact.collider_id,
                            ),
                            crate::systems::police::CrimeKind::PoliceCar(id),
                            p,
                            world,
                        );
                    }
                    let position = world.vehicles[&id].state.motor.position.0.map(|v| v.0);
                    let event = WorldEvent::Impact(Impact {
                        event_id: self.event_id(),
                        server_tick: world.tick,
                        entity_id: EntityId(id),
                        position: Vec3(position.map(Scalar)),
                        severity: Severity(f32::from(damage) / 50.0),
                    });
                    CombatSystem::combat_event(event, position, None, world, sessions);
                    population.frighten(position, world.tick);
                }
            }
        }
        let mut hits = Vec::new();
        for (&id, before) in previous {
            let Some(car) = world.vehicles.get(&id) else {
                continue;
            };
            let speed = before.speed.0.abs().max(car.state.motor.speed.0.abs());
            if speed <= 4.0 || car.state.disabled {
                continue;
            }
            for (&ped, p) in &population.pedestrians {
                if p.state.health.0 == 0 || self.pairs.contains_key(&(id, ped, true)) {
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
            self.pairs
                .insert((car, ped, true), world.tick.saturating_add(15));
            self.damage_pedestrian(ped, damage, world, population);
            if let Some(actor) = world.vehicles[&car].state.driver_id {
                let p = population.pedestrians[&ped]
                    .state
                    .motor
                    .position
                    .0
                    .map(|v| v.0);
                police.report_crime(
                    actor.0,
                    crate::systems::police::CrimeId::Impact(world.tick, car),
                    crate::systems::police::CrimeKind::Harm,
                    p,
                    world,
                );
            }
            let position = population.pedestrians[&ped]
                .state
                .motor
                .position
                .0
                .map(|v| v.0);
            population.frighten(position, world.tick);
            let event = WorldEvent::Impact(Impact {
                event_id: self.event_id(),
                server_tick: world.tick,
                entity_id: EntityId(ped),
                position: Vec3(position.map(Scalar)),
                severity: Severity(f32::from(damage) / 100.0),
            });
            CombatSystem::combat_event(event, position, None, world, sessions);
        }
        CombatSystem::process_disabled(world, sessions, population, police, missions);
    }
    pub(crate) fn clear_spawn(
        position: [f32; 3],
        except: Option<u32>,
        world: &World,
        population: &PopulationSystem,
    ) -> bool {
        !world
            .collision
            .overlaps_character(position, QueryFilter::MOVEMENT)
            .unwrap_or(true)
            && world.players.iter().all(|(&id, p)| {
                Some(id) == except
                    || p.state.mode == CharacterMode::Downed
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
            && population.pedestrians.values().all(|p| {
                p.state.health.0 == 0
                    || distance(position, p.state.motor.position.0.map(|v| v.0)) > 1.0
            })
    }
    pub(crate) fn safe_spawn(
        near: [f32; 3],
        except: Option<u32>,
        world: &World,
        population: &PopulationSystem,
    ) -> Option<[f32; 3]> {
        let mut candidates = Vec::new();
        for spawn in &world.content.manifest.spawns {
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
            CombatSystem::clear_spawn(*p, except, world, population)
                && world
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
    pub(crate) fn process_disabled(
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) {
        let peers: Vec<_> = sessions
            .peers
            .iter()
            .filter_map(|(&peer, s)| {
                s.player
                    .and_then(|id| world.players.get(&id.0))
                    .filter(|p| {
                        p.state
                            .vehicle_id
                            .is_some_and(|id| world.vehicles[&id.0].state.health.0 == 0)
                    })
                    .map(|_| peer)
            })
            .collect();
        for peer in peers {
            let mut s = sessions.peers.remove(&peer).expect("driver session");
            let id = s.player.expect("driver");
            let car_id = world.players[&id.0]
                .state
                .vehicle_id
                .expect("disabled car")
                .0;
            world.collision.exclude_vehicle(Some(car_id));
            let exit = crate::entities::vehicle::safe_exit_with(
                &world.vehicles[&car_id],
                &world.collision,
                |p| CombatSystem::clear_spawn(p, Some(id.0), world, population),
            );
            world.collision.exclude_vehicle(None);
            let exit = exit.or_else(|| {
                CombatSystem::safe_spawn(
                    world.vehicles[&car_id].state.motor.position.0.map(|v| v.0),
                    Some(id.0),
                    world,
                    population,
                )
            });
            // Keep the wire state coherent while a safe exit is unavailable: a
            // zero-health seated car is frozen, then becomes disabled on ejection.
            if world.players[&id.0].state.health.0 > 0 {
                world.players.get_mut(&id.0).expect("driver").state.health = CharacterHealth(0);
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .cancel();
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .respawn = Some(world.tick.saturating_add(150));
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .protected_until = None;
                s.status_dirty = true;
            }
            if let Some(exit) = exit {
                world
                    .vehicles
                    .get_mut(&car_id)
                    .expect("disabled car")
                    .state
                    .driver_id = None;
                world
                    .vehicles
                    .get_mut(&car_id)
                    .expect("disabled car")
                    .state
                    .disabled = true;
                let player = world.players.get_mut(&id.0).expect("driver");
                player.state.health = CharacterHealth(0);
                player.state.mode = CharacterMode::Downed;
                player.state.vehicle_id = None;
                player.state.motor = sim_core::spawn_character(exit, 0.0).expect("safe exit");
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .cancel();
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .respawn
                    .get_or_insert(world.tick.saturating_add(150));
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .lifecycle
                    .protected_until = None;
                s.status_dirty = true;
                if let Some(m) = missions.mission_mut(s.player_id())
                    && m.phase == crate::systems::missions::Phase::Active
                {
                    m.fail(world.tick);
                }
                Replication::transition(
                    peer,
                    &mut s,
                    &[id.0, car_id],
                    world,
                    sessions,
                    population,
                    police,
                )
                .expect("control epoch available");
                Replication::reliable(
                    peer,
                    s.id.as_deref(),
                    ServerMessage::ControlChanged(
                        Replication::owner(&s, world).expect("downed owner"),
                    ),
                    world,
                    sessions,
                );
            }
            sessions.peers.insert(peer, s);
        }
    }
    fn officer_fire(
        &mut self,
        shot: crate::systems::police::OfficerShot,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
        chat: &mut crate::systems::chat::ChatSystem,
    ) {
        if world.players[&shot.target]
            .lifecycle
            .protected_until
            .is_some_and(|end| world.tick < end)
        {
            return;
        }
        let (distance, target) = Self::trace_shot(
            world,
            population,
            police,
            shot.shooter,
            shot.origin,
            shot.direction,
            shot.range,
        );
        let hit = match target {
            Some(ShotTarget::Player(id)) if id == shot.target => {
                let player = world.players.get_mut(&id).expect("traced player");
                if player
                    .lifecycle
                    .protected_until
                    .is_some_and(|end| world.tick < end)
                {
                    return;
                }
                player.state.health.0 = player.state.health.0.saturating_sub(shot.damage);
                if player.state.health.0 == 0 {
                    Self::down_player(id, world, sessions, population, police, missions);
                    chat.police_elimination(id, world, sessions);
                }
                Some(EntityId(id))
            }
            Some(ShotTarget::Vehicle(id))
                if world.vehicles[&id].state.driver_id == Some(EntityId(shot.target)) =>
            {
                self.damage_vehicle(id, shot.damage, None, world)
                    .then_some(EntityId(id))
            }
            Some(_) => return,
            None => None,
        };
        let end = std::array::from_fn(|i| shot.origin[i] + shot.direction[i] * distance);
        population.frighten(shot.origin, world.tick);
        Self::combat_event(
            WorldEvent::Shot(Shot {
                weapon_id: shot.weapon,
                shot_sequence: shot.sequence,
                event_id: self.event_id(),
                server_tick: world.tick,
                shooter_id: EntityId(shot.shooter),
                origin: Vec3(shot.origin.map(Scalar)),
                end: Vec3(end.map(Scalar)),
                hit_entity_id: hit,
            }),
            shot.origin,
            None,
            world,
            sessions,
        );
    }

    pub(crate) fn update(
        &mut self,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
        chat: &mut crate::systems::chat::ChatSystem,
    ) {
        for shot in police.step_officers(world, sessions, population) {
            if world.players.get(&shot.target).is_some_and(|player| {
                player.state.health.0 > 0
                    && matches!(
                        player.state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    )
            }) {
                self.officer_fire(shot, world, sessions, population, police, missions, chat);
            }
        }
        CombatSystem::process_disabled(world, sessions, population, police, missions);
        for id in self
            .incapacitated
            .iter()
            .filter(|(_, until)| **until <= world.tick)
            .map(|(&id, _)| id)
            .collect::<Vec<_>>()
        {
            population.pedestrians.remove(&id);
            self.incapacitated.remove(&id);
        }
        let peers: Vec<_> = sessions.peers.keys().copied().collect();
        for peer in peers {
            let mut s = sessions.peers.remove(&peer).expect("session");
            if let Some(id) = s.player {
                let eligible = world.players[&id.0].state.mode == CharacterMode::OnFoot
                    && world.players[&id.0].state.health.0 > 0
                    && !police.wanted(id.0).arrest_pending
                    && s.phase == crate::sessions::SessionPhase::Playing;
                if !eligible {
                    world.players.get_mut(&id.0).unwrap().weapon.trigger = None;
                }
                let aim = world
                    .players
                    .get_mut(&id.0)
                    .unwrap()
                    .weapon
                    .automatic_aim(world.tick, s.epoch);
                if let Some(aim) = aim {
                    self.weapon_action(
                        peer,
                        &mut s,
                        &Action::Fire(aim),
                        true,
                        world,
                        sessions,
                        population,
                        police,
                        missions,
                        chat,
                    );
                }

                if world
                    .players
                    .get_mut(&id.0)
                    .unwrap()
                    .weapon
                    .advance(world.tick)
                {
                    s.status_dirty = true;
                }
                if world.players[&s.player_id()]
                    .lifecycle
                    .protected_until
                    .is_some_and(|end| world.tick >= end)
                {
                    world
                        .players
                        .get_mut(&s.player_id())
                        .expect("player equipment")
                        .lifecycle
                        .protected_until = None;
                    s.status_dirty = true;
                }
                if world.players[&s.player_id()]
                    .lifecycle
                    .respawn
                    .is_some_and(|end| world.tick >= end)
                {
                    let near = world.players[&id.0].state.motor.position.0.map(|v| v.0);
                    if let Some(position) =
                        CombatSystem::safe_spawn(near, Some(id.0), world, population)
                    {
                        let sequence = world.players[&id.0].weapon.shot_sequence;
                        let player = world.players.get_mut(&id.0).expect("respawn player");
                        player.state.motor =
                            sim_core::spawn_character(position, 0.0).expect("safe spawn");
                        player.state.health = CharacterHealth(100);
                        player.state.mode = CharacterMode::OnFoot;
                        player.state.vehicle_id = None;
                        *missions.mission_mut(s.player_id()) = None;
                        *police.wanted_mut(s.player_id()) = Default::default();
                        police.release(id.0);
                        world
                            .players
                            .get_mut(&s.player_id())
                            .expect("player equipment")
                            .weapon = Weapon::default();
                        world.players.get_mut(&id.0).unwrap().weapon.shot_sequence = sequence;
                        world
                            .players
                            .get_mut(&id.0)
                            .unwrap()
                            .lifecycle
                            .protected_until = Some(world.tick.saturating_add(90));
                        world.players.get_mut(&id.0).unwrap().lifecycle.respawn = None;
                        s.status_dirty = true;
                        Replication::transition(
                            peer,
                            &mut s,
                            &[id.0],
                            world,
                            sessions,
                            population,
                            police,
                        )
                        .expect("control epoch available");
                        Replication::reliable(
                            peer,
                            s.id.as_deref(),
                            ServerMessage::ControlChanged(
                                Replication::owner(&s, world).expect("respawn owner"),
                            ),
                            world,
                            sessions,
                        );
                    }
                }
            }
            sessions.peers.insert(peer, s);
        }
        let disabled: Vec<_> = self
            .disabled
            .iter()
            .filter(|(_, (tick, _))| *tick <= world.tick)
            .map(|(&id, &state)| (id, state))
            .collect();
        for (id, (due, retries)) in disabled {
            if police.units.contains_key(&id) {
                continue;
            }
            let Some(car) = world.vehicles.get(&id) else {
                self.disabled.remove(&id);
                continue;
            };
            if car.state.driver_id.is_some() {
                continue;
            }
            let template = car.template.clone();
            let home = self.homes[&id];
            let mut candidates = vec![home];
            if retries >= 30 {
                candidates.extend(
                    world
                        .content
                        .manifest
                        .parking
                        .iter()
                        .filter(|p| !p.mission_reserved && p.template_id == template.id)
                        .map(|p| (p.position.map(|v| v as f32), p.yaw as f32)),
                );
            }
            world.collision.exclude_vehicle(Some(id));
            let spot = candidates.into_iter().find(|(p, yaw)| {
                !world
                    .collision
                    .overlaps_vehicle(*p, *yaw, template.dimensions.map(|v| v as f32))
                    .unwrap_or(true)
                    && world
                        .players
                        .values()
                        .all(|a| distance(*p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                    && population
                        .pedestrians
                        .values()
                        .all(|a| distance(*p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
            });
            world.collision.exclude_vehicle(None);
            if let Some((position, yaw)) = spot {
                let car = world.vehicles.get_mut(&id).expect("respawn car");
                car.state.motor = sim_core::spawn_vehicle(position, yaw).expect("flat parking");
                car.state.health = VehicleHealth(template.health);
                car.state.disabled = false;
                car.idle_ticks = 30;
                population.release_ai(id);
                self.disabled.remove(&id);
            } else {
                self.disabled
                    .insert(id, (due.saturating_add(30), retries.saturating_add(1)));
            }
        }
        self.incapacitated
            .retain(|id, _| population.pedestrians.contains_key(id));
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).hypot(a[2] - b[2])
}
