pub(crate) mod motors;
use crate::transport::PeerKey;
use protocol::*;
use std::collections::BTreeMap;
/// Inputs and pre-movement positions retained only for the current tick.
pub(crate) struct MovementFrame {
    pub next: u32,
    pub failed: Vec<PeerKey>,
    pub owners: BTreeMap<u32, PeerKey>,
    pub actor_inputs: BTreeMap<u32, InputFrame>,
    pub previous_cars: BTreeMap<u32, VehicleMotor>,
    pub previous_peds: BTreeMap<u32, [f32; 3]>,
}
/// Coordinates character and vehicle movement within each tick.
#[cfg(test)]
#[derive(Default, bevy_ecs::prelude::Resource)]
pub struct MovementSystem;

pub(crate) mod pedestrians;
