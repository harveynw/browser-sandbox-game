//! Intrinsic entity bookkeeping, kept separate from shared coordination resources.
use crate::systems::{
    missions::{Mission, Reservation},
    population::VehiclePool,
};
use bevy_ecs::{component::Mutable, prelude::Component};

/// Typed value components allow existing gameplay algorithms to borrow payloads
/// without retaining a second authoritative map or changing the wire DTOs.
pub trait ComponentValue: Component<Mutability = Mutable> {
    type Value;
    fn new(value: Self::Value) -> Self;
    fn value(&self) -> &Self::Value;
    fn value_mut(&mut self) -> &mut Self::Value;
}
macro_rules! value_component {
    ($name:ident, $value:ty) => {
        #[derive(Component)]
        pub struct $name(pub $value);
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
value_component!(VehicleHome, ([f32; 3], f32));
value_component!(DisabledVehicle, (u32, u32));
value_component!(Incapacitated, u32);
value_component!(PoolMembership, VehiclePool);
value_component!(PlayerMission, Option<Mission>);
value_component!(RecoveryReservation, Reservation);
#[derive(Component)]
pub struct SecuredVehicle;
