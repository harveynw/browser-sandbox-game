pub(crate) mod native;
use super::*;
use crate::entities::player::Player as PlayerComponent;
use crate::entities::vehicle::Vehicle as VehicleComponent;
use crate::entities::{vehicle::safe_exit_with, weapon::Weapon};

const OFFICER_CAP: usize = 16;

#[derive(bevy_ecs::prelude::Component)]
pub(crate) struct Officer {
    pub(crate) unit: u32,
    pub(crate) state: CharacterState,
    pub(crate) mounted: bool,
    pub(crate) weapon: Weapon,
    assigned: Option<u32>,
    warned_at: Option<u32>,
    aiming_since: Option<u32>,
    route: VecDeque<u32>,
    route_goal: Option<u32>,
    next_plan: u32,
}

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
