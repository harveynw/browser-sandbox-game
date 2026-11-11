//! Test fixture adapters for tuple components.
use crate::components::*;
use crate::systems::{
    missions::{Mission, Reservation},
    population::VehiclePool,
};
use bevy_ecs::{component::Mutable, prelude::Component};

/// Typed value components allow existing gameplay algorithms to borrow payloads
/// without retaining a second authoritative map or changing the wire DTOs.
#[cfg(test)]
pub trait ComponentValue: Component<Mutability = Mutable> {
    type Value;
    fn new(value: Self::Value) -> Self;
    fn value(&self) -> &Self::Value;
    fn value_mut(&mut self) -> &mut Self::Value;
}
#[cfg(test)]
macro_rules! value_component {
    ($name:ident, $value:ty) => {
        impl ComponentValue for $name {
            type Value = $value;
            fn new(value: Self::Value) -> Self {
                Self(value)
            }
            fn value(&self) -> &Self::Value {
                &self.0
            }
            fn value_mut(&mut self) -> &mut Self::Value {
                &mut self.0
            }
        }
    };
}
#[cfg(test)]
value_component!(VehicleHome, ([f32; 3], f32));
#[cfg(test)]
value_component!(DisabledVehicle, (u32, u32));
#[cfg(test)]
value_component!(Incapacitated, u32);
#[cfg(test)]
value_component!(PoolMembership, VehiclePool);
#[cfg(test)]
value_component!(PlayerMission, Option<Mission>);
#[cfg(test)]
value_component!(RecoveryReservation, Reservation);
