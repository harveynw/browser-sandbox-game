//! Registration of the gameplay message queues owned by the ECS world.
use crate::gameplay_messages::*;
use bevy_ecs::prelude::*;
pub(crate) fn register(world: &mut World) {
    world.init_resource::<Messages<PlayerBecameReady>>();
    world.init_resource::<Messages<PlayerLeaving>>();
    world.init_resource::<Messages<crate::sessions::ingress::AdmissionRequested>>();
    world.init_resource::<Messages<crate::sessions::ingress::BaselineRequested>>();
    world.init_resource::<Messages<crate::sessions::native::DisconnectRequested>>();
    world.init_resource::<Messages<crate::sessions::native::PossessionRequested>>();
    world.init_resource::<Messages<crate::sessions::native::PossessionResult>>();

    world.init_resource::<Messages<crate::systems::missions::native::JobActionRequested>>();
    world.init_resource::<Messages<crate::systems::missions::native::RewardRequested>>();
    world.init_resource::<Messages<crate::systems::missions::native::RewardCommitted>>();
    world.init_resource::<Messages<crate::systems::missions::native::RecoveryRetired>>();

    world.init_resource::<Messages<crate::systems::police::native::ArrestRequested>>();
    world.init_resource::<Messages<crate::systems::police::native::ArrestStarted>>();
    world.init_resource::<Messages<ShotEmitted>>();
    world.init_resource::<Messages<crate::systems::weapons::WeaponTriggerRequested>>();
    world.init_resource::<Messages<crate::systems::weapons::OfficerWeaponTick>>();
    world.init_resource::<Messages<crate::systems::weapons::WeaponActionRequested>>();
    world.init_resource::<Messages<crate::systems::weapons::OfficerShotRequested>>();
    world.init_resource::<Messages<AutomaticShotRequested>>();
    world.init_resource::<Messages<PlayerDeathRequested>>();
    world.init_resource::<Messages<PlayerDowned>>();
    world.init_resource::<Messages<PlayerRespawned>>();
    world.init_resource::<Messages<VehicleRestored>>();
    world.init_resource::<Messages<DisabledVehicleOccupant>>();
    world.init_resource::<Messages<CrashEjected>>();
    world.init_resource::<Messages<ControlTransitionRequested>>();

    world.init_resource::<Messages<ImpactApplied>>();
    world.init_resource::<crate::systems::impact::ImpactCooldowns>();
    world.init_resource::<Messages<VehicleContactDamage>>();
    world.init_resource::<Messages<PedestrianImpactDamage>>();
    world.init_resource::<Messages<VehicleImpact>>();
    world.init_resource::<Messages<CrimeReported>>();
    world.init_resource::<Messages<NoiseEmitted>>();
    world.init_resource::<Messages<ActorEliminated>>();
}
