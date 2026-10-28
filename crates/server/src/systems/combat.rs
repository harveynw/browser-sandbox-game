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
    pub(crate) fn weapon_action(
        &mut self,
        peer: PeerKey,
        s: &mut Session,
        action: &Action,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) -> ActionCode {
        let id = s.player.expect("playing owner");
        if world.players[&id.0].state.mode != CharacterMode::OnFoot {
            return ActionCode::Unavailable;
        }
        match action {
            Action::Reload(_) => {
                if world.players[&s.player_id()].weapon.reload_end.is_some()
                    || world.players[&s.player_id()].weapon.magazine == 12
                {
                    return ActionCode::Cooldown;
                }
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .reload_end = Some(world.tick.saturating_add(45));
                s.status_dirty = true;
                ActionCode::Ok
            }
            Action::Fire(aim) => {
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
                    .next_fire = world.tick.saturating_add(9);
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .protected_until = None;
                s.status_dirty = true;
                let p = world.players[&id.0].state.motor.position.0.map(|v| v.0);
                let origin = [p[0], p[1] + 1.45, p[2]];
                let (sin, cos) = aim.aim_yaw.0.sin_cos();
                let (sp, cp) = aim.aim_pitch.0.sin_cos();
                let direction = [-sin * cp, sp, -cos * cp];
                // Query from the chest through the muzzle offset as part of the same
                // segment: a nearby wall cannot be skipped by the visual muzzle.
                // MOVEMENT includes car proxies. Resolve their exact hit independently
                // so static geometry wins equal-distance ties without trusting IDs.
                let static_hit = world
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
                for (&car_id, car) in &world.vehicles {
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
                for (&ped_id, ped) in &population.pedestrians {
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
                        .damage_vehicle(target, 25, Some(id), world)
                        .then_some(EntityId(target)),
                    Some((target, false)) => {
                        self.damage_pedestrian(target, 25, world, population);
                        Some(EntityId(target))
                    }
                    None => None,
                };
                let end = std::array::from_fn(|i| origin[i] + direction[i] * nearest);
                population.frighten(origin, world.tick);
                population.frighten(end, world.tick);
                let event = WorldEvent::Shot(Shot {
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
                if let Some((target, is_car)) = target
                    && hit.is_some()
                    && (!is_car || police.units.contains_key(&target))
                {
                    police.report_crime(
                        id.0,
                        cause,
                        if is_car {
                            crate::systems::police::CrimeKind::PoliceCar(target)
                        } else {
                            crate::systems::police::CrimeKind::Harm
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
                    .reload_end = None;
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .respawn = Some(world.tick.saturating_add(150));
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
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
                    .reload_end = None;
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
                    .respawn
                    .get_or_insert(world.tick.saturating_add(150));
                world
                    .players
                    .get_mut(&s.player_id())
                    .expect("player equipment")
                    .weapon
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
    pub(crate) fn update(
        &mut self,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) {
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
                if world.players[&s.player_id()]
                    .weapon
                    .reload_end
                    .is_some_and(|end| world.tick >= end)
                {
                    world
                        .players
                        .get_mut(&s.player_id())
                        .expect("player equipment")
                        .weapon
                        .reload_end = None;
                    world
                        .players
                        .get_mut(&s.player_id())
                        .expect("player equipment")
                        .weapon
                        .magazine = 12;
                    s.status_dirty = true;
                }
                if world.players[&s.player_id()]
                    .weapon
                    .protected_until
                    .is_some_and(|end| world.tick >= end)
                {
                    world
                        .players
                        .get_mut(&s.player_id())
                        .expect("player equipment")
                        .weapon
                        .protected_until = None;
                    s.status_dirty = true;
                }
                if world.players[&s.player_id()]
                    .weapon
                    .respawn
                    .is_some_and(|end| world.tick >= end)
                {
                    let near = world.players[&id.0].state.motor.position.0.map(|v| v.0);
                    if let Some(position) =
                        CombatSystem::safe_spawn(near, Some(id.0), world, population)
                    {
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
                            .weapon = Weapon {
                            protected_until: Some(world.tick.saturating_add(90)),
                            ..Default::default()
                        };
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
