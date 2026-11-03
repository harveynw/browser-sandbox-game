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

integer!(ProtocolVersion, u16, 4, 4, "4");
integer!(TickRate, u8, 30, 30, "30");
integer!(SnapshotRate, u8, 10, 10, "10");
integer!(EntityId, u32, 1, u32::MAX, "number");
integer!(VisibilityId, u32, 1, u32::MAX, "number");
integer!(ControlEpoch, u32, 1, u32::MAX, "number");
integer!(BaselineId, u32, 1, u32::MAX, "number");
integer!(RequestId, u32, 1, u32::MAX, "number");
integer!(EventId, u32, 1, u32::MAX, "number");
integer!(Revision, u32, 1, u32::MAX, "number");
// Packed RGB colour, shared by profile storage and player appearance.
integer!(TshirtColour, u32, 0, 0xffffff, "number");
impl Default for TshirtColour {
    fn default() -> Self {
        Self(0xe4a25b)
    }
}
integer!(Credits, u32, 0, 1_000_000_000, "number");
integer!(InputAxis, i16, -32767, 32767, "number");
integer!(Buttons, u8, 0, 15, "number");
integer!(Heat, u8, 0, 3, "number");
integer!(Magazine, u8, 0, 50, "number");
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
        "ChatText" => {
            !value.trim().is_empty()
                && count <= 256
                && value.len() <= 512
                && !value.chars().any(|c| c.is_control() && c != '\n')
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
text!(
    ChatText,
    1,
    256,
    "^[^\\u0000-\\u0009\\u000b-\\u001f\\u007f-\\u009f]+$"
);
text!(DisplayName, 3, 20, "^[^\\u0000-\\u001f\\u007f-\\u009f]+$");
text!(DetailText, 0, 512, "^[^\\u0000-\\u001f\\u007f-\\u009f]*$");

pub type Tick = u32;
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
#[serde(transparent)]
pub struct Vec3(pub [Scalar; 3]);

macro_rules! wire_struct {
    ($name:ident { $($(#[$attr:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
        pub struct $name { $($(#[$attr])* pub $field: $ty),* }
    };
}
macro_rules! wire_enum {
    ($name:ident { $($variant:ident($ty:ty)),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS, JsonSchema)]
        #[serde(tag = "kind", content = "data", rename_all = "snake_case")]
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
    world_id: Option<OpaqueId>,
    session_id: Option<OpaqueId>,
    message: ClientMessage,
});
wire_struct!(ServerEnvelope {
    v: ProtocolVersion,
    world_id: Option<OpaqueId>,
    session_id: Option<OpaqueId>,
    message: ServerMessage,
});
wire_enum!(ClientMessage {
    ClientHello(ClientHello), ClientReady(ClientReady), InputBatch(InputBatch),
    Trigger(Trigger), Action(ActionRequest), Chat(ChatText), Ping(Ping), ResyncRequest(ResyncRequest), Leave(Empty),
});
wire_struct!(ClientHello {
    protocol_version: ProtocolVersion,
    build_id: BuildId,
    content_hash: ContentHash,
    sim_config_hash: ContentHash,
    join_ticket: OpaqueId,
    display_name: DisplayName,
    #[serde(default)]
    tshirt_colour: TshirtColour,
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
wire_struct!(Trigger {
    control_epoch: ControlEpoch,
    sequence: u32,
    held: bool,
    pressed: bool,
    weapon_id: WeaponId,
    aim_yaw: Yaw,
    aim_pitch: Pitch
});
wire_struct!(ActionRequest {
    request_id: RequestId,
    control_epoch: ControlEpoch,
    action: Action
});
wire_enum!(Action {
    EnterVehicle(EnterVehicle), ExitVehicle(Empty), Fire(Fire), Reload(Empty), EquipWeapon(EquipWeapon),
    AcceptMission(AcceptMission), CancelMission(Empty), Repair(Repair),
});
wire_struct!(EnterVehicle {
    vehicle_id: EntityId
});
string_enum!(WeaponId {
    Pistol,
    Rifle,
    MachineGun
});
impl WeaponId {
    pub fn index(&self) -> usize {
        match self {
            Self::Pistol => 0,
            Self::Rifle => 1,
            Self::MachineGun => 2,
        }
    }
}
wire_struct!(WeaponDefinition {
    id: WeaponId,
    capacity: u8,
    max_reserve: u16,
    fire_ticks: u32,
    reload_ticks: u32,
    equip_ticks: u32,
    damage: u16,
    range: f64,
    automatic: bool,
    spread_degrees: f64,
    moving_multiplier: f64,
    bloom_per_shot: f64,
    bloom_max: f64,
    bloom_recovery: f64
});
pub fn weapon_catalogue() -> [WeaponDefinition; 3] {
    [
        WeaponDefinition {
            id: WeaponId::Pistol,
            capacity: 12,
            max_reserve: 72,
            fire_ticks: 9,
            reload_ticks: 45,
            equip_ticks: 9,
            damage: 25,
            range: 80.0,
            automatic: false,
            spread_degrees: 0.6,
            moving_multiplier: 1.6,
            bloom_per_shot: 0.1,
            bloom_max: 0.6,
            bloom_recovery: 2.0,
        },
        WeaponDefinition {
            id: WeaponId::Rifle,
            capacity: 20,
            max_reserve: 80,
            fire_ticks: 12,
            reload_ticks: 60,
            equip_ticks: 12,
            damage: 40,
            range: 120.0,
            automatic: false,
            spread_degrees: 0.2,
            moving_multiplier: 2.5,
            bloom_per_shot: 0.05,
            bloom_max: 0.3,
            bloom_recovery: 1.5,
        },
        WeaponDefinition {
            id: WeaponId::MachineGun,
            capacity: 50,
            max_reserve: 150,
            fire_ticks: 3,
            reload_ticks: 90,
            equip_ticks: 15,
            damage: 15,
            range: 70.0,
            automatic: true,
            spread_degrees: 1.2,
            moving_multiplier: 1.8,
            bloom_per_shot: 0.25,
            bloom_max: 3.0,
            bloom_recovery: 2.0,
        },
    ]
}
wire_struct!(WeaponSlotView {
    weapon_id: WeaponId,
    owned: bool,
    magazine: Magazine,
    reserve: u16
});
wire_struct!(EquipmentView { weapon_id: WeaponId, reload_end_tick: Option<Tick>, equip_end_tick: Option<Tick>, aiming: bool, aim_pitch: Pitch });
wire_struct!(EquipWeapon {
    weapon_id: WeaponId
});
wire_struct!(Fire {
    weapon_id: WeaponId,
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
    Chat(ChatLine), ChatError(DetailText),
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
    #[serde(default)]
    tshirt_colour: TshirtColour,
});
wire_struct!(BaselineBegin {
    baseline_id: BaselineId,
    server_tick: Tick
});
wire_struct!(EntityShow {
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
    display_name: Option<DisplayName>,
    #[serde(default)]
    tshirt_colour: Option<TshirtColour>,
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
    #[serde(default)]
    #[ts(optional)]
    equipment: Option<EquipmentView>,
    motor: CharacterMotor, health: CharacterHealth, mode: CharacterMode,
    vehicle_id: Option<EntityId>,
});
wire_struct!(VehicleState {
    motor: VehicleMotor, health: VehicleHealth,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    variant_index: Option<u16>,
    target_entity_id: Option<EntityId>,
    target_position: Option<Vec3>,
});
string_enum!(MissionPhase {
    Active,
    Completed,
    Failed
});
wire_struct!(PlayerStatus {
    equipped_weapon: WeaponId, inventory: [WeaponSlotView; 3], equip_end_tick: Option<Tick>,
    revision: Revision, server_tick: Tick, credits: Credits, completed_jobs: u32,
    heat: Heat, pursuit: Pursuit, magazine: Magazine,
    repair: Option<RepairView>,
    reload_end_tick: Option<Tick>,
    mission: Option<MissionView>,
    respawn_tick: Option<Tick>,
    protected_until_tick: Option<Tick>,
});
string_enum!(RepairPhase {
    Dwell,
    DebitPending,
    RefundPending,
    Completed,
    Cancelled,
    Failed
});
wire_struct!(RepairView {
    vehicle_id: EntityId,
    zone_id: TemplateId,
    phase: RepairPhase,
    price: Credits,
    #[schemars(range(min = 0, max = 60))]
    dwell_ticks: u16,
    code: ActionCode
});
string_enum!(Pursuit {
    Clear,
    Pursuing,
    Searching,
    Arresting
});
wire_enum!(WorldEvent { Shot(Shot), Impact(Impact) });
wire_struct!(Shot {
    weapon_id: WeaponId, shot_sequence: u32,
    event_id: EventId, server_tick: Tick, shooter_id: EntityId, origin: Vec3, end: Vec3,
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

wire_struct!(ChatLine {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    system: Option<bool>,
    id: EventId,
    username: DisplayName,
    text: ChatText,
    server_tick: Tick,
});
