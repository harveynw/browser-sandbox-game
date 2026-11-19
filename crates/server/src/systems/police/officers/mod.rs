pub(crate) mod native;
use super::*;
use crate::component::Weapon;
use crate::systems::vehicle::safe_exit_with;

const OFFICER_CAP: usize = 16;

/// Describes a proposed officer shot before weapon execution.
#[derive(Clone)]
pub(crate) struct OfficerShot {
    /// Stable gameplay ID of the shooter.
    pub shooter: u32,
    /// Gameplay ID of the suspect targeted by this shot.
    pub target: u32,
    /// World position where the shot begins, in metres.
    pub origin: [f32; 3],
    /// Unit direction vector of the proposed shot.
    pub direction: [f32; 3],
    /// Maximum shot distance in metres.
    pub range: f32,
    /// Health points removed by this damage event.
    pub damage: u16,
    /// Weapon ID used by this proposed shot.
    pub weapon: WeaponId,
    /// Shot sequence number used for event identity.
    pub sequence: u32,
}

pub(crate) use crate::component::OfficerSnapshot;
