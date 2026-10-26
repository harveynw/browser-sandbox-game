# Protocol and serialization

[Documentation index](../README.md#documentation) · [Networking lifecycle](networking.md) · [WASM bridge](simulation.md#browser-wasm-boundary)

The game protocol is **version 3**. Rust owns the shapes in [`protocol/src/types.rs`](../crates/protocol/src/types.rs). `xtask` generates TypeScript declarations, JSON Schema metadata, shared limits and cross-language fixtures. The browser does not maintain a second handwritten DTO model or run a network schema-validation pipeline.

## Wire representation

Game messages use binary **MessagePack named maps**: Rust encodes with `rmp_serde::to_vec_named`; JavaScript uses `@msgpack/msgpack`. Envelopes carry `v`, nullable `world_id`, nullable `session_id`, and `message`. Message/action/world-event enums use `{kind, data}` with snake-case tags. Vectors are numeric tuples, IDs/ticks are bounded numeric scalar types, and opaque world/session/ticket values are strings.

```ts
{
  v: 3,
  world_id: "world-…",
  session_id: "session-…",
  message: { kind: "input_batch", data: { control_epoch: 1, frames: […] } }
}
```

The initial ServerHello supplies world scope without session scope. ClientHello echoes that world and compatibility metadata with null session; Welcome assigns the session. Later messages require the active world/session, phase and control epoch. Identifiers prevent stale-session confusion; authentication is the local guest cookie plus one-use join ticket.

**JSON has distinct purposes:** HTTP session/ticket/health/metrics bodies, WebRTC signalling, authored/baked content and schema metadata. Gameplay packets do not become JSON text in transit. Browser-to-local-WASM calls use a separate MessagePack byte contract with its own version and ownership; they never travel to the server.

## Codec boundary

| Frame | Maximum encoded bytes |
|---|---:|
| Client message | 4,096 |
| Client input batch | 1,100 |
| Reliable server message | 16,384 |
| Unreliable server message | 1,100 |

Rust directly deserializes with Serde, rejects empty/oversized/trailing frames, caps MessagePack decode depth at **16**, and applies scalar bounds and the **1–3 unique ascending input-ticks** invariant. Unknown fields, omitted Option fields and other library-supported representations follow Serde behavior. There is no custom structural scanner or duplicate handwritten schema validator. Server payload decoding is trusted by its consumers; server authority checks apply to incoming client intent.

The browser performs frame/version checks and library limits: strings **512 bytes**, maps **32 fields**, arrays **128 items**, binary/extensions disabled. It trusts decoded server DTOs after that boundary. These are explicit bounds, not a claim of a separate arbitrary-allocation guarantee. [Rust codec](../crates/protocol/src/codec.rs), [client codec](../packages/protocol/src/codec.ts) and [limits](../crates/protocol/src/limits.rs) define the exact contract.

Content validation and local bridge validation retain their own schema checks; trusting the game DTOs does not remove hash verification, simulation safety checks or server gameplay rules.

## Messages and channels

| Direction / channel | Messages and role |
|---|---|
| Client reliable WS | ClientHello, ClientReady, Action, ResyncRequest, Leave, reliable Ping |
| Client unordered RTC | InputBatch, RTC Ping |
| Server reliable WS | ServerHello/Reject/Welcome; BaselineBegin/EntityShow/BaselineEnd/StartPlay; EntityHide; ControlChanged; ActionResult; private PlayerStatus; authoritative WorldEvent; Disconnect; reliable Pong |
| Server unordered RTC | OwnerState, independent Snapshot records, RTC Pong |

The pinned server API does not expose incoming delivery metadata. The high nonce bit selects only a Ping reply channel; it never grants gameplay authority or proves the request's ingress channel.

InputFrame contains tick, signed axes **−32767…32767**, look yaw **−π…π**, pitch **−1.2…1.2**, and button bits: sprint **1**, jump edge **2**, handbrake **4**, aim **8**. The client generates 30 Hz input and sends the newest frame with up to two predecessors. Duplicate ticks cannot add simulation steps; missing ticks become neutral input.

Action carries an increasing request ID and current control epoch. Variants are enter/exit vehicle, fire, reload, accept/cancel mission and repair. The server validates phase, scope, ownership, state, relevance, geometry, cooldown and funds, retains at most 256 action results, replays matching duplicates and rejects conflicting/stale IDs. Clients do not retry an uncertain action under a new ID. Exact action/result/status/event fields are in the generated [TS declarations](../packages/protocol/src/generated/types.ts), sourced from Rust.

## Baselines and ownership

A reliable baseline freezes a relevant set and sends Begin → Shows → End. Client readiness confirms matching baseline ID, count, self descriptor and verified assets/WASM. StartPlay installs the authoritative actor. EntityShow establishes a visibility generation; EntityHide retires it. Snapshot records contain full independent entity state, per-record identity/visibility and source tick, with no dependency on another snapshot packet.

OwnerState provides the controlled actor and player state, server tick, control epoch and processed input position. Only a reliable ControlChanged can install a new ownership epoch. Private PlayerStatus includes committed economy, ammunition, heat/pursuit, mission and repair views; those fields are not published in another player's replica.

Wire shape or interpretation changes require a protocol version change and regenerated fixtures. Content and local bridge versions are independent. Run `npm run protocol:generate` explicitly after changing DTOs and `npm run protocol:check` to detect drift without overwriting generated output. Fixture support is test/generator data, not an extra runtime transport layer.
