//! Authoritative ECS component storage. Game/wire IDs remain distinct from Bevy's
//! generational handles; ordered projections carry no duplicate entity data.
use bevy_ecs::prelude::*;
use std::collections::BTreeMap;
#[cfg(test)]
use std::{marker::PhantomData, ops::Index};

/// Maps stable gameplay IDs to their current ECS entities.
#[derive(Resource, Default)]
pub(crate) struct EntityIndex(
    /// Current ECS entity indexed by stable gameplay ID.
    pub(crate) BTreeMap<u32, Entity>,
);

#[cfg(test)]
mod fixture;
#[cfg(test)]
pub use fixture::EntityStore;

#[cfg(test)]
mod tests;

mod phases;
pub(crate) use phases::healthy;
pub use phases::{TickOutcome, TickPhase, tick_schedule};
mod admission;
pub(crate) mod bootstrap;
pub(crate) mod runtime;
pub(crate) mod schedule;
pub mod state;

pub(crate) mod shutdown;

pub(crate) mod checkpoints;
pub(crate) mod messages;

pub use crate::component::GameId;

#[cfg(test)]
pub(crate) mod values;
