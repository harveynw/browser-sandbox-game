//! World transactions for jobs, reservations and safe garage delivery.
use crate::{
    admission::{MemoryProfiles, random_id},
    missions::{Mission, Objective, Phase},
    world::{Session, SessionPhase, World},
};
use protocol::*;
use sim_core::CollisionQueries;
use std::{collections::BTreeMap, result::Result};
pub struct Reservation {
    pub slot: String,
    pub owner: u32,
}
impl World {
    pub(crate) fn accept_job(&mut self, s: &mut Session, template_id: &str) -> ActionCode {
        if s.profile
            .as_ref()
            .is_some_and(|p| self.repairs.contains_key(&p.profile_id.0))
        {
            return ActionCode::AlreadyActive;
        }
        if s.mission.is_some() {
            return ActionCode::AlreadyActive;
        }
        let Some(template) = self
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
        let player = &self.players[&id.0];
        let position = player
            .state
            .vehicle_id
            .map(|id| self.vehicles[&id.0].state.motor.position.0.map(|v| v.0))
            .unwrap_or_else(|| player.state.motor.position.0.map(|v| v.0));
        let Some(terminal) = self
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
        let Some(deadline) = self.tick.checked_add(template.duration_ticks) else {
            return ActionCode::Unavailable;
        };
        let mut m = Mission {
            id: instance,
            template: template.id,
            deadline,
            target: [0.0; 3],
            radius: 5.0,
            max_speed: 2.0,
            required_dwell: 30,
            reward: template.reward_credits,
            dwell: 0,
            phase: Phase::Active,
            clear_at: None,
            objective: Objective::Courier,
            checkpoint_index: 0,
            reserved_vehicle: None,
        };
        match template.objective {
            content::MissionObjective::Courier {
                destination_zone_id,
                dwell_ticks,
                max_speed,
            } => {
                let Some(zone) = self
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
                if self.recoveries.len() >= crate::population::RECOVERY_RESERVED
                    || self
                        .population
                        .pools
                        .values()
                        .filter(|p| **p == crate::population::VehiclePool::Parked)
                        .count()
                        >= crate::population::PARKED_CAP
                {
                    return ActionCode::Unavailable;
                }
                let Some(garage) = self
                    .content
                    .manifest
                    .zones
                    .iter()
                    .find(|z| z.id == garage_zone_id)
                else {
                    return ActionCode::Unavailable;
                };
                let garage = garage.center.map(|v| v as f32);
                let slot = self
                    .content
                    .manifest
                    .parking
                    .iter()
                    .find(|slot| {
                        if !slot.mission_reserved
                            || !parking_ids.contains(&slot.id)
                            || self.recoveries.values().any(|r| r.slot == slot.id)
                        {
                            return false;
                        }
                        let p = slot.position.map(|v| v as f32);
                        let Some(t) = self
                            .content
                            .manifest
                            .vehicle_templates
                            .iter()
                            .find(|t| t.id == slot.template_id)
                        else {
                            return false;
                        };
                        !self
                            .collision
                            .overlaps_vehicle(p, slot.yaw as f32, t.dimensions.map(|v| v as f32))
                            .unwrap_or(true)
                            && self
                                .players
                                .values()
                                .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                            && self
                                .population
                                .pedestrians
                                .values()
                                .all(|a| distance(p, a.state.motor.position.0.map(|v| v.0)) > 6.0)
                    })
                    .cloned();
                let Some(slot) = slot else {
                    return ActionCode::Unavailable;
                };
                let template = self
                    .content
                    .manifest
                    .vehicle_templates
                    .iter()
                    .find(|t| t.id == slot.template_id)
                    .expect("validated recovery template")
                    .clone();
                let car = self.next_entity;
                let Some(next) = car.checked_add(1) else {
                    return ActionCode::Unavailable;
                };
                let Ok(motor) =
                    sim_core::spawn_vehicle(slot.position.map(|v| v as f32), slot.yaw as f32)
                else {
                    return ActionCode::Unavailable;
                };
                self.next_entity = next;
                m.reserved_vehicle = Some(car);
                m.target = slot.position.map(|v| v as f32);
                m.objective = Objective::Recovery { garage };
                self.vehicles.insert(
                    car,
                    crate::vehicles::Vehicle {
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
                self.population
                    .pools
                    .insert(car, crate::population::VehiclePool::Parked);
                if slot.secured {
                    self.police.secured.insert(car);
                }
                self.recoveries.insert(
                    car,
                    Reservation {
                        slot: slot.id,
                        owner: id.0,
                    },
                );
                self.refresh_job_proxies();
            }
        }
        s.weapon.protected_until = None;
        s.mission = Some(m);
        s.status_dirty = true;
        ActionCode::Ok
    }
    fn refresh_job_proxies(&mut self) {
        self.collision
            .set_vehicle_proxies(
                &self
                    .vehicles
                    .values()
                    .map(|v| v.proxy())
                    .collect::<Vec<_>>(),
            )
            .expect("validated job vehicle proxies");
    }
    fn remove_recovery(&mut self, car: u32) {
        self.recoveries.remove(&car);
        self.vehicles.remove(&car);
        self.population.pools.remove(&car);
        self.police.secured.remove(&car);
        self.combat.homes.remove(&car);
        self.combat.disabled.remove(&car);
        self.refresh_job_proxies();
    }
    pub(crate) fn release_disconnected_recoveries(&mut self, owner: u32) {
        let cars: Vec<_> = self
            .recoveries
            .iter()
            .filter(|(_, r)| r.owner == owner)
            .map(|(&id, _)| id)
            .collect();
        for car in cars {
            self.remove_recovery(car);
        }
    }
    /// Garage delivery never falls back to teleporting the player to a spawn.
    fn retire_recovery(
        &mut self,
        peer: crate::transport::PeerKey,
        s: &mut Session,
        car: u32,
    ) -> Result<bool, String> {
        if !self.recoveries.contains_key(&car) {
            return Ok(true);
        }
        let id = s.player.ok_or("recovery owner missing")?;
        if self.players[&id.0].state.vehicle_id == Some(EntityId(car)) {
            self.vehicles
                .get_mut(&car)
                .ok_or("recovery car missing")?
                .stop();
            self.collision.exclude_vehicle(Some(car));
            let exit =
                crate::vehicles::safe_exit_with(&self.vehicles[&car], &self.collision, |p| {
                    self.clear_spawn(p, Some(id.0))
                });
            self.collision.exclude_vehicle(None);
            let Some(exit) = exit else {
                return Ok(false);
            };
            let yaw = self.vehicles[&car].state.motor.yaw.0;
            self.vehicles
                .get_mut(&car)
                .expect("recovery car")
                .state
                .driver_id = None;
            let player = self.players.get_mut(&id.0).expect("recovery driver");
            player.state.vehicle_id = None;
            player.state.mode = CharacterMode::OnFoot;
            player.state.motor = sim_core::spawn_character(exit, yaw)?;
            s.weapon.reload_end = None;
            s.status_dirty = true;
            self.transition(peer, s, &[id.0, car])?;
            self.reliable(
                peer,
                s.id.as_deref(),
                ServerMessage::ControlChanged(self.owner(s)?),
            );
        }
        self.remove_recovery(car);
        Ok(true)
    }
    pub(crate) fn advance_missions(
        &mut self,
        profiles: &mut MemoryProfiles,
        previous: &BTreeMap<u32, VehicleMotor>,
    ) -> Result<(), String> {
        for peer in self.sessions.keys().copied().collect::<Vec<_>>() {
            let mut s = self.sessions.remove(&peer).expect("mission session");
            let mut changed = s.status_dirty;
            if let Some(mut m) = s.mission.take() {
                let before = (m.phase, m.checkpoint_index);
                let id = s.player.ok_or("mission owner missing")?;
                let player = &self.players[&id.0];
                let car = player.state.vehicle_id.map(|id| id.0);
                let motor = car
                    .map(|id| sim_core::presentation_motor(&self.vehicles[&id].state.motor))
                    .unwrap_or_else(|| player.state.motor.clone());
                let alive = player.state.health.0 > 0
                    && matches!(
                        player.state.mode,
                        CharacterMode::OnFoot | CharacterMode::Driving
                    )
                    && car.is_none_or(|id| self.vehicles[&id].state.health.0 > 0)
                    && m.reserved_vehicle.is_none_or(|id| {
                        self.vehicles.get(&id).is_some_and(|v| v.state.health.0 > 0)
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
                        if let Some(v) = m.reserved_vehicle.and_then(|id| self.vehicles.get(&id)) {
                            m.target = v.state.motor.position.0.map(|v| v.0);
                        }
                    }
                }
                m.advance(
                    self.tick,
                    crate::missions::Motion {
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
                    && !self.retire_recovery(peer, &mut s, car)?
                {
                    m.phase = Phase::Active;
                    m.checkpoint_index = 2;
                    m.dwell = m.required_dwell;
                }
                if m.phase == Phase::RewardPending {
                    let profile = s.profile.as_ref().ok_or("mission profile missing")?;
                    if !self.repairs.contains_key(&profile.profile_id.0)
                        && let Ok(committed) =
                            profiles.apply_mission_reward(&profile.profile_id.0, &m.id, m.reward)
                    {
                        s.profile = Some(committed);
                        m.phase = Phase::Completed;
                        m.clear_at = Some(self.tick.saturating_add(150));
                    }
                }
                let mut released = true;
                if m.phase == Phase::Failed
                    && let Some(car) = m.reserved_vehicle
                {
                    released = self.retire_recovery(peer, &mut s, car)?;
                    m.checkpoint_index = if released { 0 } else { 2 };
                }
                changed |= before != (m.phase, m.checkpoint_index);
                if released && m.clear_at.is_some_and(|end| self.tick >= end) {
                    changed = true;
                } else {
                    s.mission = Some(m);
                }
            }
            if changed {
                s.status_revision = s
                    .status_revision
                    .checked_add(1)
                    .ok_or("status revision exhausted")?;
                s.status_dirty = false;
                if s.phase == SessionPhase::Playing {
                    self.reliable(
                        peer,
                        s.id.as_deref(),
                        ServerMessage::PlayerStatus(Self::status(self.tick, &s)?),
                    );
                }
            }
            self.sessions.insert(peer, s);
        }
        Ok(())
    }
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
