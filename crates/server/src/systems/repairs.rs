use crate::admission::{EconomicError, Profiles};
use crate::{
    sessions::{Session, SessionPhase, Sessions},
    systems::{missions::MissionSystem, police::PoliceSystem},
    world::World,
};
use protocol::*;
use std::collections::BTreeMap;
use std::result::Result;
pub struct PendingRepair {
    session: String,
    player: u32,
    epoch: u32,
    car: u32,
    zone: String,
    event: String,
    phase: RepairPhase,
    dwell: u16,
    invalidated: bool,
    queued: bool,
    code: ActionCode,
}
impl PendingRepair {
    fn view(&self) -> RepairView {
        RepairView {
            vehicle_id: EntityId(self.car),
            zone_id: TemplateId(self.zone.clone()),
            phase: self.phase,
            price: Credits(50),
            dwell_ticks: self.dwell,
            code: self.code,
        }
    }
    fn operation(&self) -> String {
        if self.phase == RepairPhase::RefundPending {
            format!("refund:{}", self.event)
        } else {
            self.event.clone()
        }
    }
}

#[derive(Default)]
pub struct RepairSystem {
    pub(crate) pending: BTreeMap<String, PendingRepair>,
}

impl RepairSystem {
    pub(crate) fn repair_eligible(
        s: &Session,
        car: u32,
        zone: &str,
        epoch: u32,
        world: &World,
        police: &PoliceSystem,
    ) -> bool {
        let Some(id) = s.player else {
            return false;
        };
        let Some(player) = world.players.get(&id.0) else {
            return false;
        };
        let Some(v) = world.vehicles.get(&car) else {
            return false;
        };
        s.phase == SessionPhase::Playing
            && s.epoch == epoch
            && player.state.health.0 > 0
            && player.state.mode == CharacterMode::Driving
            && player.state.vehicle_id == Some(EntityId(car))
            && v.state.driver_id == Some(id)
            && v.state.health.0 > 0
            && v.state.health.0 < v.template.health
            && !v.state.disabled
            && police.wanted(s.player_id()).heat == 0
            && police.wanted(s.player_id()).pursuit == Pursuit::Clear
            && v.state.motor.speed.0.abs() < 0.5
            && world.content.manifest.zones.iter().any(|z| {
                z.id == zone
                    && z.kind == content::ZoneKind::Repair
                    && distance(
                        v.state.motor.position.0.map(|v| v.0),
                        z.center.map(|v| v as f32),
                    ) <= z.radius as f32
            })
    }
    pub(crate) fn request_repair(
        &mut self,
        s: &mut Session,
        request: &Repair,
        request_id: u32,
        world: &mut World,
        police: &mut PoliceSystem,
        missions: &mut MissionSystem,
    ) -> ActionCode {
        let Some(profile) = &s.profile else {
            return ActionCode::NotReady;
        };
        let profile = profile.profile_id.0.clone();
        if self.pending.len() >= 32 {
            return ActionCode::Unavailable;
        }
        if self.pending.contains_key(&profile)
            || missions
                .mission(s.player_id())
                .as_ref()
                .is_some_and(|m| m.phase == crate::systems::missions::Phase::RewardPending)
        {
            return ActionCode::AlreadyActive;
        }
        let id = s.player.expect("repair player");
        let Some(car) = world.players[&id.0].state.vehicle_id.map(|id| id.0) else {
            return ActionCode::InvalidTarget;
        };
        if self.pending.values().any(|r| r.car == car) {
            return ActionCode::AlreadyActive;
        }
        let Some(zone) = world
            .content
            .manifest
            .zones
            .iter()
            .find(|z| z.id == request.zone_id.0 && z.kind == content::ZoneKind::Repair)
        else {
            return ActionCode::InvalidTarget;
        };
        if distance(
            world.vehicles[&car].state.motor.position.0.map(|v| v.0),
            zone.center.map(|v| v as f32),
        ) > zone.radius as f32
        {
            return ActionCode::OutOfRange;
        }
        if world.vehicles[&car].state.motor.speed.0.abs() >= 0.5 {
            return ActionCode::TooFast;
        }
        if police.wanted(s.player_id()).heat > 0
            || police.wanted(s.player_id()).pursuit != Pursuit::Clear
        {
            return ActionCode::PursuitActive;
        }
        if !RepairSystem::repair_eligible(s, car, &request.zone_id.0, s.epoch, world, police) {
            return ActionCode::Unavailable;
        }
        if s.profile.as_ref().expect("repair profile").credits.0 < 50 {
            return ActionCode::InsufficientFunds;
        }
        let sid = s.id.clone().expect("repair session");
        // Original player/session/epoch identity is rechecked on worker completion.
        let pending = PendingRepair {
            session: sid.clone(),
            player: id.0,
            epoch: s.epoch,
            car,
            zone: request.zone_id.0.clone(),
            event: format!("repair:{sid}:{request_id}"),
            phase: RepairPhase::Dwell,
            dwell: 0,
            invalidated: false,
            queued: false,
            code: ActionCode::Ok,
        };
        s.repair_view = Some(pending.view());
        s.repair_clear = None;
        s.status_dirty = true;
        self.pending.insert(profile, pending);
        ActionCode::Ok
    }
    pub(crate) fn update(
        &mut self,
        profiles: &mut Profiles,
        world: &mut World,
        sessions: &mut Sessions,
        police: &mut PoliceSystem,
    ) -> Result<(), String> {
        let mut results: BTreeMap<_, _> = profiles
            .poll_economic(world.tick)
            .into_iter()
            .map(|r| (r.event, r.result))
            .collect();
        for s in sessions.peers.values_mut() {
            if s.repair_clear.is_some_and(|end| world.tick >= end) {
                s.repair_view = None;
                s.repair_clear = None;
                s.status_dirty = true;
            }
        }
        for profile in self.pending.keys().cloned().collect::<Vec<_>>() {
            let mut r = self.pending.remove(&profile).expect("pending repair");
            let eligible = sessions
                .peers
                .values()
                .find(|s| {
                    s.id.as_deref() == Some(&r.session) && s.player == Some(EntityId(r.player))
                })
                .is_some_and(|s| {
                    RepairSystem::repair_eligible(s, r.car, &r.zone, r.epoch, world, police)
                });
            if !eligible {
                r.invalidated = true;
            }
            match r.phase {
                RepairPhase::Dwell => {
                    if !eligible {
                        r.phase = RepairPhase::Cancelled;
                        r.code = ActionCode::Unavailable;
                    } else {
                        r.dwell += 1;
                        if r.dwell >= 60 {
                            r.phase = RepairPhase::DebitPending;
                        }
                    }
                }
                RepairPhase::DebitPending => {
                    if let Some(result) = results.remove(&r.event) {
                        r.queued = false;
                        match result {
                            Ok(_) if eligible && !r.invalidated => {
                                let v =
                                    world.vehicles.get_mut(&r.car).expect("eligible repair car");
                                v.state.health = VehicleHealth(v.template.health);
                                v.idle_ticks = 0;
                                profiles.finish_repair_debit(&profile, &r.event);
                                r.phase = RepairPhase::Completed;
                                r.code = ActionCode::Ok;
                            }
                            Ok(_) => {
                                r.phase = RepairPhase::RefundPending;
                            }
                            Err(EconomicError::InsufficientFunds) => {
                                r.phase = RepairPhase::Failed;
                                r.code = ActionCode::InsufficientFunds;
                            }
                            Err(EconomicError::Unavailable) => {
                                r.code = ActionCode::Unavailable;
                            }
                        }
                    }
                }
                RepairPhase::RefundPending => {
                    if let Some(result) = results.remove(&r.operation()) {
                        r.queued = false;
                        if result.is_ok() {
                            r.phase = RepairPhase::Cancelled;
                            r.code = ActionCode::Ok;
                        } else {
                            r.code = ActionCode::Unavailable;
                        }
                    }
                }
                _ => {}
            }
            let pending = matches!(
                r.phase,
                RepairPhase::Dwell | RepairPhase::DebitPending | RepairPhase::RefundPending
            );
            if matches!(
                r.phase,
                RepairPhase::DebitPending | RepairPhase::RefundPending
            ) && !r.queued
            {
                r.queued = profiles
                    .submit_repair_event(
                        &profile,
                        &r.operation(),
                        r.phase == RepairPhase::RefundPending,
                        world.tick,
                    )
                    .is_ok();
                if !r.queued {
                    r.code = ActionCode::Unavailable;
                }
            }
            for s in sessions.peers.values_mut().filter(|s| {
                s.profile
                    .as_ref()
                    .is_some_and(|p| p.profile_id.0 == profile)
            }) {
                if let Some(committed) = profiles.committed(&profile)
                    && s.profile.as_ref() != Some(&committed)
                {
                    s.profile = Some(committed);
                    s.status_dirty = true;
                }
                let view = r.view();
                if (!pending || r.phase != RepairPhase::Dwell || world.tick.is_multiple_of(6))
                    && s.repair_view.as_ref() != Some(&view)
                {
                    s.repair_view = Some(view);
                    s.status_dirty = true;
                }
                if !pending {
                    s.repair_clear = Some(world.tick.saturating_add(150));
                }
            }
            if pending {
                self.pending.insert(profile, r);
            }
        }
        Ok(())
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
