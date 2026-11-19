//! Typed queries and filters for borrowing independent actor components.
use super::*;
use bevy_ecs::{prelude::*, query::QueryData};

/// Borrows character movement, health, and occupancy as one typed query.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct CharacterQuery {
    /// Authoritative movement component borrowed by this query.
    pub motor: &'static mut CharacterMovement,
    /// Authoritative remaining health points.
    pub health: &'static mut Health,
    /// Occupancy and presentation state separate from movement.
    pub body: &'static mut CharacterBody,
}

impl Deref for CharacterQueryItem<'_, '_> {
    type Target = CharacterBody;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

impl DerefMut for CharacterQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.body
    }
}

impl Deref for CharacterQueryReadOnlyItem<'_, '_> {
    type Target = CharacterBody;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

/// Borrows vehicle movement, health, and occupancy as one typed query.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct VehicleStateQuery {
    /// Authoritative movement component borrowed by this query.
    pub motor: &'static mut VehicleMovement,
    /// Authoritative remaining health points.
    pub health: &'static mut Health,
    /// Occupancy and presentation state separate from movement.
    pub body: &'static mut VehicleBody,
}

impl Deref for VehicleStateQueryItem<'_, '_> {
    type Target = VehicleBody;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

impl DerefMut for VehicleStateQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.body
    }
}

impl Deref for VehicleStateQueryReadOnlyItem<'_, '_> {
    type Target = VehicleBody;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

/// Joins player metadata with its independent movement and health components.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct PlayerQuery {
    /// Actor-specific identity and gameplay metadata.
    pub data: &'static mut PlayerData,
    /// Borrowed movement, health, and occupancy components.
    pub state: CharacterQuery,
}

impl Deref for PlayerQueryItem<'_, '_> {
    type Target = PlayerData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for PlayerQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Deref for PlayerQueryReadOnlyItem<'_, '_> {
    type Target = PlayerData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// Joins pedestrian metadata with its independent movement and health components.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct PedestrianQuery {
    /// Actor-specific identity and gameplay metadata.
    pub data: &'static mut PedestrianData,
    /// Borrowed movement, health, and occupancy components.
    pub state: CharacterQuery,
}

impl Deref for PedestrianQueryItem<'_, '_> {
    type Target = PedestrianData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for PedestrianQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Deref for PedestrianQueryReadOnlyItem<'_, '_> {
    type Target = PedestrianData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// Joins vehicle metadata with its independent movement and health components.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct VehicleQuery {
    /// Actor-specific identity and gameplay metadata.
    pub data: &'static mut VehicleData,
    /// Borrowed movement, health, and occupancy components.
    pub state: VehicleStateQuery,
}

impl Deref for VehicleQueryItem<'_, '_> {
    type Target = VehicleData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for VehicleQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Deref for VehicleQueryReadOnlyItem<'_, '_> {
    type Target = VehicleData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// Joins officer metadata with its independent movement and health components.
#[derive(QueryData)]
#[query_data(mutable)]
pub(crate) struct OfficerQuery {
    /// Actor-specific identity and gameplay metadata.
    pub data: &'static mut OfficerData,
    /// Borrowed movement, health, and occupancy components.
    pub state: CharacterQuery,
}

impl Deref for OfficerQueryItem<'_, '_> {
    type Target = OfficerData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for OfficerQueryItem<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

impl Deref for OfficerQueryReadOnlyItem<'_, '_> {
    type Target = OfficerData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl Copy for CharacterQueryReadOnlyItem<'_, '_> {}

impl Clone for CharacterQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            motor: self.motor,
            health: self.health,
            body: self.body,
        }
    }
}

impl Copy for VehicleStateQueryReadOnlyItem<'_, '_> {}

impl Clone for VehicleStateQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            motor: self.motor,
            health: self.health,
            body: self.body,
        }
    }
}

impl Copy for PlayerQueryReadOnlyItem<'_, '_> {}

impl Clone for PlayerQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            state: self.state,
        }
    }
}

impl Copy for VehicleQueryReadOnlyItem<'_, '_> {}

impl Clone for VehicleQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            state: self.state,
        }
    }
}

impl Copy for PedestrianQueryReadOnlyItem<'_, '_> {}

impl Clone for PedestrianQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            state: self.state,
        }
    }
}

impl Copy for OfficerQueryReadOnlyItem<'_, '_> {}

impl Clone for OfficerQueryReadOnlyItem<'_, '_> {
    fn clone(&self) -> Self {
        Self {
            data: self.data,
            state: self.state,
        }
    }
}

/// Keeps player queries disjoint from other actor categories.
pub(crate) type PlayerFilter = (
    Without<VehicleData>,
    Without<PedestrianData>,
    Without<OfficerData>,
);

/// Keeps vehicle queries disjoint from other actor categories.
pub(crate) type VehicleFilter = (
    Without<PlayerData>,
    Without<PedestrianData>,
    Without<OfficerData>,
);

/// Keeps pedestrian queries disjoint from other actor categories.
pub(crate) type PedestrianFilter = (
    Without<PlayerData>,
    Without<VehicleData>,
    Without<OfficerData>,
);

/// Keeps officer queries disjoint from other actor categories.
pub(crate) type OfficerFilter = (
    Without<PlayerData>,
    Without<VehicleData>,
    Without<PedestrianData>,
);
