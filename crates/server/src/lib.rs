pub mod admission;
pub mod config;
pub mod content_loader;
pub mod control;
pub mod diagnostics;
pub mod ecs;
pub mod entities;
pub(crate) mod gameplay_messages;
pub mod metadata;
pub mod performance;
pub mod persistence;
pub mod replication;
pub mod sessions;
pub(crate) mod shutdown;
pub mod systems;
pub mod tick;
pub mod transport;
pub mod world;

#[cfg(test)]
#[path = "../tests/world_cases/mod.rs"]
mod world_cases;
