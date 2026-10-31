//! Authored content, deterministic baking and validation. No networking or physics engine.
pub mod bake;
pub mod canonical;
pub mod contract;
pub mod types;
pub mod validate;
mod vegetation;
pub use types::*;
pub type Result<T> = std::result::Result<T, String>;
