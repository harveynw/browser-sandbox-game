pub(crate) mod native;
use super::*;
use crate::components::Player as PlayerComponent;
use crate::components::Vehicle as VehicleComponent;
use crate::components::Weapon;
use crate::systems::vehicle::safe_exit_with;

const OFFICER_CAP: usize = 16;

#[derive(Clone)]
pub(crate) struct OfficerShot {
    pub shooter: u32,
    pub target: u32,
    pub origin: [f32; 3],
    pub direction: [f32; 3],
    pub range: f32,
    pub damage: u16,
    pub weapon: WeaponId,
    pub sequence: u32,
}

pub(crate) use crate::components::Officer;
