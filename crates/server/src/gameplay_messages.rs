//! Tick-local facts, consumed at explicit same-tick reaction checkpoints.
//! These are separate from protocol WorldEvent packets sent to clients.
use crate::systems::police::{CrimeId, CrimeReport};
use bevy_ecs::prelude::*;
use protocol::{DisplayName, EntityId};

/// First successful transition into gameplay, after the initial baseline.
#[derive(Message, Clone)]
pub(crate) struct PlayerBecameReady {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub entity: Entity,
    pub session: String,
    pub tick: u32,
}
/// The session is detached; subscribers run before entity cleanup/despawn.
#[derive(Message, Clone)]
pub(crate) struct PlayerLeaving {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub entity: Option<Entity>,
    pub name: Option<DisplayName>,
    pub tick: u32,
}

#[derive(Message, Clone)]
pub(crate) struct ShotEmitted {
    pub event: protocol::Shot,
    pub position: [f32; 3],
    pub source: Option<crate::transport::PeerKey>,
    pub tick: u32,
}

#[derive(Message, Clone)]
pub(crate) struct AutomaticShotRequested {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub entity: Entity,
    pub tick: u32,
    pub epoch: u32,
    pub aim: protocol::Fire,
}

#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerDeathRequested {
    pub player: EntityId,
    pub entity: Entity,
    pub tick: u32,
}
#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerDowned(pub PlayerDeathRequested);

#[derive(Message, Clone, Copy)]
pub(crate) struct PlayerRespawned {
    pub player: EntityId,
    pub entity: Entity,
    pub tick: u32,
}

#[derive(Message, Clone, Copy)]
pub(crate) struct VehicleRestored {
    pub vehicle: EntityId,
    pub entity: Entity,
    pub tick: u32,
}

/// One successful vehicle motor step, including zero-contact steps.
#[derive(Message, Clone)]
pub(crate) struct VehicleImpact {
    pub vehicle: EntityId,
    pub entity: Entity,
    pub tick: u32,
    pub delta_speed: f32,
    pub contacts: Vec<sim_core::Contact>,
}

/// Calculated contact damage; health and gameplay consequences are applied later.
#[derive(Message, Clone)]
pub(crate) struct VehicleContactDamage {
    pub vehicle: EntityId,
    pub entity: Entity,
    pub tick: u32,
    pub contact: sim_core::Contact,
    pub damage: u16,
}
#[derive(Message, Clone)]
pub(crate) struct PedestrianImpactDamage {
    pub car: EntityId,
    pub car_entity: Entity,
    pub pedestrian: EntityId,
    pub pedestrian_entity: Entity,
    pub tick: u32,
    pub damage: u8,
}

/// A current occupant still needs recovery from a destroyed car (reissued for retries).
#[derive(Message, Clone)]
pub(crate) struct DisabledVehicleOccupant {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub player_entity: Entity,
    pub car: EntityId,
    pub car_entity: Entity,
    pub tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct CrashEjected {
    pub player: EntityId,
    pub player_entity: Entity,
    pub tick: u32,
}
/// Gameplay has changed control; replication must advance epochs and refresh replicas.
#[derive(Message, Clone)]
pub(crate) struct ControlTransitionRequested {
    pub peer: crate::transport::PeerKey,
    pub player: EntityId,
    pub player_entity: Entity,
    pub changed: Vec<u32>,
    pub tick: u32,
}

/// Applied health change, ready for client-facing impact presentation.
#[derive(Message, Clone)]
pub(crate) struct ImpactApplied {
    pub target: EntityId,
    pub position: [f32; 3],
    pub severity: f32,
    pub tick: u32,
}

#[derive(Message, Clone)]
pub(crate) struct CrimeReported {
    pub actor: EntityId,
    pub actor_entity: Entity,
    pub cause: CrimeId,
    // Immutable authoritative evidence captured at the incident, not delivery time.
    pub report: CrimeReport,
}
#[derive(Message, Clone, Copy)]
pub(crate) struct NoiseEmitted {
    pub position: [f32; 3],
    pub tick: u32,
}
#[derive(Message, Clone)]
pub(crate) struct ActorEliminated {
    pub killer: Option<EntityId>,
    pub killer_name: DisplayName,
    pub victim: EntityId,
    pub victim_name: DisplayName,
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
