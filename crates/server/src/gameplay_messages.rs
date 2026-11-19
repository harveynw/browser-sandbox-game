//! Tick-local facts, consumed at explicit same-tick reaction checkpoints.
//! These are separate from protocol WorldEvent packets sent to clients.
use crate::systems::police::{CrimeId, CrimeReport};
use bevy_ecs::prelude::*;
use protocol::{DisplayName, EntityId};

/// First successful transition into gameplay, after the initial baseline.
#[derive(Message, Clone)]
pub(crate) struct PlayerBecameReady {
    /// Connection incarnation associated with this item.
    pub peer: crate::transport::PeerKey,
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Session ID captured to reject results from an earlier connection.
    pub session: String,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}
/// The session is detached; subscribers run before entity cleanup/despawn.
#[derive(Message, Clone)]
pub(crate) struct PlayerLeaving {
    /// Connection incarnation associated with this item.
    pub peer: crate::transport::PeerKey,
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Option<Entity>,
    /// Last announced display name of the departing player, if present.
    pub name: Option<DisplayName>,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Carries a committed shot for presentation to observers.
#[derive(Message, Clone)]
pub(crate) struct ShotEmitted {
    /// Committed shot payload sent to observing clients.
    pub event: protocol::Shot,
    /// World position in metres.
    pub position: [f32; 3],
    /// Peer that produced the shot, if a player fired it.
    pub source: Option<crate::transport::PeerKey>,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Requests a leased automatic shot for the current player and control epoch.
#[derive(Message, Clone)]
pub(crate) struct AutomaticShotRequested {
    /// Connection incarnation associated with this item.
    pub peer: crate::transport::PeerKey,
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Control generation used to reject stale client actions and inputs.
    pub epoch: u32,
    /// Aim and control scope captured for the requested automatic shot.
    pub aim: protocol::Fire,
}

/// Requests a death transition for a specific player identity.
#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerDeathRequested {
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}
/// Reports the player identity and tick of a committed death transition.
#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerDowned(
    /// Validated death transition applied to the player.
    pub PlayerDeathRequested,
);

/// Reports a completed player respawn to gameplay subscribers.
#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerRespawned {
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Reports a completed vehicle restoration to reservation subscribers.
#[derive(Message, Clone, Copy)]
pub(crate) struct VehicleRestored {
    /// Stable gameplay ID of the vehicle.
    pub vehicle: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// One successful vehicle motor step, including zero-contact steps.
#[derive(Message, Clone)]
pub(crate) struct VehicleImpact {
    /// Stable gameplay ID of the vehicle.
    pub vehicle: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Change in vehicle speed caused by the motor's contacts.
    pub delta_speed: f32,
    /// Contacts reported by the vehicle motor during this step.
    pub contacts: Vec<sim_core::Contact>,
}

/// Calculated contact damage; health and gameplay consequences are applied later.
#[derive(Message, Clone)]
pub(crate) struct VehicleContactDamage {
    /// Stable gameplay ID of the vehicle.
    pub vehicle: EntityId,
    /// ECS identity captured to reject stale actor operations.
    pub entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Collision contact reported by the motor.
    pub contact: sim_core::Contact,
    /// Health points removed by this damage event.
    pub damage: u16,
}
/// Requests pedestrian damage caused by a specific vehicle impact.
#[derive(Message, Clone)]
pub(crate) struct PedestrianImpactDamage {
    /// Stable gameplay ID of the vehicle.
    pub car: EntityId,
    /// ECS identity of the vehicle when the incident occurred.
    pub car_entity: Entity,
    /// Gameplay ID of the pedestrian hit by the vehicle.
    pub pedestrian: EntityId,
    /// ECS identity of the pedestrian when the incident occurred.
    pub pedestrian_entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Health points removed by this damage event.
    pub damage: u8,
}

/// A current occupant still needs recovery from a destroyed car (reissued for retries).
#[derive(Message, Clone)]
pub(crate) struct DisabledVehicleOccupant {
    /// Connection incarnation associated with this item.
    pub peer: crate::transport::PeerKey,
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity of the player when the incident occurred.
    pub player_entity: Entity,
    /// Stable gameplay ID of the vehicle.
    pub car: EntityId,
    /// ECS identity of the vehicle when the incident occurred.
    pub car_entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}
/// Reports a player released from a disabled vehicle after a crash.
#[derive(Message, Clone)]
pub(crate) struct CrashEjected {
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity of the player when the incident occurred.
    pub player_entity: Entity,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}
/// Gameplay has changed control; replication must advance epochs and refresh replicas.
#[derive(Message, Clone)]
pub(crate) struct ControlTransitionRequested {
    /// Connection incarnation associated with this item.
    pub peer: crate::transport::PeerKey,
    /// Stable gameplay ID of the player.
    pub player: EntityId,
    /// ECS identity of the player when the incident occurred.
    pub player_entity: Entity,
    /// Gameplay entity IDs whose control state changed.
    pub changed: Vec<u32>,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Applied health change, ready for client-facing impact presentation.
#[derive(Message, Clone)]
pub(crate) struct ImpactApplied {
    /// Gameplay ID of the actor that received the impact.
    pub target: EntityId,
    /// World position in metres.
    pub position: [f32; 3],
    /// Impact strength used by presentation effects.
    pub severity: f32,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Carries captured crime evidence and the actor identity that produced it.
#[derive(Message, Clone)]
pub(crate) struct CrimeReported {
    /// Gameplay ID of the actor that caused the crime.
    pub actor: EntityId,
    /// ECS identity of the actor when the crime occurred.
    pub actor_entity: Entity,
    /// Stable incident identity used to reject duplicate crime reports.
    pub cause: CrimeId,
    // Immutable authoritative evidence captured at the incident, not delivery time.
    pub report: CrimeReport,
}
/// Reports an audible incident at its original position and tick.
#[derive(Message, Clone, Copy)]
pub(crate) struct NoiseEmitted {
    /// World position in metres.
    pub position: [f32; 3],
    /// Simulation tick recorded for this item.
    pub tick: u32,
}
/// Reports an elimination with authoritative names for chat announcements.
#[derive(Message, Clone)]
pub(crate) struct ActorEliminated {
    /// Gameplay ID of the killer, if an actor caused the elimination.
    pub killer: Option<EntityId>,
    /// Authoritative display name used for the elimination announcement.
    pub killer_name: DisplayName,
    /// Gameplay ID of the eliminated actor.
    pub victim: EntityId,
    /// Authoritative display name of the eliminated actor.
    pub victim_name: DisplayName,
    /// Simulation tick recorded for this item.
    pub tick: u32,
}

/// Every registered reader runs before retirement. Each checkpoint gets its own
/// reader cursor; clearing the batch prevents delivery again at the next checkpoint.
/// Future subscribers must be added before clear_batch, not after this group.
pub(crate) fn clear_batch(
    mut crimes: ResMut<Messages<CrimeReported>>,
    mut noises: ResMut<Messages<NoiseEmitted>>,
    mut eliminations: ResMut<Messages<ActorEliminated>>,
) {
    crimes.clear();
    noises.clear();
    eliminations.clear();
}
/// A failed tick is not rolled back. Discard any undelivered facts rather than
/// replaying stale side effects on a later step; production exits on tick failure.
pub(crate) fn clear(world: &mut World) {
    world.resource_mut::<Messages<PlayerBecameReady>>().clear();
    world.resource_mut::<Messages<PlayerLeaving>>().clear();

    world
        .resource_mut::<Messages<crate::sessions::ingress::AdmissionRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::sessions::ingress::BaselineRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::sessions::native::DisconnectRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::sessions::native::PossessionRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::sessions::native::PossessionResult>>()
        .clear();

    world
        .resource_mut::<Messages<crate::systems::missions::native::JobActionRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::missions::native::RewardRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::missions::native::RewardCommitted>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::missions::native::RecoveryRetired>>()
        .clear();

    world
        .resource_mut::<Messages<crate::systems::police::native::ArrestRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::police::native::ArrestStarted>>()
        .clear();
    world.resource_mut::<Messages<ShotEmitted>>().clear();
    world
        .resource_mut::<Messages<crate::systems::weapons::WeaponTriggerRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::weapons::OfficerWeaponTick>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::weapons::WeaponActionRequested>>()
        .clear();
    world
        .resource_mut::<Messages<crate::systems::weapons::OfficerShotRequested>>()
        .clear();
    world
        .resource_mut::<Messages<AutomaticShotRequested>>()
        .clear();
    world
        .resource_mut::<Messages<PlayerDeathRequested>>()
        .clear();
    world.resource_mut::<Messages<PlayerDowned>>().clear();
    world.resource_mut::<Messages<PlayerRespawned>>().clear();
    world.resource_mut::<Messages<VehicleRestored>>().clear();
    world
        .resource_mut::<Messages<DisabledVehicleOccupant>>()
        .clear();
    world.resource_mut::<Messages<CrashEjected>>().clear();
    world
        .resource_mut::<Messages<ControlTransitionRequested>>()
        .clear();

    world.resource_mut::<Messages<ImpactApplied>>().clear();
    world
        .resource_mut::<Messages<VehicleContactDamage>>()
        .clear();
    world
        .resource_mut::<Messages<PedestrianImpactDamage>>()
        .clear();
    world.resource_mut::<Messages<VehicleImpact>>().clear();
    world.resource_mut::<Messages<CrimeReported>>().clear();
    world.resource_mut::<Messages<NoiseEmitted>>().clear();
    world.resource_mut::<Messages<ActorEliminated>>().clear();
}

/// Captures crime evidence and queues it for police processing.
#[cfg(test)]
pub(crate) fn report_crime(
    actor: u32,
    cause: CrimeId,
    kind: crate::systems::police::CrimeKind,
    at: [f32; 3],
    world: &mut crate::world::World,
) {
    if let Some(report) = police::observe_crime(actor, kind, at, world) {
        let actor = EntityId(actor);
        let actor_entity = world.entities.entity(actor).expect("observed actor");
        world.entities.write_message(CrimeReported {
            actor,
            actor_entity,
            cause,
            report,
        });
    }
}

#[cfg(test)]
use crate::systems::police;
