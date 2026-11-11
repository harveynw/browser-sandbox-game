pub(crate) mod native;
mod transactions;
use crate::admission::{EconomicError, Profiles};
use crate::entities::player::Player as PlayerComponent;
use crate::entities::vehicle::Vehicle as VehicleComponent;
use crate::sessions::{Session, SessionPhase, Sessions};
use crate::systems::police::Wanted as WantedComponent;
use protocol::*;
use std::collections::BTreeMap;
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

#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct RepairSystem {
    pub(crate) pending: BTreeMap<String, PendingRepair>,
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
