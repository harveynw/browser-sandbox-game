//! DTOs and scalar constraints. Keep transport and gameplay dependencies out.
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use ts_rs::TS;

fn integer_schema(min: i64, max: u64) -> Schema {
    serde_json::json!({"type": "integer", "minimum": min, "maximum": max})
        .try_into()
        .expect("a schema object is valid")
}

macro_rules! integer {
    ($name:ident, $ty:ty, $min:expr, $max:expr, $ts:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
        #[serde(transparent)]
        pub struct $name(#[ts(type = $ts)] pub $ty);
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
                let value = <$ty>::deserialize(de)?;
                if !(($min as i128)..=($max as i128)).contains(&(value as i128)) {
                    return Err(serde::de::Error::custom(concat!(
                        stringify!($name),
                        " out of range"
                    )));
                }
                Ok(Self(value))
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> {
                stringify!($name).into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                integer_schema($min as i64, $max as u64)
            }
        }
    };
}

integer!(ProtocolVersion, u16, 1, 1, "1");
integer!(TickRate, u8, 30, 30, "30");
integer!(SnapshotRate, u8, 10, 10, "10");
integer!(EntityId, u32, 1, u32::MAX, "number");
integer!(VisibilityId, u32, 1, u32::MAX, "number");
integer!(ControlEpoch, u32, 1, u32::MAX, "number");
integer!(BaselineId, u32, 1, u32::MAX, "number");
integer!(RequestId, u32, 1, u32::MAX, "number");
integer!(EventId, u32, 1, u32::MAX, "number");
integer!(Revision, u32, 1, u32::MAX, "number");
integer!(Credits, u32, 0, 1_000_000_000, "number");
integer!(InputAxis, i16, -32767, 32767, "number");
integer!(Buttons, u8, 0, 15, "number");
integer!(Heat, u8, 0, 3, "number");
integer!(Magazine, u8, 0, 12, "number");
integer!(InputLead, u8, 2, 6, "number");
integer!(EntityCount, u16, 0, 96, "number");
integer!(CharacterHealth, u16, 0, 100, "number");
integer!(VehicleHealth, u16, 0, 160, "number");

macro_rules! float {
    ($name:ident, $ty:ty, $min:expr, $max:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Serialize, TS)]
        #[serde(transparent)]
        pub struct $name(pub $ty);
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
                let value = f64::deserialize(de)?;
                if !value.is_finite() || value < ($min as f64) || value > ($max as f64) {
                    return Err(serde::de::Error::custom(concat!(stringify!($name), " out of range")));
                }
                Ok(Self(value as $ty))
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                serde_json::json!({"type": "number", "minimum": $min as f64, "maximum": $max as f64})
                    .try_into().expect("a schema object is valid")
            }
        }
    };
}

// Bounds use f32's rounded constants so a Rust-encoded endpoint is accepted.
float!(Yaw, f32, -std::f32::consts::PI, std::f32::consts::PI);
float!(Pitch, f32, -1.2_f32, 1.2_f32);
float!(Steering, f32, -0.55_f32, 0.55_f32);
float!(Scalar, f32, -f32::MAX, f32::MAX);
float!(Severity, f32, 0.0, 1.0);
float!(Milliseconds, f64, 0.0, crate::limits::MAX_SAFE_INTEGER);

pub fn valid_text(kind: &str, value: &str) -> bool {
    let count = value.chars().count();
    match kind {
        "DisplayName" => {
            (3..=20).contains(&count)
                && value.trim() == value
                && !value.chars().any(char::is_control)
        }
        "ContentHash" => {
            value.len() == 64
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        }
        "DetailText" => {
            value.len() <= crate::limits::MAX_STRING_BYTES && !value.chars().any(char::is_control)
        }
        _ => {
            (1..=64).contains(&value.len())
                && value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
        }
    }
}

macro_rules! text {
    ($name:ident, $min:expr, $max:expr, $pattern:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
        #[serde(transparent)]
        pub struct $name(pub String);
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(de: D) -> std::result::Result<Self, D::Error> {
                let value = String::deserialize(de)?;
                if !valid_text(stringify!($name), &value) {
                    return Err(serde::de::Error::custom(concat!("invalid ", stringify!($name))));
                }
                Ok(Self(value))
            }
        }
        impl JsonSchema for $name {
            fn schema_name() -> Cow<'static, str> { stringify!($name).into() }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                serde_json::json!({"type": "string", "minLength": $min,
                    "maxLength": $max, "pattern": $pattern})
                    .try_into().expect("a schema object is valid")
            }
        }
    };
}

text!(OpaqueId, 1, 64, "^[A-Za-z0-9_.:-]+$");
text!(BuildId, 1, 64, "^[A-Za-z0-9_.:-]+$");
text!(TemplateId, 1, 64, "^[A-Za-z0-9_.:-]+$");
text!(ContentHash, 64, 64, "^[0-9a-f]{64}$");
text!(DisplayName, 3, 20, "^[^\\u0000-\\u001f\\u007f-\\u009f]+$");
text!(DetailText, 0, 512, "^[^\\u0000-\\u001f\\u007f-\\u009f]*$");

pub type Tick = u32;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
pub struct Vec3(pub [Scalar; 3]);

fn required_nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    de: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(de)
}

macro_rules! wire_struct {
    ($name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $($(#[$attr])* pub $field: $ty),* }
    };
}
macro_rules! wire_enum {
    ($name:ident { $($variant:ident($ty:ty)),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case", deny_unknown_fields)]
        pub enum $name { $($variant($ty)),* }
    };
}
macro_rules! string_enum {
    ($name:ident { $($variant:ident),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, JsonSchema)]
        #[serde(rename_all = "snake_case")]
        pub enum $name { $($variant),* }
    };
}

wire_struct!(Empty {});
wire_struct!(ClientEnvelope {
    v: ProtocolVersion,
    #[serde(deserialize_with = "required_nullable")]
    world_id: Option<OpaqueId>,
    #[serde(deserialize_with = "required_nullable")]
    session_id: Option<OpaqueId>,
    message: ClientMessage,
});
wire_struct!(ServerEnvelope {
    v: ProtocolVersion,
    #[serde(deserialize_with = "required_nullable")]
    world_id: Option<OpaqueId>,
    #[serde(deserialize_with = "required_nullable")]
    session_id: Option<OpaqueId>,
    message: ServerMessage,
});
wire_enum!(ClientMessage {
    ClientHello(ClientHello), ClientReady(ClientReady), InputBatch(InputBatch),
    Action(ActionRequest), Ping(Ping), ResyncRequest(ResyncRequest), Leave(Empty),
});
wire_struct!(ClientHello {
    protocol_version: ProtocolVersion,
    build_id: BuildId,
    content_hash: ContentHash,
    sim_config_hash: ContentHash,
    join_ticket: OpaqueId,
    display_name: DisplayName,
});
wire_struct!(ClientReady {
    baseline_id: BaselineId
});
wire_struct!(InputBatch {
    control_epoch: ControlEpoch,
    #[schemars(length(min = 1, max = 3))]
    frames: Vec<InputFrame>,
});
wire_struct!(InputFrame {
    tick: Tick,
    move_x: InputAxis,
    move_z: InputAxis,
    look_yaw: Yaw,
    look_pitch: Pitch,
    buttons: Buttons,
});
wire_struct!(ActionRequest {
    request_id: RequestId,
    control_epoch: ControlEpoch,
    action: Action
});
wire_enum!(Action {
    EnterVehicle(EnterVehicle), ExitVehicle(Empty), Fire(Fire), Reload(Empty),
    AcceptMission(AcceptMission), CancelMission(Empty), Repair(Repair),
});
wire_struct!(EnterVehicle {
    vehicle_id: EntityId
});
wire_struct!(Fire {
    aim_yaw: Yaw,
    aim_pitch: Pitch
});
wire_struct!(AcceptMission {
    template_id: TemplateId
});
wire_struct!(Repair {
    zone_id: TemplateId
});
wire_struct!(Ping {
    nonce: u32,
    client_time_ms: Milliseconds
});
wire_struct!(ResyncRequest {
    reason: ResyncReason
});
string_enum!(ResyncReason {
    Stale,
    Overflow,
    InvalidState
});
string_enum!(Delivery {
    Reliable,
    Unreliable
});

wire_enum!(ServerMessage {
    ServerHello(ServerHello), Reject(Reject), Welcome(Welcome),
    BaselineBegin(BaselineBegin), EntityShow(EntityShow), BaselineEnd(BaselineEnd),
    StartPlay(StartPlay), EntityHide(EntityHide), Snapshot(Snapshot),
    OwnerState(OwnerState), ControlChanged(OwnerState), ActionResult(ActionResult),
    PlayerStatus(PlayerStatus), WorldEvent(WorldEvent), Pong(Pong), Disconnect(Disconnect),
});
wire_struct!(ServerHello {
    protocol_version: ProtocolVersion,
    build_id: BuildId,
    content_hash: ContentHash,
    sim_config_hash: ContentHash,
    tick_rate: TickRate,
    snapshot_rate: SnapshotRate,
});
wire_struct!(Reject {
    code: RejectCode,
    detail: DetailText
});
string_enum!(RejectCode {
    VersionMismatch,
    ContentMismatch,
    InvalidTicket,
    ServerFull,
    InvalidName,
    Unavailable,
});
wire_struct!(Welcome {
    player_id: EntityId,
    server_tick: Tick,
    input_lead_ticks: InputLead,
    profile: ProfileView,
});
wire_struct!(ProfileView {
    profile_id: OpaqueId,
    display_name: DisplayName,
    credits: Credits,
    completed_jobs: u32,
});
wire_struct!(BaselineBegin {
    baseline_id: BaselineId,
    server_tick: Tick
});
wire_struct!(EntityShow {
    #[serde(deserialize_with = "required_nullable")]
    baseline_id: Option<BaselineId>,
    visibility_id: VisibilityId, descriptor: EntityDescriptor, state: EntityState,
    state_tick: Tick,
});
wire_struct!(BaselineEnd {
    baseline_id: BaselineId,
    entity_count: EntityCount
});
wire_struct!(StartPlay {
    baseline_id: BaselineId,
    owner: OwnerState
});
wire_struct!(EntityHide {
    entity_id: EntityId,
    visibility_id: VisibilityId,
    server_tick: Tick,
    reason: HideReason,
});
string_enum!(HideReason {
    OutOfRange,
    Destroyed,
    Disconnected
});
wire_struct!(Snapshot {
    server_tick: Tick,
    #[schemars(length(min = 1, max = 96))]
    records: Vec<StateRecord>,
});
wire_struct!(EntityDescriptor {
    id: EntityId, entity_kind: EntityKind, template_id: TemplateId,
    #[serde(deserialize_with = "required_nullable")]
    display_name: Option<DisplayName>,
});
string_enum!(EntityKind {
    Player,
    Vehicle,
    Pedestrian,
    Police
});
wire_struct!(CharacterMotor {
    position: Vec3,
    velocity: Vec3,
    yaw: Yaw,
    grounded: bool,
    jump_cooldown_ticks: u16,
});
wire_struct!(VehicleMotor {
    position: Vec3,
    yaw: Yaw,
    speed: Scalar,
    steering: Steering,
    velocity: Vec3,
});
wire_struct!(CharacterState {
    motor: CharacterMotor, health: CharacterHealth, mode: CharacterMode,
    #[serde(deserialize_with = "required_nullable")]
    vehicle_id: Option<EntityId>,
});
wire_struct!(VehicleState {
    motor: VehicleMotor, health: VehicleHealth,
    #[serde(deserialize_with = "required_nullable")]
    driver_id: Option<EntityId>,
    disabled: bool,
});
string_enum!(CharacterMode {
    OnFoot,
    Driving,
    Downed,
    Arrested
});
wire_enum!(EntityState { Character(CharacterState), Vehicle(VehicleState) });
wire_struct!(StateRecord {
    entity_id: EntityId,
    visibility_id: VisibilityId,
    state: EntityState
});
wire_struct!(OwnerState {
    server_tick: Tick,
    consumed_through_tick: Tick,
    control_epoch: ControlEpoch,
    player_id: EntityId,
    controlled_entity_id: EntityId,
    state: EntityState,
    player_state: CharacterState,
});
wire_struct!(ActionResult {
    request_id: RequestId, applied_tick: Tick, ok: bool, code: ActionCode,
    #[serde(deserialize_with = "required_nullable")]
    affected_entity_id: Option<EntityId>,
});
string_enum!(ActionCode {
    Ok,
    NotReady,
    StaleEpoch,
    StaleRequest,
    InvalidTarget,
    OutOfRange,
    Occupied,
    TooFast,
    ExitBlocked,
    Cooldown,
    NoAmmo,
    Dead,
    Unavailable,
    InsufficientFunds,
    AlreadyActive,
    NotActive,
    PursuitActive,
});
wire_struct!(MissionView {
    instance_id: OpaqueId, template_id: TemplateId, phase: MissionPhase,
    checkpoint_index: u16, deadline_tick: Tick,
    #[serde(deserialize_with = "required_nullable")]
    target_entity_id: Option<EntityId>,
    #[serde(deserialize_with = "required_nullable")]
    target_position: Option<Vec3>,
});
string_enum!(MissionPhase {
    Active,
    Completed,
    Failed
});
wire_struct!(PlayerStatus {
    revision: Revision, server_tick: Tick, credits: Credits, completed_jobs: u32,
    heat: Heat, pursuit: Pursuit, magazine: Magazine,
    #[serde(deserialize_with = "required_nullable")]
    reload_end_tick: Option<Tick>,
    #[serde(deserialize_with = "required_nullable")]
    mission: Option<MissionView>,
    #[serde(deserialize_with = "required_nullable")]
    respawn_tick: Option<Tick>,
    #[serde(deserialize_with = "required_nullable")]
    protected_until_tick: Option<Tick>,
});
string_enum!(Pursuit {
    Clear,
    Pursuing,
    Searching,
    Arresting
});
wire_enum!(WorldEvent { Shot(Shot), Impact(Impact) });
wire_struct!(Shot {
    event_id: EventId, server_tick: Tick, shooter_id: EntityId, origin: Vec3, end: Vec3,
    #[serde(deserialize_with = "required_nullable")]
    hit_entity_id: Option<EntityId>,
});
wire_struct!(Impact {
    event_id: EventId,
    server_tick: Tick,
    entity_id: EntityId,
    position: Vec3,
    severity: Severity,
});
wire_struct!(Pong {
    nonce: u32,
    client_time_ms: Milliseconds,
    server_tick: Tick,
    server_time_ms: Milliseconds,
});
wire_struct!(Disconnect {
    code: TemplateId,
    detail: DetailText
});
