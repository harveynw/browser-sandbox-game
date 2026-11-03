//! Content coordinates are meters: +X east, +Y up, -Z north. Feet/bottom anchors.
pub use protocol::{WeaponDefinition, WeaponId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type Vec3 = [f64; 3];
macro_rules! object {
    ($name:ident { $($(#[$meta:meta])* $field:ident: $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$meta])* pub $field: $ty),* }
    };
}
object!(Bounds {
    min: Vec3,
    max: Vec3
});
// Two-component vectors in this rectangle are [X,Z], never [X,Y].
object!(Rect {
    min: [f64; 2],
    max: [f64; 2]
});
object!(Roads { x: Vec<f64>, z: Vec<f64>, carriageway_width: f64, sidewalk_width: f64 });
object!(Reservation {
    id: String,
    rect: Rect,
    clear: bool
});
object!(Access {
    id: String,
    position: Vec3,
    road_axis: Axis,
    road_center: f64,
    driveway_id: String
});
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Z,
}
object!(SpawnPoint {
    id: String,
    position: Vec3,
    yaw: f64,
    safe_radius: f64,
    reservation_id: String,
    access_id: String
});
object!(ParkingSpawn {
    id: String,
    position: Vec3,
    yaw: f64,
    template_id: String,
    secured: bool,
    mission_reserved: bool,
    reservation_id: String,
    access_id: String
});
object!(Zone { id: String, center: Vec3, radius: f64, kind: ZoneKind, reservation_id: String, access_id: String, allowed_templates: Vec<String> });
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ZoneKind {
    Delivery,
    Garage,
    Repair,
    Terminal,
}
object!(MissionTemplate {
    id: String,
    terminal_zone_id: String,
    duration_ticks: u32,
    reward_credits: u32,
    objective: MissionObjective,
    #[serde(default)]
    variants: Vec<MissionVariant>
});
object!(MissionVariant {
    name: String,
    duration_ticks: u32,
    reward_credits: u32,
    objective: MissionObjective
});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum MissionObjective {
    Courier {
        destination_zone_id: String,
        dwell_ticks: u32,
        max_speed: f64,
    },
    TimeTrial {
        checkpoints: Vec<Vec3>,
        gate_radius: f64,
    },
    Recovery {
        parking_ids: Vec<String>,
        garage_zone_id: String,
    },
}
object!(VehicleTemplate {
    id: String, dimensions: Vec3, top_speed: f64, reverse_speed: f64, acceleration: f64,
    brake: f64, wheelbase: f64, health: u16, visual_asset_id: String,
    seat_offset: Vec3, exit_offsets: Vec<Vec3>,
});
object!(CharacterConfig {
    height: f64,
    radius: f64,
    walk_speed: f64,
    sprint_speed: f64,
    ground_acceleration: f64,
    air_acceleration: f64,
    jump_speed: f64,
    jump_cooldown_ticks: u32,
    gravity: f64,
    max_fall_speed: f64,
    skin: f64,
    slope_limit_degrees: f64,
    step_height: f64,
    ground_snap: f64,
    turn_speed: f64,
});
object!(VehicleConfig {
    rolling_resistance: f64,
    handbrake: f64,
    steering_slow: f64,
    steering_fast: f64,
    steering_rate: f64,
    max_yaw_rate: f64,
    translation_substep: f64,
    yaw_substep: f64,
    max_substeps: u32,
});
object!(SimConfig {
    schema_version: u32,
    tick_rate: u32,
    character: CharacterConfig,
    vehicle: VehicleConfig
});
object!(BuildingRecipe {
    grid: f64,
    size: f64,
    spacing: f64,
    setback: f64,
    seed: u32
});
object!(Landmark {
    id: String,
    position: Vec3,
    size: Vec3,
    color: String
});
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ArchitectureStyle {
    Quay,
    Industrial,
    Modern,
    Residential,
    Civic,
    Market,
    Dockside,
}
object!(ArchitectureRecipe {
    style: ArchitectureStyle,
    width: [f64; 2],
    depth: [f64; 2],
    floors: [u32; 2],
    floor_height: f64
});
object!(BuildingArchetype {
    id: String,
    purpose: String,
    weight: u32,
    architecture: ArchitectureRecipe,
});
object!(District {
    id: String,
    name: String,
    rect: Rect,
    color: String,
    architecture: ArchitectureRecipe,
    #[serde(default)]
    archetypes: Vec<BuildingArchetype>
});
object!(Authoring {
    schema_version: u32, world_asset_id: String, bounds: Bounds, roads: Roads,
    buildings: BuildingRecipe, districts: Vec<District>, reservations: Vec<Reservation>, accesses: Vec<Access>,
    spawns: Vec<SpawnPoint>, parking: Vec<ParkingSpawn>, zones: Vec<Zone>,
    mission_templates: Vec<MissionTemplate>, vehicle_templates: Vec<VehicleTemplate>,
    landmarks: Vec<Landmark>, sim_config: SimConfig,
});
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ColliderRole {
    Ground,
    Boundary,
    Building,
    Landmark,
}
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum BoxShape {
    Box,
}
object!(StaticCollider {
    id: u32,
    shape: BoxShape,
    role: ColliderRole,
    center: Vec3,
    half_extents: Vec3,
    yaw: f64,
    flags: u32
});
object!(CollisionPackage { schema_version: u32, colliders: Vec<StaticCollider> });
object!(GraphNode {
    id: u32,
    position: Vec3
});
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    Lane,
    Sidewalk,
    Crossing,
    Driveway,
}
object!(GraphEdge {
    from: u32,
    to: u32,
    width: f64,
    speed_limit: f64,
    kind: EdgeKind
});
object!(GraphAccess {
    access_id: String,
    node_id: u32
});
object!(RoadGraph { nodes: Vec<GraphNode>, edges: Vec<GraphEdge>, accesses: Vec<GraphAccess> });
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum VisualRole {
    Vegetation,
    Ground,
    Road,
    Sidewalk,
    Marking,
    Building,
    Boundary,
    Landmark,
    Vehicle,
}
object!(BuildingIdentity {
    architecture: ArchitectureRecipe,
    purpose: String,
    neighbours: Vec<Bounds>,
});
object!(Primitive {
    id: String, role: VisualRole, center: Vec3, half_extents: Vec3, color: String,
    #[serde(deserialize_with = "required_nullable")]
    collider_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    building: Option<BuildingIdentity>,
});
object!(VisualChunk { schema_version: u32, id: String, primitives: Vec<Primitive> });
object!(DebugOverlay { schema_version: u32, colliders: Vec<StaticCollider>, reservations: Vec<Reservation>, accesses: Vec<Access> });
object!(ChunkAsset {
    id: String, cx: u32, cz: u32, visual_url: String, sha256: String,
    #[serde(deserialize_with = "required_nullable")]
    lod_url: Option<String>, bounds: Bounds,
});
object!(VisualAsset {
    id: String,
    url: String,
    sha256: String
});
object!(WorldManifest {
    weapons: [WeaponDefinition;3],
    schema_version: u32, world_asset_id: String, content_hash: String,
    sim_config_hash: String, sim_config_url: String,
    bounds: Bounds, roads: Roads, collision_url: String, collision_sha256: String,
    debug_url: String, debug_sha256: String, chunks: Vec<ChunkAsset>,
    districts: Vec<District>, backdrop: VisualAsset, minimap: VisualAsset,
    road_graph: RoadGraph, pedestrian_graph: RoadGraph,
    reservations: Vec<Reservation>, accesses: Vec<Access>, spawns: Vec<SpawnPoint>,
    parking: Vec<ParkingSpawn>, zones: Vec<Zone>, mission_templates: Vec<MissionTemplate>,
    vehicle_templates: Vec<VehicleTemplate>, vehicle_visuals: Vec<VisualAsset>,
});
object!(PackageIndex {
    schema_version: u32,
    content_hash: String,
    manifest_url: String,
    manifest_sha256: String
});
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
