# Architectural decisions

[Documentation index](../README.md#documentation)

| Decision | Reason / consequence |
|---|---|
| One localhost authoritative Rust world, sixteen players | A single owner decides gameplay; no distributed world or client-authoritative state |
| Pinned net-crate transport | Use the project's required WS/RTC implementation; Cargo.lock fixes its transitive WebRTC source |
| TypeScript/Three.js presentation with shared Rust/WASM motors | Browser rendering and UI remain flexible without duplicating physics |
| Named-map MessagePack for gameplay | Explicit field names, direct Serde/JS-library interoperability and cross-language fixtures |
| Rust-derived TS and schema metadata | One owner of DTO shapes; network schemas are metadata, while content/local bridge retain schema validation |
| Small game codec boundary | Library parsing plus frame/depth/scalar/input invariants; scope, phase, epochs and game rules remain server responsibilities |
| Independent complete snapshot records | Packet loss does not invalidate subsequent deltas; visibility generations and per-record ticks reject stale data |
| Complete collision preload, streamed visuals | Movement correctness never depends on a graphics download or chunk retention |
| Fixed 30 Hz input/simulation, 10 Hz replication | Rendering frequency is independent; replay remains bounded |
| Upright arcade vehicles | Share predictable collision/movement on the authored flat map without a suspension/drivetrain simulator |
| No PvP damage or rewind shooting | Crime, NPC/car damage and local cooperation do not require competitive lag compensation |
| SQLite profiles, not persistent world state | Retain committed progress while restarting a fresh city; transactional economy and recoverable repairs have a clear boundary |
| Original procedural art and synthesized effects | No imported asset pipeline or external media downloads are required |
| Owned local process supervision | Build/start/probe/stop are explicit, with no implicit global installations or automatic takeover of occupied ports |
| Measured limits remain explicit | Functional localhost verification does not certify impairment, reference hardware, memory soak or the full browser matrix |

## Dependency source of truth

[Workspace Cargo manifest](../Cargo.toml), [Cargo.lock](../Cargo.lock), [npm manifest](../package.json) and [package-lock.json](../package-lock.json) define installed versions. Rust's toolchain and Node recommendations are repository files, not mutable web documentation. wasm-bindgen CLI must match the Rust crate's ABI version.

The pinned transport source is [net-crate a255164](https://github.com/harveynw/net-crate/tree/a25516439f95d421d3dc752be1cbded8150c8805). Relevant boundaries are its [server handle](https://github.com/harveynw/net-crate/blob/a25516439f95d421d3dc752be1cbded8150c8805/src/server/mod.rs), [connection actor](https://github.com/harveynw/net-crate/blob/a25516439f95d421d3dc752be1cbded8150c8805/src/server/connection.rs), [RTC channel](https://github.com/harveynw/net-crate/blob/a25516439f95d421d3dc752be1cbded8150c8805/src/server/webrtc/mod.rs) and [event queue](https://github.com/harveynw/net-crate/blob/a25516439f95d421d3dc752be1cbded8150c8805/src/queue.rs). Our adapter's bounded state does not change dependency-internal buffering or add generation tags to already emitted numeric events.

## Compatibility boundaries

Network version 3, content schema/canonical-hash version 1, local bridge version 1 and SQLite migration 1 are independent contracts. Review changes against their actual consumers and regenerate only the owning outputs. Stable asset IDs and frozen simulation fixtures remain compatibility/test data.

Supported deployment is loopback. Public/LAN operation, TLS, NAT traversal, TURN/STUN, public authentication, pre-auth abuse resistance and public transport hardening require separate deployment work. Existing host binding, local cookie/ticket assumptions and dependency queue/ID limitations must not be advertised as an internet-ready service. No cloud infrastructure is configured.
