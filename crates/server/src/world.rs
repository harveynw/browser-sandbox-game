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
#[derive(Default, Debug, serde::Serialize, bevy_ecs::prelude::Resource)]
pub struct Metrics {
    pub rate_limited_inputs: u64,
    pub late_inputs: u64,
    pub future_inputs: u64,
    pub conflicting_inputs: u64,
    pub stale_epochs: u64,
    pub ingress_bytes: u64,
    pub egress_bytes: u64,
    pub budget_drops: u64,
    pub snapshot_packets: u64,
    pub max_snapshot_bytes: usize,
    pub max_refresh_age_ticks: u32,
}
#[cfg(test)]
pub struct World {
    pub timings: crate::performance::WorldTimings,
    pub tick: u32,
    pub id: String,
    pub metrics: Metrics,
    pub entities: EntityStore,
    pub(crate) content: WorldContent,
    pub(crate) config: SimConfig,
    pub(crate) collision: CollisionWorld,
    pub(crate) next_entity: u32,
}

#[cfg(test)]
impl World {
    pub fn new(content: WorldContent) -> Result<Self, String> {
        Ok(crate::ecs::state::unpack_core(crate::ecs::bootstrap::base(
            content,
        )?))
    }
    pub fn build(&self) -> ServerHello {
        build(&self.content)
    }
}
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
#[cfg(test)]
pub(crate) fn test_envelope(
    world: &str,
    sid: Option<&str>,
    kind: &str,
    data: serde_json::Value,
) -> ServerEnvelope {
    serde_json::from_value(serde_json::json!({"v":4,"world_id":world,"session_id":sid,"message":{"kind":kind,"data":data}})).unwrap()
}

pub(crate) fn entity_position(state: &EntityState) -> [f32; 3] {
    match state {
        EntityState::Character(v) => v.motor.position.0.map(|v| v.0),
        EntityState::Vehicle(v) => v.motor.position.0.map(|v| v.0),
    }
}
