//! Complete actor bundles and indexed entity construction.
use super::*;
use crate::ecs::{EntityIndex, GameId};
use bevy_ecs::prelude::*;

/// Spawns player metadata, movement, health, and occupancy together.
#[derive(Bundle)]
pub(crate) struct PlayerBundle {
    /// Actor-specific identity and gameplay metadata.
    pub data: PlayerData,
    /// Complete motor state stored as its own ECS component.
    pub movement: CharacterMovement,
    /// Authoritative remaining health points.
    pub health: Health,
    /// Occupancy and presentation state separate from movement.
    pub body: CharacterBody,
}

impl PlayerSnapshot {
    /// Splits a spawn or snapshot value into its authoritative ECS components.
    pub(crate) fn into_bundle(self) -> PlayerBundle {
        PlayerBundle {
            data: PlayerData {
                id: self.id,
                tshirt_colour: self.tshirt_colour,
                display_name: self.display_name,
                lifecycle: self.lifecycle,
                weapon: self.weapon,
                slot: self.slot,
            },
            movement: CharacterMovement(self.state.motor),
            health: Health(self.state.health.0),
            body: CharacterBody {
                mode: self.state.mode,
                vehicle_id: self.state.vehicle_id,
                equipment: self.state.equipment,
            },
        }
    }
}

/// Spawns pedestrian metadata, movement, health, and occupancy together.
#[derive(Bundle)]
pub(crate) struct PedestrianBundle {
    /// Actor-specific identity and gameplay metadata.
    pub data: PedestrianData,
    /// Complete motor state stored as its own ECS component.
    pub movement: CharacterMovement,
    /// Authoritative remaining health points.
    pub health: Health,
    /// Occupancy and presentation state separate from movement.
    pub body: CharacterBody,
}

impl PedestrianSnapshot {
    /// Splits a spawn or snapshot value into its authoritative ECS components.
    pub(crate) fn into_bundle(self) -> PedestrianBundle {
        PedestrianBundle {
            data: PedestrianData {
                id: self.id,
                phase: self.phase,
                from: self.from,
                to: self.to,
                input: self.input,
                wait_until: self.wait_until,
                far_since: self.far_since,
                progress_at: self.progress_at,
                progress_position: self.progress_position,
                flee_until: self.flee_until,
                threat: self.threat,
            },
            movement: CharacterMovement(self.state.motor),
            health: Health(self.state.health.0),
            body: CharacterBody {
                mode: self.state.mode,
                vehicle_id: self.state.vehicle_id,
                equipment: self.state.equipment,
            },
        }
    }
}

/// Spawns vehicle metadata, movement, health, and occupancy together.
#[derive(Bundle)]
pub(crate) struct VehicleBundle {
    /// Actor-specific identity and gameplay metadata.
    pub data: VehicleData,
    /// Complete motor state stored as its own ECS component.
    pub movement: VehicleMovement,
    /// Authoritative remaining health points.
    pub health: Health,
    /// Occupancy and presentation state separate from movement.
    pub body: VehicleBody,
}

impl VehicleSnapshot {
    /// Splits a spawn or snapshot value into its authoritative ECS components.
    pub(crate) fn into_bundle(self) -> VehicleBundle {
        VehicleBundle {
            data: VehicleData {
                id: self.id,
                template: self.template,
                idle_ticks: self.idle_ticks,
            },
            movement: VehicleMovement(self.state.motor),
            health: Health(self.state.health.0),
            body: VehicleBody {
                driver_id: self.state.driver_id,
                disabled: self.state.disabled,
            },
        }
    }
}

/// Spawns officer metadata, movement, health, and occupancy together.
#[derive(Bundle)]
pub(crate) struct OfficerBundle {
    /// Actor-specific identity and gameplay metadata.
    pub data: OfficerData,
    /// Complete motor state stored as its own ECS component.
    pub movement: CharacterMovement,
    /// Authoritative remaining health points.
    pub health: Health,
    /// Occupancy and presentation state separate from movement.
    pub body: CharacterBody,
}

impl OfficerSnapshot {
    /// Splits a spawn or snapshot value into its authoritative ECS components.
    pub(crate) fn into_bundle(self) -> OfficerBundle {
        OfficerBundle {
            data: OfficerData {
                unit: self.unit,
                mounted: self.mounted,
                weapon: self.weapon,
                assigned: self.assigned,
                warned_at: self.warned_at,
                aiming_since: self.aiming_since,
                route: self.route,
                route_goal: self.route_goal,
                next_plan: self.next_plan,
            },
            movement: CharacterMovement(self.state.motor),
            health: Health(self.state.health.0),
            body: CharacterBody {
                mode: self.state.mode,
                vehicle_id: self.state.vehicle_id,
                equipment: self.state.equipment,
            },
        }
    }
}

/// Creates an indexed actor through deferred ECS commands.
pub(crate) fn spawn(commands: &mut Commands, id: u32, bundle: impl Bundle) {
    commands.queue(move |world: &mut World| {
        assert!(
            !world.resource::<EntityIndex>().0.contains_key(&id),
            "duplicate actor ID"
        );
        let entity = world.spawn((GameId(EntityId(id)), bundle)).id();
        world.resource_mut::<EntityIndex>().0.insert(id, entity);
    });
}

/// Removes an actor and its stable gameplay-ID mapping.
pub(crate) fn despawn(commands: &mut Commands, id: u32, entity: Entity) {
    commands.queue(move |world: &mut World| {
        crate::systems::lifecycle::despawn_indexed(world, entity, EntityId(id))
    });
}

/// A director's local command staging: existing components remain borrowed in
/// place; only newly created actors are owned until deferred spawn is applied.
pub(crate) enum Pending<'a, T> {
    Live(&'a mut T),
    Spawn(T),
}

impl<T> std::ops::Deref for Pending<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        match self {
            Self::Live(v) => v,
            Self::Spawn(v) => v,
        }
    }
}

impl<T> std::ops::DerefMut for Pending<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        match self {
            Self::Live(v) => v,
            Self::Spawn(v) => v,
        }
    }
}
