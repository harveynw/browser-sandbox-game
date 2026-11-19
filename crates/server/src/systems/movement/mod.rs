pub(crate) mod motors;
use crate::transport::PeerKey;
use protocol::*;
use std::collections::BTreeMap;
/// Inputs and pre-movement positions retained only for the current tick.
pub(crate) struct MovementFrame {
    /// Next simulation tick to process.
    pub next: u32,
    /// Peers whose movement failed during this tick.
    pub failed: Vec<PeerKey>,
    /// Controlled actor IDs mapped to their owning peers.
    pub owners: BTreeMap<u32, PeerKey>,
    /// Movement inputs indexed by gameplay actor ID.
    pub actor_inputs: BTreeMap<u32, InputFrame>,
    /// Vehicle motors captured before this movement step.
    pub previous_cars: BTreeMap<u32, VehicleMotor>,
    /// Pedestrian positions captured before this movement step.
    pub previous_peds: BTreeMap<u32, [f32; 3]>,
}
/// Coordinates character and vehicle movement within each tick.
#[cfg(test)]
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct MovementSystem;

pub(crate) mod pedestrians;
