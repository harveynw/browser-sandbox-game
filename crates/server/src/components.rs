//! Authoritative ECS components and their data payloads. Behaviour lives in systems.
use crate::systems::{
    missions::{Mission, Reservation},
    police::{CrimeId, Role, VehicleSighting},
    population::{PedestrianPhase, TrafficPhase, VehiclePool},
};
use bevy_ecs::prelude::Component;
use protocol::*;
use std::collections::VecDeque;

#[derive(bevy_ecs::prelude::Component)]
pub struct Player {
    pub id: EntityId,
    pub state: CharacterState,
    pub tshirt_colour: TshirtColour,
    pub display_name: DisplayName,
    pub lifecycle: Lifecycle,
    pub weapon: Weapon,
    pub(crate) slot: usize,
}

/// Per-player chat eligibility and presence; retired with the player entity.
#[derive(Component, Default)]
pub struct ChatParticipant {
    pub(crate) last_sent: Option<u64>,
    pub(crate) announced_name: Option<DisplayName>,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Pedestrian {
    pub id: EntityId,
    pub state: CharacterState,
    pub phase: PedestrianPhase,
    pub(crate) from: u32,
    pub(crate) to: u32,
    pub(crate) input: InputFrame,
    pub(crate) wait_until: u32,
    pub(crate) far_since: Option<u32>,
    pub(crate) progress_at: u32,
    pub(crate) progress_position: [f32; 3],
    pub(crate) flee_until: u32,
    pub(crate) threat: [f32; 3],
}

#[derive(bevy_ecs::prelude::Component, Clone)]
pub struct Vehicle {
    pub id: EntityId,
    /// Immutable authored geometry/mechanics shared with read-only AI observations.
    pub template: std::sync::Arc<content::VehicleTemplate>,
    pub state: VehicleState,
    pub idle_ticks: u32,
}

#[derive(Clone, Default)]
pub struct Slot {
    pub owned: bool,
    pub magazine: u8,
    pub reserve: u16,
    pub bloom: f32,
    pub next_fire: u32,
}

#[derive(Clone)]
pub struct Weapon {
    pub trigger: Option<(Trigger, u32)>,
    pub trigger_sequence: u32,
    pub aiming: bool,
    pub aim_pitch: Pitch,
    pub slots: [Slot; 3],
    pub equipped: WeaponId,
    pub reload_end: Option<u32>,
    pub equip_end: Option<u32>,
    pub shot_sequence: u32,
}

#[derive(Default)]
pub struct Lifecycle {
    pub respawn: Option<u32>,
    pub protected_until: Option<u32>,
}

#[derive(bevy_ecs::prelude::Component, Clone)]
pub struct Wanted {
    pub heat: u8,
    pub pursuit: Pursuit,
    pub last_known: [f32; 3],
    pub last_seen: u32,
    pub(crate) vehicle_sighting: Option<VehicleSighting>,
    pub(crate) search_since: Option<u32>,
    pub(crate) points: u8,
    pub(crate) last_police_attack: Option<u32>,
    pub(crate) last_gunfire: Option<u32>,
    pub(crate) next_officer_shot: u32,
    pub(crate) fire_authorized: bool,
    pub(crate) last_noise_increase: Option<u32>,
    pub(crate) arrest_ticks: u32,
    pub(crate) arrest_pending: bool,
    pub(crate) surrendering: bool,
    pub(crate) seen: VecDeque<CrimeId>,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Unit {
    pub target: Option<u32>,
    pub(crate) role: Role,
    pub(crate) intercept_retry: u32,
    pub(crate) intercepting: bool,
    pub(crate) home: [f32; 3],
    pub(crate) route: VecDeque<u32>,
    pub(crate) input: InputFrame,
    pub(crate) target_speed: f32,
    pub(crate) blocked: bool,
    pub(crate) blocked_since: Option<u32>,
    pub(crate) reverse_until: u32,
    pub(crate) retry_after: u32,
    pub(crate) avoided_node: Option<u32>,
    pub(crate) avoid_until: u32,
    pub(crate) route_goal: Option<u32>,
    pub(crate) lane_entry: Option<[f32; 3]>,
    pub(crate) next_plan: u32,
    pub(crate) progress_at: u32,
    pub(crate) progress_position: [f32; 3],
    pub(crate) wreck_until: Option<u32>,
    pub(crate) patrol_leg: usize,
    pub(crate) search_leg: Option<usize>,
}

#[derive(bevy_ecs::prelude::Component)]
pub struct Traffic {
    pub phase: TrafficPhase,
    pub(crate) route: VecDeque<u32>,
    pub(crate) input: InputFrame,
    pub(crate) target_speed: f32,
    pub(crate) next_plan: u32,
    pub(crate) progress_at: u32,
    pub(crate) progress_position: [f32; 3],
    pub(crate) far_since: Option<u32>,
    pub(crate) parking: Option<usize>,
    pub(crate) born: u32,
}

#[derive(bevy_ecs::prelude::Component)]
pub(crate) struct Officer {
    pub(crate) unit: u32,
    pub(crate) state: CharacterState,
    pub(crate) mounted: bool,
    pub(crate) weapon: Weapon,
    pub(crate) assigned: Option<u32>,
    pub(crate) warned_at: Option<u32>,
    pub(crate) aiming_since: Option<u32>,
    pub(crate) route: VecDeque<u32>,
    pub(crate) route_goal: Option<u32>,
    pub(crate) next_plan: u32,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable)]
pub struct GameId(pub EntityId);

#[derive(bevy_ecs::prelude::Component)]
pub struct VehicleHome(pub ([f32; 3], f32));

#[derive(bevy_ecs::prelude::Component)]
pub struct DisabledVehicle(pub (u32, u32));

#[derive(bevy_ecs::prelude::Component)]
pub struct Incapacitated(pub u32);

#[derive(bevy_ecs::prelude::Component)]
pub struct PoolMembership(pub VehiclePool);

#[derive(bevy_ecs::prelude::Component)]
pub struct PlayerMission(pub Option<Mission>);

#[derive(bevy_ecs::prelude::Component)]
pub struct RecoveryReservation(pub Reservation);

#[derive(bevy_ecs::prelude::Component)]
pub struct SecuredVehicle;
