#[cfg(test)]
use crate::world::World;
pub(crate) mod native;
use crate::admission::random_id;
use crate::component::VehicleSnapshot;
use crate::{
    sessions::{Session, SessionPhase, Sessions},
    systems::repairs::RepairSystem,
};
use protocol::*;
use sim_core::CollisionQueries;
use std::collections::BTreeMap;
/// Links a reserved recovery slot to its mission owner.
pub struct Reservation {
    /// Assigned player spawn slot.
    pub slot: String,
    /// Gameplay ID of the player that owns this reservation.
    pub owner: u32,
}

/// Provides the mission system marker used by fixture setup.
#[cfg(test)]
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct MissionSystem;

mod selection;
mod state;

pub use state::{Mission, Motion, Objective, Phase};

#[cfg(test)]
impl MissionSystem {
    /// Updates collision proxies used by mission geometry checks.
    pub(crate) fn refresh_job_proxies(world: &mut World) {
        world
            .collision
            .set_vehicle_proxies(
                &world
                    .entities
                    .view::<VehicleSnapshot>()
                    .values()
                    .map(|v| crate::systems::vehicle::proxy(&v))
                    .collect::<Vec<_>>(),
            )
            .expect("validated job vehicle proxies");
    }
}

/// Returns horizontal distance between two world positions in metres.
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
