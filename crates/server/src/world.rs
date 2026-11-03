//! Entity storage and shared simulation data. Tick orchestration lives in `tick`.
use crate::{admission::random_id, content_loader::WorldContent, entities::player::Player};
use protocol::*;
use sim_core::{CollisionQueries, CollisionWorld, QueryFilter, SimConfig};
use std::collections::BTreeMap;
use std::result::Result;
pub const BUILD_ID: &str = "protocol-simple-local";
pub const MAX_PLAYERS: usize = 16;
#[derive(Default, Debug, serde::Serialize)]
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
pub struct World {
    pub timings: crate::performance::WorldTimings,
    pub tick: u32,
    pub id: String,
    pub players: BTreeMap<u32, Player>,
    pub metrics: Metrics,
    pub vehicles: BTreeMap<u32, crate::entities::vehicle::Vehicle>,
    pub(crate) content: WorldContent,
    pub(crate) config: SimConfig,
    pub(crate) collision: CollisionWorld,
    pub(crate) next_entity: u32,
}

impl World {
    pub fn new(content: WorldContent) -> Result<Self, String> {
        let config = SimConfig::from_content(&content.config)?;
        let mut collision = CollisionWorld::from_content(&content.collision, &config)?;
        // Reserve and verify all local player positions at boot, before readiness.
        for slot in 0..MAX_PLAYERS {
            let spawn = spawn_position(&content, slot)?;
            if collision.overlaps_character(spawn, QueryFilter::MOVEMENT)? {
                return Err("player spawn obstructed".into());
            }
        }
        let mut vehicles = BTreeMap::new();
        for parking in content
            .manifest
            .parking
            .iter()
            .filter(|p| !p.mission_reserved)
        {
            let template = content
                .manifest
                .vehicle_templates
                .iter()
                .find(|t| t.id == parking.template_id)
                .ok_or("parking template missing")?
                .clone();
            sim_core::vehicle::validate_template(&template)?;
            let id = EntityId(vehicles.len() as u32 + 1);
            let motor =
                sim_core::spawn_vehicle(parking.position.map(|v| v as f32), parking.yaw as f32)?;
            if collision.overlaps_vehicle(
                motor.position.0.map(|v| v.0),
                motor.yaw.0,
                template.dimensions.map(|v| v as f32),
            )? {
                return Err("car spawn obstructed".into());
            }
            vehicles.insert(
                id.0,
                crate::entities::vehicle::Vehicle {
                    id,
                    state: VehicleState {
                        motor,
                        health: VehicleHealth(template.health),
                        driver_id: None,
                        disabled: false,
                    },
                    template,
                    idle_ticks: 30,
                },
            );
            collision
                .set_vehicle_proxies(&vehicles.values().map(|v| v.proxy()).collect::<Vec<_>>())?;
        }
        let next_entity = vehicles.len() as u32 + 1;
        let world = Self {
            timings: Default::default(),
            tick: 0,
            id: random_id("world-", 16)?,
            players: BTreeMap::new(),
            metrics: Metrics::default(),
            content,
            config,
            collision,
            next_entity,
            vehicles,
        };
        Ok(world)
    }
    pub fn build(&self) -> ServerHello {
        ServerHello {
            protocol_version: ProtocolVersion(4),
            build_id: BuildId(BUILD_ID.into()),
            content_hash: ContentHash(self.content.manifest.content_hash.clone()),
            sim_config_hash: ContentHash(self.content.manifest.sim_config_hash.clone()),
            tick_rate: TickRate(30),
            snapshot_rate: SnapshotRate(10),
        }
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
