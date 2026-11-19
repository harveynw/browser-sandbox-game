pub(crate) mod native;
mod transactions;
use crate::admission::{EconomicError, Profiles};
use crate::sessions::{Session, SessionPhase, Sessions};
use crate::systems::police::Wanted as WantedComponent;
use protocol::*;
use std::collections::BTreeMap;
/// Tracks a repair's eligibility, dwell time, and durable payment progress.
pub struct PendingRepair {
    /// Session ID that started this repair.
    session: String,
    /// Gameplay ID of the player who requested the repair.
    player: u32,
    /// Control generation used to reject stale client actions and inputs.
    epoch: u32,
    /// Stable gameplay ID of the vehicle.
    car: u32,
    /// Authored repair zone selected for this request.
    zone: String,
    /// Stable event ID shared by this repair debit and its refund.
    event: String,
    /// Current stage of this item's lifecycle.
    phase: RepairPhase,
    /// Consecutive eligible ticks spent at the current target.
    dwell: u16,
    /// Whether eligibility was lost after the repair began.
    invalidated: bool,
    /// Whether the current persistence operation has been submitted.
    queued: bool,
    /// Gameplay result code exposed to the player.
    code: ActionCode,
}
impl PendingRepair {
    /// Builds a borrowed view of the state exposed to its caller.
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
    /// Returns the persistence event ID for the repair's current payment phase.
    fn operation(&self) -> String {
        if self.phase == RepairPhase::RefundPending {
            format!("refund:{}", self.event)
        } else {
            self.event.clone()
        }
    }
}

/// Tracks unfinished repair transactions by profile.
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct RepairSystem {
    /// Repair operations still awaiting completion, indexed by profile.
    pub(crate) pending: BTreeMap<String, PendingRepair>,
}

/// Returns horizontal distance between two world positions in metres.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
