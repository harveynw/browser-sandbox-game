//! Shared world metadata, build information and geometry helpers.
//! The owned World wrapper below exists only for scenario fixtures.
use crate::content_loader::WorldContent;
#[cfg(test)]
use crate::ecs::EntityStore;
use protocol::*;
#[cfg(test)]
use sim_core::{CollisionWorld, SimConfig};
use std::result::Result;
pub const BUILD_ID: &str = "protocol-simple-local";
pub const MAX_PLAYERS: usize = 16;
/// Counts rejected inputs, network traffic, and replication limits.
#[derive(Default, Debug, serde::Serialize, bevy_ecs::prelude::Resource)]
pub struct Metrics {
    /// Number of movement inputs rejected by the rate limit.
    pub rate_limited_inputs: u64,
    /// Number of inputs rejected because their tick has passed.
    pub late_inputs: u64,
    /// Number of inputs rejected because their tick is too far ahead.
    pub future_inputs: u64,
    /// Number of conflicting duplicate input frames received.
    pub conflicting_inputs: u64,
    /// Number of messages rejected for an old control generation.
    pub stale_epochs: u64,
    /// Total gameplay bytes received by the server.
    pub ingress_bytes: u64,
    /// Total gameplay bytes sent by the server.
    pub egress_bytes: u64,
    /// Number of outputs dropped because their byte budget was exhausted.
    pub budget_drops: u64,
    /// Total number of snapshot packets emitted.
    pub snapshot_packets: u64,
    /// Largest encoded snapshot packet size in bytes.
    pub max_snapshot_bytes: usize,
    /// Largest observed gap between entity refreshes in ticks.
    pub max_refresh_age_ticks: u32,
}
/// Owns world state for scenario fixtures between scheduled ticks.
#[cfg(test)]
pub struct World {
    /// Distributions of work duration by tick phase.
    pub timings: crate::performance::WorldTimings,
    /// Simulation tick recorded for this item.
    pub tick: u32,
    /// Unique ID of this item within its owning scope.
    pub id: String,
    /// Measurements collected for server diagnostics.
    pub metrics: Metrics,
    /// Actor component storage used by the fixture.
    pub entities: EntityStore,
    /// Validated world content used by this operation.
    pub(crate) content: WorldContent,
    /// Runtime simulation settings.
    pub(crate) config: SimConfig,
    /// Collision queries used to validate movement and placement.
    pub(crate) collision: CollisionWorld,
    /// Next available stable gameplay entity ID.
    pub(crate) next_entity: u32,
}

#[cfg(test)]
impl World {
    /// Creates an owned scenario world from validated content.
    pub fn new(content: WorldContent) -> Result<Self, String> {
        Ok(crate::ecs::state::unpack_core(crate::ecs::bootstrap::base(
            content,
        )?))
    }
    /// Returns the protocol, content hashes, and update rates advertised to clients.
    pub fn build(&self) -> ServerHello {
        build(&self.content)
    }
}
/// Returns the protocol, content hashes, and update rates advertised to clients.
pub fn build(content: &WorldContent) -> ServerHello {
    ServerHello {
        protocol_version: ProtocolVersion(4),
        build_id: BuildId(BUILD_ID.into()),
        content_hash: ContentHash(content.manifest.content_hash.clone()),
        sim_config_hash: ContentHash(content.manifest.sim_config_hash.clone()),
        tick_rate: TickRate(30),
        snapshot_rate: SnapshotRate(10),
    }
}
/// Places a player slot on the grid around the first authored spawn.
pub(crate) fn spawn_position(content: &WorldContent, slot: usize) -> Result<[f32; 3], String> {
    let origin = content
        .manifest
        .spawns
        .first()
        .ok_or("missing spawn")?
        .position;
    Ok([
        origin[0] as f32 + ((slot % 4) as f32 - 1.5) * 2.0,
        origin[1] as f32,
        origin[2] as f32 + ((slot / 4) as f32 - 1.5) * 2.0,
    ])
}
/// Builds a typed server envelope from a scenario's JSON payload.
#[cfg(test)]
pub(crate) fn test_envelope(
    world: &str,
    sid: Option<&str>,
    kind: &str,
    data: serde_json::Value,
) -> ServerEnvelope {
    serde_json::from_value(serde_json::json!({"v":4,"world_id":world,"session_id":sid,"message":{"kind":kind,"data":data}})).unwrap()
}

/// Extracts a character or vehicle world position from replicated state.
pub(crate) fn entity_position(state: &EntityState) -> [f32; 3] {
    match state {
        EntityState::Character(v) => v.motor.position.0.map(|v| v.0),
        EntityState::Vehicle(v) => v.motor.position.0.map(|v| v.0),
    }
}
