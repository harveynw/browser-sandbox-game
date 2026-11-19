//! Authoritative ECS component definitions and their data payloads.
use crate::systems::{
    missions::{Mission, Reservation},
    police::{CrimeId, Role, VehicleSighting},
    population::{PedestrianPhase, TrafficPhase, VehiclePool},
};
use bevy_ecs::prelude::Component;
use protocol::*;
use std::{
    collections::VecDeque,
    ops::{Deref, DerefMut},
};

pub(crate) mod snapshots;
pub(crate) use snapshots::OfficerSnapshot;
pub use snapshots::{PedestrianSnapshot, PlayerSnapshot, VehicleSnapshot};
pub(crate) mod queries;
pub(crate) mod spawn;
#[cfg(test)]
pub(crate) mod tests;

/// Stores player identity, appearance, equipment, and recovery timers.
#[derive(Component, Clone)]
pub struct PlayerData {
    /// Stable gameplay ID of this player.
    pub id: EntityId,
    /// T-shirt colour shown to other players.
    pub tshirt_colour: TshirtColour,
    /// Player name shown in presence and chat messages.
    pub display_name: DisplayName,
    /// Respawn and spawn-protection timers for this player.
    pub lifecycle: Lifecycle,
    /// Weapon inventory and action timers.
    pub weapon: Weapon,
    /// Assigned player spawn slot.
    pub(crate) slot: usize,
}

/// Per-player chat eligibility and presence; retired with the player entity.
#[derive(Component, Default)]
pub struct ChatParticipant {
    /// Time of the participant's last accepted chat message.
    pub(crate) last_sent: Option<u64>,
    /// Name last announced to other chat participants.
    pub(crate) announced_name: Option<DisplayName>,
}

/// Stores pedestrian navigation, movement intent, and flight decisions.
#[derive(Component, Clone)]
pub struct PedestrianData {
    /// Stable gameplay ID of this pedestrian.
    pub id: EntityId,
    /// Current stage of this item's lifecycle.
    pub phase: PedestrianPhase,
    /// Navigation node at the start of the current pedestrian edge.
    pub(crate) from: u32,
    /// Navigation node at the end of the current pedestrian edge.
    pub(crate) to: u32,
    /// Movement input selected for the current simulation step.
    pub(crate) input: InputFrame,
    /// Tick when the pedestrian can resume walking.
    pub(crate) wait_until: u32,
    /// First tick when the actor became distant from all observers.
    pub(crate) far_since: Option<u32>,
    /// Tick of the last movement progress check.
    pub(crate) progress_at: u32,
    /// Position recorded at the last movement progress check.
    pub(crate) progress_position: [f32; 3],
    /// Tick when the pedestrian's flight response expires.
    pub(crate) flee_until: u32,
    /// Position from which the pedestrian is fleeing.
    pub(crate) threat: [f32; 3],
}

/// Stores the vehicle identity, shared template, and idle timer.
#[derive(Component, Clone)]
pub struct VehicleData {
    /// Stable gameplay ID of this vehicle.
    pub id: EntityId,
    /// Immutable authored geometry/mechanics shared with read-only AI observations.
    pub template: std::sync::Arc<content::VehicleTemplate>,
    /// Number of consecutive ticks the vehicle has remained idle.
    pub idle_ticks: u32,
}

/// Tracks ownership, ammunition, and firing limits for one weapon.
#[derive(Clone, Default)]
pub struct Slot {
    /// Whether the actor owns this weapon.
    pub owned: bool,
    /// Rounds currently loaded in the weapon.
    pub magazine: u8,
    /// Rounds available outside the magazine.
    pub reserve: u16,
    /// Current increase in weapon spread.
    pub bloom: f32,
    /// Earliest tick when this weapon can fire again.
    pub next_fire: u32,
}

/// Tracks equipped weapons, trigger intent, and action timers.
#[derive(Clone)]
pub struct Weapon {
    /// Active automatic-fire intent and its lease deadline.
    pub trigger: Option<(Trigger, u32)>,
    /// Latest accepted trigger sequence number.
    pub trigger_sequence: u32,
    /// Whether the actor is aiming its weapon.
    pub aiming: bool,
    /// Vertical angle used to aim the weapon.
    pub aim_pitch: Pitch,
    /// Separate ammunition and cooldown state for each weapon.
    pub slots: [Slot; 3],
    /// Weapon currently selected by the actor.
    pub equipped: WeaponId,
    /// Tick when the pending reload completes.
    pub reload_end: Option<u32>,
    /// Tick when the pending weapon switch completes.
    pub equip_end: Option<u32>,
    /// Sequence number of the last committed shot.
    pub shot_sequence: u32,
}

/// Tracks respawn timing and temporary spawn protection.
#[derive(Clone, Default)]
pub struct Lifecycle {
    /// Tick when the player becomes eligible to respawn.
    pub respawn: Option<u32>,
    /// Tick when temporary spawn protection expires.
    pub protected_until: Option<u32>,
}

/// Tracks crime evidence, pursuit progress, and arrest conditions for a player.
#[derive(bevy_ecs::prelude::Component, Clone)]
pub struct Wanted {
    /// Current police response level.
    pub heat: u8,
    /// Current pursuit state reported to the player.
    pub pursuit: Pursuit,
    /// Last player position known to police.
    pub last_known: [f32; 3],
    /// Tick when police last saw the player.
    pub last_seen: u32,
    /// Last witnessed vehicle motion used for prediction.
    pub(crate) vehicle_sighting: Option<VehicleSighting>,
    /// Tick when the current police search began.
    pub(crate) search_since: Option<u32>,
    /// Accumulated crime score used to set police heat.
    pub(crate) points: u8,
    /// Tick of the player's last attack on police.
    pub(crate) last_police_attack: Option<u32>,
    /// Tick of the player's last reported gunfire.
    pub(crate) last_gunfire: Option<u32>,
    /// Earliest tick when another officer may shoot at this player.
    pub(crate) next_officer_shot: u32,
    /// Whether officers have permission to engage this player.
    pub(crate) fire_authorized: bool,
    /// Tick of the last heat increase caused by gunfire noise.
    pub(crate) last_noise_increase: Option<u32>,
    /// Number of consecutive ticks that arrest conditions have held.
    pub(crate) arrest_ticks: u32,
    /// Whether an arrest awaits execution.
    pub(crate) arrest_pending: bool,
    /// Whether the player currently meets surrender conditions.
    pub(crate) surrendering: bool,
    /// Recent crime IDs retained to reject duplicate evidence.
    pub(crate) seen: VecDeque<CrimeId>,
}

/// Stores a police vehicle's assignment, route, and recovery state.
#[derive(bevy_ecs::prelude::Component)]
pub struct Unit {
    /// Gameplay ID of the assigned suspect, if any.
    pub target: Option<u32>,
    /// Assigned role within the police response.
    pub(crate) role: Role,
    /// Earliest tick when the unit can retry an interception.
    pub(crate) intercept_retry: u32,
    /// Whether the unit is following an interception route.
    pub(crate) intercepting: bool,
    /// Assigned patrol home position.
    pub(crate) home: [f32; 3],
    /// Navigation nodes remaining on the actor's route.
    pub(crate) route: VecDeque<u32>,
    /// Movement input selected for the current simulation step.
    pub(crate) input: InputFrame,
    /// Desired vehicle speed in metres per second.
    pub(crate) target_speed: f32,
    /// Whether an obstacle currently prevents forward movement.
    pub(crate) blocked: bool,
    /// Tick when the current blockage began.
    pub(crate) blocked_since: Option<u32>,
    /// Tick when the current reversing manoeuvre ends.
    pub(crate) reverse_until: u32,
    /// Earliest tick when a failed operation can be retried.
    pub(crate) retry_after: u32,
    /// Navigation node temporarily excluded after a blockage.
    pub(crate) avoided_node: Option<u32>,
    /// Tick when the navigation exclusion expires.
    pub(crate) avoid_until: u32,
    /// Destination node used to compute the current route.
    pub(crate) route_goal: Option<u32>,
    /// Temporary waypoint used to enter a road lane.
    pub(crate) lane_entry: Option<[f32; 3]>,
    /// Earliest tick when the actor can request another route.
    pub(crate) next_plan: u32,
    /// Tick of the last movement progress check.
    pub(crate) progress_at: u32,
    /// Position recorded at the last movement progress check.
    pub(crate) progress_position: [f32; 3],
    /// Tick when a destroyed patrol becomes eligible for replacement.
    pub(crate) wreck_until: Option<u32>,
    /// Index of the current patrol destination.
    pub(crate) patrol_leg: usize,
    /// Index of the current search destination, if assigned.
    pub(crate) search_leg: Option<usize>,
}

/// Stores an ambient vehicle's route and driving decisions.
#[derive(bevy_ecs::prelude::Component)]
pub struct Traffic {
    /// Current stage of this item's lifecycle.
    pub phase: TrafficPhase,
    /// Navigation nodes remaining on the actor's route.
    pub(crate) route: VecDeque<u32>,
    /// Movement input selected for the current simulation step.
    pub(crate) input: InputFrame,
    /// Desired vehicle speed in metres per second.
    pub(crate) target_speed: f32,
    /// Earliest tick when the actor can request another route.
    pub(crate) next_plan: u32,
    /// Tick of the last movement progress check.
    pub(crate) progress_at: u32,
    /// Position recorded at the last movement progress check.
    pub(crate) progress_position: [f32; 3],
    /// First tick when the actor became distant from all observers.
    pub(crate) far_since: Option<u32>,
    /// Reserved parking slot for this traffic vehicle.
    pub(crate) parking: Option<usize>,
    /// Tick when the traffic vehicle was spawned.
    pub(crate) born: u32,
}

/// Stores officer assignment, deployment, navigation, and equipment.
#[derive(Component, Clone)]
pub struct OfficerData {
    /// Gameplay ID of the officer's patrol vehicle.
    pub(crate) unit: u32,
    /// Whether the officer remains inside its patrol vehicle.
    pub(crate) mounted: bool,
    /// Weapon inventory and action timers.
    pub(crate) weapon: Weapon,
    /// Gameplay ID of the officer's assigned suspect.
    pub(crate) assigned: Option<u32>,
    /// Tick when the officer began warning the suspect.
    pub(crate) warned_at: Option<u32>,
    /// Tick when the officer began its current aim.
    pub(crate) aiming_since: Option<u32>,
    /// Navigation nodes remaining on the actor's route.
    pub(crate) route: VecDeque<u32>,
    /// Destination node used to compute the current route.
    pub(crate) route_goal: Option<u32>,
    /// Earliest tick when the actor can request another route.
    pub(crate) next_plan: u32,
}

/// Links an ECS entity to its stable gameplay ID.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[component(immutable)]
pub struct GameId(
    /// Stable gameplay entity ID.
    pub EntityId,
);

/// Stores the vehicle's original position and yaw for restoration.
#[derive(bevy_ecs::prelude::Component)]
pub struct VehicleHome(
    /// Original world position in metres and yaw in radians.
    pub ([f32; 3], f32),
);

/// Stores the vehicle cleanup deadline and restoration retry tick.
#[derive(bevy_ecs::prelude::Component)]
pub struct DisabledVehicle(
    /// Cleanup deadline tick and next restoration retry tick.
    pub (u32, u32),
);

/// Stores the tick when an incapacitated actor must be removed.
#[derive(bevy_ecs::prelude::Component)]
pub struct Incapacitated(
    /// Simulation tick when this actor must be removed.
    pub u32,
);

/// Identifies the population pool that owns a vehicle.
#[derive(bevy_ecs::prelude::Component)]
pub struct PoolMembership(
    /// Population pool that owns this vehicle.
    pub VehiclePool,
);

/// Stores the player's current mission, if one exists.
#[derive(bevy_ecs::prelude::Component)]
pub struct PlayerMission(
    /// Current private mission, if one is active.
    pub Option<Mission>,
);

/// Reserves a recovery vehicle for its mission owner.
#[derive(bevy_ecs::prelude::Component)]
pub struct RecoveryReservation(
    /// Reserved slot and the player that owns it.
    pub Reservation,
);

/// Marks a vehicle whose theft can trigger a crime report.
#[derive(bevy_ecs::prelude::Component)]
pub struct SecuredVehicle;

/// Stores a character's complete authoritative movement state.
#[derive(Component, Clone)]
pub struct CharacterMovement(
    /// Position, velocity, facing, grounding, and jump timing.
    pub CharacterMotor,
);

/// Stores a vehicle's complete authoritative movement state.
#[derive(Component, Clone)]
pub struct VehicleMovement(
    /// Complete vehicle motor used by the shared simulation.
    pub VehicleMotor,
);

/// Stores remaining health points for any damageable actor.
#[derive(Component, Clone, Copy)]
pub struct Health(
    /// Remaining health points; maximum health comes from the actor definition.
    pub u16,
);

/// Stores character presentation and vehicle occupancy independently of movement.
#[derive(Component, Clone)]
pub struct CharacterBody {
    /// Current character movement or lifecycle mode.
    pub mode: CharacterMode,
    /// Gameplay ID of the occupied vehicle, if any.
    pub vehicle_id: Option<EntityId>,
    /// Equipment presentation maintained for this character.
    pub equipment: Option<EquipmentView>,
}

/// Stores vehicle occupancy and disabled state independently of movement.
#[derive(Component, Clone)]
pub struct VehicleBody {
    /// Gameplay ID of the vehicle driver, if occupied.
    pub driver_id: Option<EntityId>,
    /// Whether the vehicle is unavailable for normal movement and entry.
    pub disabled: bool,
}

impl Deref for CharacterMovement {
    type Target = CharacterMotor;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for CharacterMovement {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Deref for VehicleMovement {
    type Target = VehicleMotor;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for VehicleMovement {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
