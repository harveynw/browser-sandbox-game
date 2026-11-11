#[cfg(test)]
use crate::world::World;
pub(crate) mod native;
use crate::admission::random_id;
use crate::entities::pedestrian::Pedestrian as PedestrianComponent;
use crate::entities::player::Player as PlayerComponent;
use crate::entities::vehicle::Vehicle as VehicleComponent;
use crate::{
    sessions::{Session, SessionPhase, Sessions},
    systems::repairs::RepairSystem,
};
use protocol::*;
use sim_core::CollisionQueries;
use std::collections::BTreeMap;
pub struct Reservation {
    pub slot: String,
    pub owner: u32,
}

#[cfg(test)]
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct MissionSystem;

mod selection;
mod state;

pub use state::{Mission, Motion, Objective, Phase};

#[cfg(test)]
impl MissionSystem {
    pub(crate) fn refresh_job_proxies(world: &mut World) {
        world
            .collision
            .set_vehicle_proxies(
                &world
                    .entities
                    .view::<VehicleComponent>()
                    .values()
                    .map(|v| v.proxy())
                    .collect::<Vec<_>>(),
            )
            .expect("validated job vehicle proxies");
    }
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
}
