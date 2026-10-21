//! Shared native/WASM kinematic simulation. No sockets, wall clock, renderer or gameplay.
pub mod character;
pub mod collision;
pub mod config;
pub mod trace;
pub mod vehicle;
pub use character::{CharacterStep, StepContext, neutral_input, spawn_character, step_character};
pub use collision::{
    CharacterCastOptions, CollisionQueries, CollisionWorld, Contact, MoveResult, QueryFilter,
    RayHit,
};
pub use config::{CharacterSettings, SimConfig};
pub use vehicle::{anchor, presentation_motor, spawn_vehicle, step_vehicle};
pub const TICK_RATE: u32 = 30;
pub const DT_SECONDS: f32 = 1.0 / TICK_RATE as f32;
pub type Result<T> = std::result::Result<T, String>;
pub(crate) const POSITION_LIMIT: f32 = 1_000_000.0;
pub(crate) fn finite3(v: [f32; 3]) -> bool {
    v.iter().all(|n| n.is_finite() && n.abs() <= POSITION_LIMIT)
}
