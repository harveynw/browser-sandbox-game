use crate::admission::{Profiles, random_id};
use crate::{
    replication::Replication,
    sessions::{Session, SessionPhase, Sessions},
    systems::{
        combat::CombatSystem, police::PoliceSystem, population::PopulationSystem,
        repairs::RepairSystem,
    },
    world::World,
};
use protocol::*;
use sim_core::CollisionQueries;
use std::collections::BTreeMap;
use std::result::Result;
pub struct Reservation {
    pub slot: String,
    pub owner: u32,
}

#[derive(Default)]
pub struct MissionSystem {
    pub(crate) active: BTreeMap<u32, Option<Mission>>,
    pub(crate) recoveries: BTreeMap<u32, Reservation>,
}

mod selection;
mod state;

pub use state::{Mission, Motion, Objective, Phase};

impl MissionSystem {
    pub(crate) fn accept_job(
        &mut self,
        s: &mut Session,
        template_id: &str,
        world: &mut World,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        repairs: &mut RepairSystem,
    ) -> ActionCode {
        if s.profile
            .as_ref()
            .is_some_and(|p| repairs.pending.contains_key(&p.profile_id.0))
        {
            return ActionCode::AlreadyActive;
        }
        if self.mission(s.player_id()).is_some() {
            return ActionCode::AlreadyActive;
        }
        let Some(template) = world
            .content
            .manifest
            .mission_templates
            .iter()
            .find(|t| t.id == template_id)
            .cloned()
        else {
            return ActionCode::InvalidTarget;
        };
        let id = s.player.expect("job player");
        let player = &world.players[&id.0];
        let position = player
            .state
            .vehicle_id
            .map(|id| world.vehicles[&id.0].state.motor.position.0.map(|v| v.0))
            .unwrap_or_else(|| player.state.motor.position.0.map(|v| v.0));
        let Some(terminal) = world
            .content
            .manifest
            .zones
            .iter()
            .filter(|z| {
                z.kind == content::ZoneKind::Terminal && z.allowed_templates.contains(&template.id)
            })
            .min_by(|a, b| {
                distance(position, a.center.map(|v| v as f32))
                    .total_cmp(&distance(position, b.center.map(|v| v as f32)))
            })
        else {
            return ActionCode::Unavailable;
        };
        if distance(position, terminal.center.map(|v| v as f32)) > terminal.radius.min(3.0) as f32 {
            return ActionCode::OutOfRange;
        }
        let Ok(instance) = random_id("mission-", 16) else {
            return ActionCode::Unavailable;
        };
        let variant_index = selection::variant(&instance, template.variants.len());
        let variant = variant_index.map(|i| &template.variants[usize::from(i)]);
        let duration = variant.map_or(template.duration_ticks, |v| v.duration_ticks);
        let reward = variant.map_or(template.reward_credits, |v| v.reward_credits);
        let objective = variant
            .map_or(&template.objective, |v| &v.objective)
            .clone();
        let Some(deadline) = world.tick.checked_add(duration) else {
            return ActionCode::Unavailable;
        };
        let mut m = Mission {
            id: instance,
            template: template.id,
            variant_index,
            deadline,
            target: [0.0; 3],
            radius: 5.0,
            max_speed: 2.0,
            required_dwell: 30,
            reward,
            dwell: 0,
            phase: Phase::Active,
            clear_at: None,
            objective: Objective::Courier,
            checkpoint_index: 0,
            reserved_vehicle: None,
        };
        match objective {
            content::MissionObjective::Courier {
                destination_zone_id,
                dwell_ticks,
                max_speed,
            } => {
                let Some(zone) = world
                    .content
                    .manifest
                    .zones
                    .iter()
                    .find(|z| z.id == destination_zone_id)
                else {
                    return ActionCode::Unavailable;
                };
                m.target = zone.center.map(|v| v as f32);
                m.radius = zone.radius.min(5.0) as f32;
                m.required_dwell = dwell_ticks;
                m.max_speed = max_speed as f32;
            }
            content::MissionObjective::TimeTrial {
                checkpoints,
                gate_radius,
            } => {
                if player.state.vehicle_id.is_none() {
                    return ActionCode::InvalidTarget;
                }
                if checkpoints.len() < 2
                    || distance(position, checkpoints[0].map(|v| v as f32)) > gate_radius as f32
                {
                    return ActionCode::OutOfRange;
                }
                m.checkpoint_index = 1;
                m.target = checkpoints[1].map(|v| v as f32);
                m.objective = Objective::TimeTrial {
                    checkpoints: checkpoints
                        .into_iter()
                        .map(|p| p.map(|v| v as f32))
                        .collect(),
                    gate_radius: gate_radius as f32,
                };
            }
            content::MissionObjective::Recovery {
                parking_ids,
                garage_zone_id,
            } => {
                if self.recoveries.len() >= crate::systems::population::RECOVERY_RESERVED
                    || population
                        .pools
                        .values()
                        .filter(|p| **p == crate::systems::population::VehiclePool::Parked)
                        .count()
                        >= crate::systems::population::PARKED_CAP
                {
                    return ActionCode::Unavailable;
                }
                let Some(garage) = world
                    .content
                    .manifest
                    .zones
                    .iter()
                    .find(|z| z.id == garage_zone_id)
                else {
                    return ActionCode::Unavailable;
                };
                let garage = garage.center.map(|v| v as f32);
                let slot = world
                    .content
                    .manifest
                    .parking
                    .iter()
                    .filter(|slot| {
                        if !slot.mission_reserved
                            || !parking_ids.contains(&slot.id)
                            || self.recoveries.values().any(|r| r.slot == slot.id)
                        {
                            return false;
                        }
                        let p = slot.position.map(|v| v as f32);
                        let Some(t) = world
                            .content
                            .manifest
                            .vehicle_templates
                            .iter()
                            .find(|t| t.id == slot.template_id)
                        else {
                            return false;
                        };
                        !world
                            .collision
                            .overlaps_vehicle(p, slot.yaw as f32, t.dimensions.map(|v| v as f32))
                            .unwrap_or(true)
                            && world
                                .players
                                .values()
                                .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                            && population
                                .pedestrians
                                .values()
                                .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                    })
                    .min_by_key(|slot| selection::rank(&m.id, &slot.id))
                    .cloned();
                let Some(slot) = slot else {
                    return ActionCode::Unavailable;
                };
                let template = world
                    .content
                    .manifest
                    .vehicle_templates
                    .iter()
                    .find(|t| t.id == slot.template_id)
                    .expect("validated recovery template")
                    .clone();
                let car = world.next_entity;
                let Some(next) = car.checked_add(1) else {
                    return ActionCode::Unavailable;
                };
                let Ok(motor) =
                    sim_core::spawn_vehicle(slot.position.map(|v| v as f32), slot.yaw as f32)
                else {
                    return ActionCode::Unavailable;
                };
                world.next_entity = next;
                m.reserved_vehicle = Some(car);
                m.target = slot.position.map(|v| v as f32);
                m.objective = Objective::Recovery { garage };
                world.vehicles.insert(
                    car,
                    crate::entities::vehicle::Vehicle {
                        id: EntityId(car),
                        state: VehicleState {
                            motor,
                            health: VehicleHealth(template.health),
                            driver_id: None,
                            disabled: false,
                        },
                        template,
                        idle_ticks: 30,
                    },
                );
                population
                    .pools
                    .insert(car, crate::systems::population::VehiclePool::Parked);
                if slot.secured {
                    police.secured.insert(car);
                }
                self.recoveries.insert(
                    car,
                    Reservation {
                        slot: slot.id,
                        owner: id.0,
                    },
                );
                MissionSystem::refresh_job_proxies(world);
            }
        }
        world
            .players
            .get_mut(&s.player_id())
            .expect("player equipment")
            .weapon
            .protected_until = None;
        *self.mission_mut(s.player_id()) = Some(m);
        s.status_dirty = true;
        ActionCode::Ok
    }
    pub(crate) fn refresh_job_proxies(world: &mut World) {
        world
            .collision
            .set_vehicle_proxies(
                &world
                    .vehicles
                    .values()
                    .map(|v| v.proxy())
                    .collect::<Vec<_>>(),
            )
            .expect("validated job vehicle proxies");
    }
    pub(crate) fn remove_recovery(
        &mut self,
        car: u32,
        world: &mut World,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
    ) {
        self.recoveries.remove(&car);
        world.vehicles.remove(&car);
        population.pools.remove(&car);
        police.secured.remove(&car);
        combat.homes.remove(&car);
        combat.disabled.remove(&car);
        MissionSystem::refresh_job_proxies(world);
    }
    pub(crate) fn release_disconnected_recoveries(
        &mut self,
        owner: u32,
        world: &mut World,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
    ) {
        let cars: Vec<_> = self
            .recoveries
            .iter()
            .filter(|(_, r)| r.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        for car in cars {
            self.remove_recovery(car, world, population, police, combat);
        }
    }
    pub(crate) fn retire_recovery(
        &mut self,
        peer: crate::transport::PeerKey,
        s: &mut Session,
        car: u32,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
    ) -> Result<bool, String> {
        if !self.recoveries.contains_key(&car) {
            return Ok(true);
        }
        let id = s.player.ok_or("recovery owner missing")?;
        if world.players[&id.0].state.vehicle_id == Some(EntityId(car)) {
            world
                .vehicles
                .get_mut(&car)
                .ok_or("recovery car missing")?
                .stop();
            world.collision.exclude_vehicle(Some(car));
            let exit = crate::entities::vehicle::safe_exit_with(
                &world.vehicles[&car],
                &world.collision,
                |p| CombatSystem::clear_spawn(p, Some(id.0), world, population),
            );
            world.collision.exclude_vehicle(None);
            let Some(exit) = exit else {
                return Ok(false);
            };
            let yaw = world.vehicles[&car].state.motor.yaw.0;
            world
                .vehicles
                .get_mut(&car)
                .expect("recovery car")
                .state
                .driver_id = None;
            let player = world.players.get_mut(&id.0).expect("recovery driver");
            player.state.vehicle_id = None;
            player.state.mode = CharacterMode::OnFoot;
            player.state.motor = sim_core::spawn_character(exit, yaw)?;
            world
                .players
                .get_mut(&s.player_id())
                .expect("player equipment")
                .weapon
                .reload_end = None;
            s.status_dirty = true;
            Replication::transition(peer, s, &[id.0, car], world, sessions, population, police)?;
            Replication::reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::ControlChanged(Replication::owner(s, world)?),
                world,
                sessions,
            );
        }
        self.remove_recovery(car, world, population, police, combat);
        Ok(true)
    }
    pub(crate) fn update(
        &mut self,
        profiles: &mut Profiles,
        previous: &BTreeMap<u32, VehicleMotor>,
        world: &mut World,
        sessions: &mut Sessions,
        population: &mut PopulationSystem,
        police: &mut PoliceSystem,
        combat: &mut CombatSystem,
        repairs: &mut RepairSystem,
    ) -> Result<(), String> {
        // Pending guests have no player or mission state; admitted players also update during resync.
        let peers: Vec<_> = sessions
            .peers
            .iter()
            .filter_map(|(&peer, session)| session.player.map(|_| peer))
            .collect();
        for peer in peers {
            let mut s = sessions.peers.remove(&peer).expect("mission session");
            let mut changed = s.status_dirty;
            if let Some(mut m) = self.mission_mut(s.player_id()).take() {
                let before = (m.phase, m.checkpoint_index);
                let id = s.player.ok_or("mission owner missing")?;
                let player = &world.players[&id.0];
                let car = player.state.vehicle_id.map(|id| id.0);
                let motor = car
                    .map(|id| sim_core::presentation_motor(&world.vehicles[&id].state.motor))
                    .unwrap_or_else(|| player.state.motor.clone());
                let alive = player.state.health.0 > 0
                    && matches!(
                        player.state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    )
                    && car.is_none_or(|id| world.vehicles[&id].state.health.0 > 0)
                    && m.reserved_vehicle.is_none_or(|id| {
                        world
                            .vehicles
                            .get(&id)
                            .is_some_and(|v| v.state.health.0 > 0)
                    });
                if m.phase == Phase::Active
                    && let Objective::Recovery { garage } = m.objective
                {
                    if car == m.reserved_vehicle {
                        m.target = garage;
                        if m.checkpoint_index != 2
                            || distance(motor.position.0.map(|v| v.0), garage) > m.radius
                            || motor
                                .velocity
                                .0
                                .iter()
                                .map(|v| v.0 * v.0)
                                .sum::<f32>()
                                .sqrt()
                                >= m.max_speed
                        {
                            m.checkpoint_index = 1;
                        }
                    } else {
                        m.checkpoint_index = 0;
                        if let Some(v) = m.reserved_vehicle.and_then(|id| world.vehicles.get(&id)) {
                            m.target = v.state.motor.position.0.map(|v| v.0);
                        }
                    }
                }
                m.advance(
                    world.tick,
                    crate::systems::missions::Motion {
                        alive,
                        playing: s.phase == SessionPhase::Playing,
                        position: motor.position.0.map(|v| v.0),
                        speed: motor
                            .velocity
                            .0
                            .iter()
                            .map(|v| v.0 * v.0)
                            .sum::<f32>()
                            .sqrt(),
                        vehicle: car,
                        previous: car
                            .and_then(|id| previous.get(&id))
                            .map(|m| m.position.0.map(|v| v.0)),
                    },
                );
                if m.phase == Phase::RewardPending
                    && let Some(car) = m.reserved_vehicle
                    && self.recoveries.contains_key(&car)
                    && !self.retire_recovery(
                        peer, &mut s, car, world, sessions, population, police, combat,
                    )?
                {
                    m.phase = Phase::Active;
                    m.checkpoint_index = 2;
                    m.dwell = m.required_dwell;
                }
                if m.phase == Phase::RewardPending {
                    let profile = s.profile.as_ref().ok_or("mission profile missing")?;
                    if !repairs.pending.contains_key(&profile.profile_id.0)
                        && !profiles.economic_pending(&profile.profile_id.0)
                        && let Ok(committed) =
                            profiles.apply_mission_reward(&profile.profile_id.0, &m.id, m.reward)
                    {
                        s.profile = Some(committed);
                        m.phase = Phase::Completed;
                        m.clear_at = Some(world.tick.saturating_add(150));
                    }
                }
                let mut released = true;
                if m.phase == Phase::Failed
                    && let Some(car) = m.reserved_vehicle
                {
                    released = self.retire_recovery(
                        peer, &mut s, car, world, sessions, population, police, combat,
                    )?;
                    m.checkpoint_index = if released { 0 } else { 2 };
                }
                changed |= before != (m.phase, m.checkpoint_index);
                if released && m.clear_at.is_some_and(|end| world.tick >= end) {
                    changed = true;
                } else {
                    *self.mission_mut(s.player_id()) = Some(m);
                }
            }
            if changed {
                s.status_revision = s
                    .status_revision
                    .checked_add(1)
                    .ok_or("status revision exhausted")?;
                s.status_dirty = false;
                if s.phase == SessionPhase::Playing {
                    Replication::reliable(
                        peer,
                        s.id.as_deref(),
                        ServerMessage::PlayerStatus(Replication::status(
                            world.tick, &s, world, police, self,
                        )?),
                        world,
                        sessions,
                    );
                }
            }
            sessions.peers.insert(peer, s);
        }
        Ok(())
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}

impl MissionSystem {
    pub(crate) fn mission(&self, id: u32) -> &Option<Mission> {
        &self.active[&id]
    }
    pub(crate) fn mission_mut(&mut self, id: u32) -> &mut Option<Mission> {
        self.active.get_mut(&id).expect("admitted mission state")
    }
}
