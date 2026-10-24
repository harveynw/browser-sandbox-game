# 05 — Protocol and shared types

[Index](../README.md) · Timing: [Simulation and netcode](06-simulation-and-netcode.md)

This is the normative v3 wire contract. Implement the types in `crates/protocol`; generate TypeScript and schema metadata. Names in the TypeScript-shaped reference below are exact wire names. All structures use snake_case.

## Encoding and compatibility

- One binary transport message contains exactly one MessagePack value. No length prefix, compression, batching of unrelated envelopes, or stream decoder.
- Encode Rust structs as named maps with `rmp_serde::to_vec_named`. Check encoded packet size. Decode directly into Serde DTOs with frame/depth limits and reject trailing bytes. No JSON conversion, runtime schema validation or encode/decode self-check is used.
- Browser uses `@msgpack/msgpack` `encode`/`decode`. Do not use extension types, Date, BigInt, undefined, Map, Set, or class instances.
- Integers use ordinary MessagePack integer encoding and bounded Rust `u8/u16/u32/i16` types. Rust scalar deserializers reject nonfinite/out-of-range inputs. Float32 or representable float64 values are accepted for floating fields; tests allow f32 roundoff. The browser trusts server numeric values.
- Encoders serialize absent optional fields as `null`. Standard Serde decoding also accepts omitted `Option<T>` fields as None. Fixed-size vectors retain their Rust tuple lengths.
- Enums are adjacently tagged: `#[serde(tag = "kind", content = "data", rename_all = "snake_case")]`. Unit variants are represented by dedicated empty payload structs where needed, so all variants still have `data`.
- Every envelope has `v: 3`. Exact version match is required. Any wire-shape or interpretation change increments the version; alpha has no rolling compatibility.
- Rust rejects unknown enum variants and follows normal Serde defaults for fields: ignore unknown fields, accept omitted optional fields and library-supported positional struct representations. Canonical encoders always write named maps. The browser trusts the typed server payload after frame/library limits and a version check.

Prototype example:

```rust
#[derive(serde::Serialize, serde::Deserialize, ts_rs::TS, schemars::JsonSchema)]
pub struct InputFrame {
    pub tick: u32,
    pub move_x: i16,
    pub move_z: i16,
    pub look_yaw: f32,
    pub look_pitch: f32,
    pub buttons: u8,
}
// InputFrame has no client position and no variable dt.
```

Derive TS and schema metadata from the same Rust types. Generated TS is compile-time documentation; browser game packets are not runtime schema-validated. Content loading and the local WASM bridge retain their independently owned schema checks. The server owns phase, scope, epochs, ownership and gameplay validation. Do not maintain a second handwritten TS DTO definition.

## Envelope and primitives

```ts
type Envelope<T> = {
  v: 3;
  world_id: string | null;
  session_id: string | null;
  message: T;
};
type Vec3 = [number, number, number];
type EntityId = number;        // Rust u32; zero is invalid
type Tick = number;            // Rust u32; 30 ticks per second
type VisibilityId = number;    // Rust u32, per observer/entity show cycle
type ControlEpoch = number;    // Rust u32, increments on controlled-actor changes
```

The initial `ServerHello` envelope has `world_id` set and `session_id = null`. `ClientHello` echoes that world ID with null session. `Welcome` assigns a non-null session ID; all later messages require exact world/session values. A pre-welcome `Reject` uses null session. These identifiers prevent accidental stale-session use, not authentication by themselves.

Before any decode: cap C→S binary messages at 4 KiB; S→C reliable at 16 KiB; S→C unreliable at 1,100 bytes. C→S `InputBatch` also must fit 1,100 bytes. RTC sizing is a conservative application policy, not a claim about exact path MTU.

The Rust MessagePack decoder has a depth limit of 16 and rejects trailing data; Serde supplies type/tuple parsing and scalar bounds. The browser uses library limits of 512-byte strings, 32 map fields and 128 array items, with binary/extensions disabled. Input batches retain a 1–3 ascending unique tick check. There is no custom MessagePack scanner, total-node budget, special-key policy or separate allocation guarantee beyond the library behavior and frame/depth limits.

## Client → server messages

```ts
type ClientMessage =
  | { kind: "client_hello"; data: {
      protocol_version: number; build_id: string;
      content_hash: string; sim_config_hash: string;
      join_ticket: string; display_name: string;
    }}
  | { kind: "client_ready"; data: { baseline_id: number }}
  | { kind: "input_batch"; data: {
      control_epoch: ControlEpoch; frames: InputFrame[];
    }}
  | { kind: "action"; data: {
      request_id: number; control_epoch: ControlEpoch; action: Action;
    }}
  | { kind: "ping"; data: { nonce: number; client_time_ms: number }}
  | { kind: "resync_request"; data: { reason: "stale" | "overflow" | "invalid_state" }}
  | { kind: "leave"; data: Record<string, never> };

type InputFrame = {
  tick: Tick;
  move_x: number;             // i16 [-32767,32767]: strafe or steering-right
  move_z: number;             // i16 [-32767,32767]: forward or throttle
  look_yaw: number;           // radians [-pi,pi]
  look_pitch: number;         // radians [-1.2,1.2], positive looks up
  buttons: number;            // u8: 1 sprint, 2 jump edge, 4 handbrake, 8 aim
};
type Action =
  | { kind: "enter_vehicle"; data: { vehicle_id: EntityId }}
  | { kind: "exit_vehicle"; data: Record<string, never> }
  | { kind: "fire"; data: { aim_yaw: number; aim_pitch: number }}
  | { kind: "reload"; data: Record<string, never> }
  | { kind: "accept_mission"; data: { template_id: string }}
  | { kind: "cancel_mission"; data: Record<string, never> }
  | { kind: "repair"; data: { zone_id: string } };
```

`InputBatch.frames` contains the newest frame and up to two previous frames, ascending by tick, with unique ticks. Keyboard axes map to full range; normalize on-foot diagonal vectors after decoding. Jump is an edge bit tied to that tick. Reserved button bits must be zero.

Actions are reliable WebSocket requests. `request_id` increases from 1 within a session; retain the latest 256 results. Duplicate IDs with the same payload replay the cached result; duplicate IDs with different payload reject. An ID older than the cache floor rejects as `stale_request` and never executes. At most 32 actions may be awaiting results client-side. Do not retry a pending action with a new ID.

All actions require the current control epoch and `Playing` state. Validate target relevance, distance, cooldown, state, and affordability on the server. “Reliable” does not bypass validation. There is no client money/health/damage setter or teleport command.

## Server → client messages

```ts
type ServerMessage =
  | { kind: "server_hello"; data: {
      protocol_version: number; build_id: string; content_hash: string;
      sim_config_hash: string; tick_rate: 30; snapshot_rate: 10;
    }}
  | { kind: "reject"; data: { code: RejectCode; detail: string }}
  | { kind: "welcome"; data: {
      player_id: EntityId; server_tick: Tick; input_lead_ticks: number;
      profile: ProfileView;
    }}
  | { kind: "baseline_begin"; data: { baseline_id: number; server_tick: Tick }}
  | { kind: "entity_show"; data: {
      baseline_id: number | null; visibility_id: VisibilityId;
      descriptor: EntityDescriptor; state: EntityState; state_tick: Tick;
    }}
  | { kind: "baseline_end"; data: { baseline_id: number; entity_count: number }}
  | { kind: "start_play"; data: { baseline_id: number; owner: OwnerState }}
  | { kind: "entity_hide"; data: {
      entity_id: EntityId; visibility_id: VisibilityId; server_tick: Tick;
      reason: "out_of_range" | "destroyed" | "disconnected";
    }}
  | { kind: "snapshot"; data: {
      server_tick: Tick; records: StateRecord[];
    }}
  | { kind: "owner_state"; data: OwnerState }
  | { kind: "control_changed"; data: OwnerState }
  | { kind: "action_result"; data: {
      request_id: number; applied_tick: Tick; ok: boolean;
      code: ActionCode; affected_entity_id: EntityId | null;
    }}
  | { kind: "player_status"; data: PlayerStatus }
  | { kind: "world_event"; data: WorldEvent }
  | { kind: "pong"; data: {
      nonce: number; client_time_ms: number;
      server_tick: Tick; server_time_ms: number;
    }}
  | { kind: "disconnect"; data: { code: string; detail: string }};
```

`RejectCode`: `version_mismatch | content_mismatch | invalid_ticket | server_full | invalid_name | unavailable`.

`ActionCode`: `ok | not_ready | stale_epoch | stale_request | invalid_target | out_of_range | occupied | too_fast | exit_blocked | cooldown | no_ammo | dead | unavailable | insufficient_funds | already_active | not_active | pursuit_active`.

`ProfileView = { profile_id: string; display_name: string; credits: number; completed_jobs: number }`. Credits are `u32` capped at 1,000,000,000; completed jobs `u32`. Overflow rejects a transaction rather than wrapping.

## Entity and owner shapes

```ts
type EntityDescriptor = {
  id: EntityId;
  entity_kind: "player" | "vehicle" | "pedestrian" | "police";
  template_id: string;
  display_name: string | null;
};
type CharacterMotor = {
  position: Vec3; velocity: Vec3; yaw: number;
  grounded: boolean; jump_cooldown_ticks: number;
};
type VehicleMotor = {
  position: Vec3; yaw: number; speed: number;
  steering: number; velocity: Vec3;
};
type CharacterState = {
  motor: CharacterMotor; health: number;
  mode: "on_foot" | "driving" | "downed" | "arrested";
  vehicle_id: EntityId | null;
};
type VehicleState = {
  motor: VehicleMotor; health: number;
  driver_id: EntityId | null; disabled: boolean;
};
type EntityState =
  | { kind: "character"; data: CharacterState }
  | { kind: "vehicle"; data: VehicleState };
type StateRecord = {
  entity_id: EntityId; visibility_id: VisibilityId;
  state: EntityState;
};
type OwnerState = {
  server_tick: Tick; consumed_through_tick: Tick;
  control_epoch: ControlEpoch;
  player_id: EntityId; controlled_entity_id: EntityId;
  state: EntityState;
  player_state: CharacterState;
};
```

Police descriptor with a car template uses `VehicleState`; on-foot police use `CharacterState`. Templates define which kind is valid. A driving player’s player state follows the seat anchor and references the vehicle; the controlled state is the vehicle. An on-foot/downed player controls its player entity, with neutral movement when disabled.

`OwnerState` includes all evolving state necessary for motor replay. Cosmetic wheel rotation, camera springs, and animation clocks are not simulation state. When additional motor state is introduced, add it here, the WASM bridge, fixtures, and the protocol version together.

`consumed_through_tick` equals `server_tick`: all client input for ticks at or before this value is retired, whether applied or absent. It is not the last packet arrival time.

## Private status and events

```ts
type MissionView = {
  instance_id: string; template_id: string;
  phase: "active" | "completed" | "failed";
  checkpoint_index: number; deadline_tick: Tick;
  target_entity_id: EntityId | null; target_position: Vec3 | null;
};
type PlayerStatus = {
  revision: number; server_tick: Tick;
  credits: number; completed_jobs: number;
  repair: RepairView | null;
  heat: number; pursuit: "clear" | "pursuing" | "searching" | "arresting";
  magazine: number; reload_end_tick: Tick | null;
  mission: MissionView | null; respawn_tick: Tick | null;
  protected_until_tick: Tick | null;
};
type WorldEvent =
  | { kind: "shot"; data: {
      event_id: number; server_tick: Tick; shooter_id: EntityId;
      origin: Vec3; end: Vec3; hit_entity_id: EntityId | null;
    }}
  | { kind: "impact"; data: {
      event_id: number; server_tick: Tick; entity_id: EntityId;
      position: Vec3; severity: number;
    }};
```

Status is a complete private view, sent reliably on change and once per second as a repair heartbeat. Newer revision wins. Events are cosmetic; they never change authoritative health/cash locally. Shot events are reliable; impact events may be unreliable and dropped. Deduplicate the last 256 event IDs; ignore cosmetic events older than 1 second.

## Channel, ordering, and phase table

| Messages | Preferred channel | Valid phase | Requirement |
|---|---|---|---|
| ServerHello / ClientHello / Welcome / Reject | WS reliable | Authentication | 5 s hello deadline after transport open |
| BaselineBegin / EntityShow / BaselineEnd | WS reliable | Loading or resync | No more than 96 entities in baseline |
| ClientReady / StartPlay | WS reliable | Loading | Echo current baseline, all mandatory assets ready |
| InputBatch | RTC unreliable | Playing | ≤ 3 frames, 30 batches/s nominal, 60/s hard limit |
| Action / ActionResult | WS reliable | Playing | ≤ 10/s, burst 20; duplicate handling above |
| Snapshot / OwnerState | RTC unreliable | Playing | 10 Hz; each payload ≤ 1,100 bytes |
| ControlChanged | WS reliable | Playing | Increment epoch and reset prediction |
| EntityShow / EntityHide | WS reliable | Playing | Reliable visibility lifecycle |
| PlayerStatus | WS reliable | Playing | Change-driven, ≤ 10/s, plus 1 Hz refresh |
| Shot event / Impact event | WS / RTC respectively | Playing | Cosmetic only |
| Ping / Pong | WS reliable normally; RTC allowed for transport probes | Authenticated (diagnostic admission in T03) | 1 Hz per channel in T03; normal heartbeat ≤ 4/s |
| ResyncRequest | WS reliable | Playing | ≤ 1 per 5 s |
| Leave / Disconnect | WS reliable | Any established phase | Best effort before close |

The server API does not expose incoming channel identity. A valid Action accidentally arriving via RTC must still be idempotent and safe; lack of retransmission only affects whether it arrives. Preferred-channel enforcement is a client rule until the crate gains channel metadata.

## Join and resync sequence

```mermaid
sequenceDiagram
  participant B as Browser
  participant N as net transport
  participant S as Game server
  B->>N: WS + SDP/ICE + game channel open
  N->>S: Event::Open
  S->>B: ServerHello
  B->>S: ClientHello(ticket, hashes, name)
  S->>B: Welcome
  S->>B: BaselineBegin, EntityShow*, BaselineEnd
  B->>B: Load collision and spawn visuals; verify baseline count
  B->>S: ClientReady(baseline_id)
  S->>B: StartPlay(fresh OwnerState)
  B->>S: InputBatch for future ticks
  S->>B: OwnerState + Snapshot records
```

Build the baseline from one world view, send it in the same reliable FIFO, then buffer/append subsequent visibility changes on that FIFO. Loading peers do not receive unreliable state. StartPlay supplies fresh local state; after it, fresh snapshots repair remote movement since the baseline. A player is invulnerable/inert during loading and removed after 30 s if not ready.

Resync stops local controls, increments the control epoch, neutralizes the actor, resets the observer visibility set, and sends a new baseline followed by the same readiness handshake. Clear old replicas/tombstones at BaselineBegin, but keep recently visible visuals as inert presentation until replacement to avoid a flash. Packets for old visibility IDs/epochs are ignored.

## Snapshot rules

- Snapshots contain complete independent state records, not deltas. Split records into multiple ≤ 1,100-byte envelopes by actual encoded size; never split one record.
- A record’s tick is the enclosing `server_tick`; keep per-entity last applied tick. Do not reject all records in another packet merely because a packet at the same tick already arrived.
- Apply only to an existing matching `(entity_id, visibility_id)`. Unknown records are discarded; a later snapshot repairs them after EntityShow.
- Never infer despawn from an entity being absent from a snapshot.
- A new EntityShow carries a higher visibility ID and initial state. EntityHide retires that exact visibility ID. Maintain bounded tombstones for the session.
- OwnerState bypasses ordinary interest culling. A newer control epoch must be installed through reliable ControlChanged/StartPlay before applying its unreliable owner states. Cache at most one future-epoch owner state; otherwise discard.

## Contract acceptance

Maintain a compact representative interoperability corpus and focused malformed/version cases. Rust encodes → TS decodes; TS encodes → Rust deserializes. Compare semantic values, not encoder byte identity. Keep authority/lifecycle/gameplay invariants tested in their owning modules. Commit generated artifacts and fail CI on drift.

Test the initial full OwnerState and maximum-sized InputBatch against 1,100 bytes. If they exceed it, adjust the schema/version deliberately before shipping; do not rely on transport fragmentation.

## Current codec policy

The original T02/v2 implementation used custom preflight scanners, JSON intermediates, runtime schema validation, required-null/unknown-field enforcement and outgoing round-trip checks. The user explicitly approved replacing those layers on 24 October 2026. [Protocol simplification](34-protocol-simplification.md) records the v3 policy and verification. The default `codec` feature exposes direct MessagePack helpers; `default-features = false` remains available to shared simulation consumers. No jsonschema runtime dependency remains.

Scalar deserialization bounds retained on the Rust side:

| Field/category | Bound |
|---|---|
| World/session/profile/mission IDs, build IDs, tickets, template/zone/disconnect-code IDs | 1–64 ASCII characters from letters, digits, underscore, hyphen, dot, colon |
| Content/simulation hashes | Exactly 64 lowercase hexadecimal characters |
| Entity, visibility, control-epoch, baseline, request, event, revision IDs | Nonzero u32 |
| Tick, nonce, completed jobs | u32; no wrapping |
| Credits | 0–1,000,000,000 |
| Character / vehicle health | 0–100 / 0–160 |
| Heat / magazine / input lead | 0–3 / 0–12 / 2–6 |
| Baseline count / input frames | 0–96 / 1–3; snapshot population is bounded by server replication |
| Character jump cooldown / checkpoint index | u16 |
| Yaw, pitch, steering | ±f32(pi), ±f32(1.2), ±f32(0.55) |
| Impact severity | Normalized 0–1 |
| Millisecond clocks | Finite, nonnegative f64 up to JavaScript's maximum safe integer |
| Other scalar motor/vector values | Finite f32 range; world-specific checks remain authoritative |

Display names require 3–20 Unicode scalar values, no control characters, and canonical Unicode whitespace trimming. Detail text has at most 512 UTF-8 bytes and no control characters. These scalar constraints are enforced by Rust deserialization. There is no protocol-specific scanner for duplicate/reserved keys; standard decoder behavior applies.

Use integer markers for integer fields, as the ordinary JS and Rust encoders do. The old JSON whole-float normalization is removed; Serde decides which representations each Rust field accepts. Float endpoints use f32-rounded constants. Cross-language tests compare IDs/ticks exactly and allow float roundoff.

World ID must be present and non-null for every game envelope. Hello/Reject use null session IDs; Leave/Disconnect may use null before authentication. Other messages require a session. The authoritative server session enforces these scope/phase rules at application time; the protocol codec does not authenticate identity.

See the [package API guide](../packages/protocol/README.md) and [implementation log](implementation-log.md) for commands and measured fixture sizes. The initial v1 contract shipped with T02; T16 advances the wire to v2 for explicit repair status.

### T03 diagnostic clarification

Pong is valid on either delivery channel in both codecs and in the shared fixtures. No message shape changed. The localhost diagnostic sets nonce bit 31 for an RTC return path and clears it for WS; the rest is a per-connection counter. This is a temporary diagnostic convention, not a gameplay routing or authentication field. net-crate provides no ingress channel metadata. The browser sends on the selected channel, matches its nonce/time/channel against the response, and declares PASS only after receiving both paths. T07 replaces diagnostic admission with the real session state machine.

## T16 repair status and version 2

Version 2 adds the required nullable `PlayerStatus.repair`. Existing version-1 envelopes are rejected; both game build IDs advance to `t16-local`. Content and local WASM bridge schema versions remain independent and unchanged. `RepairView` contains `vehicle_id`, `zone_id`, `phase` (`dwell`, `debit_pending`, `refund_pending`, `completed`, `cancelled`, `failed`), `price` (exactly 50), `dwell_ticks` (0–60), and an `ActionCode` result in `code`. Active dwell progress is published at 5 Hz, with immediate phase transitions. Initial ActionResult acknowledges reservation; only Completed status and replicated health confirm committed repair. RefundPending explicitly identifies compensation still in flight.

For recovery missions, `checkpoint_index` is 0 for collection, 1 for garage delivery and 2 for a blocked safe exit. Time trials consume gate 0 at driving acceptance; subsequent indices identify the next ordered gate, with a distinct final return gate. MissionView remains private to its owner.


T17 retains network version 2 and advances matching game build IDs to `t17-local`. An active MissionView with null target_position means its objective has reached the server's reward-pending phase; no completion/credit increase is published until persistence commits. HTTP session preparation may return 202 while the worker loads/creates the profile; this JSON control-plane response is separate from MessagePack gameplay envelopes.
