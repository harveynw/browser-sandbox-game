# Implementation log

Latest checkpoint: **T12 complete, 21 October 2026**. The user requested a stop/check-in after each major chunk. T13 has not been started. Historical entries below retain the scope and measurements of their original checkpoints.

## Task status

| Task | Status |
|---|---|
| T01 — Workspace and commands | Complete |
| T02 — Protocol/code generation | Complete |
| T03 — Real transport spike | Complete |
| T04 — Greybox content package | Complete |
| T05 — Native character motor | Complete |
| T06 — WASM prediction bridge | Complete |
| T07 — Authoritative sessions/world | Complete |
| T08 — Browser scene and controls | Complete |
| T09 — Prediction/replication | Complete |
| T10 — Cars and possession | Complete |
| T11 — Courier vertical slice | Complete |
| T12 — Full city and content kit | Complete |
| T13 — Traffic and pedestrians | Complete |
| T14 — Pistol and damage | Complete |
| T15 — Police and heat | Not started |
| T16 — Remaining jobs and repair | Not started |
| T17 — Local profile persistence | Not started |
| T18 — Presentation and UX | Not started |
| T19 — Impairment and lifecycle suite | Not started |
| T20 — Capacity and performance | Not started |
| T21 — One-command local launch | Not started |
| T22 — Final acceptance and handoff | Not started |

## T01 — Workspace and commands

Status: **complete**.

Prerequisites verified: the repository contained the design documents and no implementation; no applicable `AGENTS.md` was found. Existing design content was preserved except status and usage updates.

Files/contracts changed:

- Cargo workspace with six minimal crates: protocol, content, sim-core, sim-wasm, server, and xtask.
- Fixed simulation-frequency constants shared by native/WASM; one diagnostic WASM export. No movement code yet.
- npm workspaces for client and shared protocol package. The protocol package has no placeholder DTOs.
- Strict TypeScript, ESLint, Prettier, rustfmt, and Clippy configuration; explicit toolchain pins and lockfiles.
- Vite browser entry point displaying scaffold status and the bundled Three.js revision. It does not create a renderer or connect to a server.
- Build/check/dev commands; unfinished generation/bake commands return failure with their owning task ID.
- Ignore rules for local state/build output; README startup instructions and plan status.

Tested toolchain and dependencies:

| Tool | Version |
|---|---|
| Rust / Cargo | 1.94.0 / 1.94.0 |
| Rust target | `wasm32-unknown-unknown` |
| Node / npm | 24.19.0 / 10.9.2 |
| TypeScript | 6.0.3 |
| Vite / Vitest | 8.3.4 / 5.0.5 |
| ESLint / typescript-eslint | 10.13.0 / 8.72.0 |
| Prettier | 3.9.10 |
| Three.js / types | 0.186.1 / 0.186.0 |
| wasm-bindgen | 0.2.129 |

Tool selection: TypeScript 7 was available, but typescript-eslint 8.72.0 supports TypeScript `<6.1.0`; 6.0.3 was selected to keep the tools compatible. The existing system Node 23 was not used for verification; Node 24 was selected without changing the global Node default. Rust 1.94.0 and its components/target were installed alongside the existing stable toolchain without changing its default.

Commands and results:

- `npm install` — pass; generated `package-lock.json`, installed the workspace dependencies, registry audit reported zero vulnerabilities at installation time.
- `cargo generate-lockfile` and `cargo fetch --locked` — pass; generated `Cargo.lock` and fetched its dependencies.
- `npm run format` — pass.
- `npm run check` — pass: all three TS projects, lint with zero warnings, formatting, Clippy with warnings denied, and 3 Rust `xtask` tests. Vitest launched successfully and explicitly reported zero test files; this is harness verification, not frontend test coverage.
- `cargo test --workspace --locked --offline` — pass; 3 tests, no gameplay tests exist yet.
- `npm run build` — pass: native workspace, WASM target, and Vite production build.
- CLI subprocess checks — `generate`, `check-generated`, and `bake-content` each returned exit 1 with a not-implemented reason; unknown command returned 2; help returned 0. The server scaffold returned 0 and explicitly reported no listeners.
- Node `WebAssembly.compile` of the raw artifact — pass; the expected diagnostic function exists with wasm-bindgen's hashed raw export name. An initial assertion assumed an unhashed name and failed; inspecting exports established the raw format and the corrected artifact check passed. No WASM runtime behavior was claimed.

Manual scenario evidence: started `npm run dev`, opened `http://localhost:5173/` in the in-app browser, and verified the visible heading, T01 status, absence-of-gameplay notice, and Three.js r186 label. The initial sandboxed server launch was denied permission to listen; the approved localhost launch succeeded.

Build outputs: frontend under `apps/client/dist/`; raw WASM under `target/wasm32-unknown-unknown/debug/sim_wasm.wasm`. Both are ignored build artifacts. Diagnostic frontend build size was 0.47 kB HTML, 0.59 kB CSS, and 1.24 kB JS before compression; these are not game-performance measurements.

Known limitations:

- No game server listeners, actual networking, wire DTOs/codecs, collision, city, or gameplay.
- `net` is pinned as a workspace dependency but is not linked/downloaded by a server crate until T03.
- WASM compiles and validates; JavaScript binding generation and browser execution are deferred to T06. `wasm-pack` is not installed for this chunk.
- `npm run build` currently skips unfinished generation/content stages. `npm run check` skips unfinished generated-drift checks. T02/T04 must wire these into the pipeline.
- The Vitest zero-test allowance is temporary; remove it when T02 introduces protocol tests.
- `dev:all`, `setup`, `preview:local`, persistence, and API proxying belong to later tasks; only frontend dev/preview commands exist now.

Next action: **check in with the user**. If they choose to continue, T02 implements the complete shared protocol, generated TypeScript/JSON Schema, codecs, bounded validation, and cross-language fixtures.


## T02 — Shared protocol, validation, generation, and fixtures

Status: **complete**. T01 prerequisites were reviewed and remained intact. Historical T01 limitations above describe that earlier checkpoint; this entry supersedes its protocol/test-pipeline limitations.

Implemented:

- Every v1 client/server message, action, state, event, status, and string-enum variant in Rust. Numeric/string scalar wrappers supply shared constraints; nullable fields require explicit presence.
- Generated TypeScript, Schema 2020-12 client/server contracts, limits, and fixture manifest. The generator requires all declared properties while preserving null unions.
- Native and browser MessagePack codecs with bounded structural preflight: frame bytes, depth, total nodes, declared lengths, valid UTF-8, safe integers, finite floats, duplicate/reserved map keys, no binary/extensions, and no trailing values.
- Schema validation on both sides, preventing positional arrays from being accepted as structs. Rust uses a cached native validator with HTTP/file resolution disabled. Rust outbound serialization uses a bounded writer in named-map mode.
- Stateless state/coherence checks and pure phase/session/epoch/baseline/input-window admission helpers. Gameplay, authentication, action deduplication, rate limits, and ownership lookup remain later tasks.
- Deterministic generation plus read-only drift checks, integrated into the root build/check pipeline. The T01 generation stubs were replaced; content baking still fails explicitly for T04.
- A shared corpus with **72 positive and 64 negative fixtures**, including every variant and boundary-size messages. TS decodes Rust bytes and sends TS-generated bytes through the real Rust decoder; IDs/ticks are compared exactly and floats with tolerance.
- The browser diagnostic now imports the shared package and verifies an actual local MessagePack Ping round trip. This starts no network connection.

Pinned dependencies added: Serde 1.0.229, serde_json 1.0.151, rmp-serde 1.3.1, ts-rs 12.0.1, Schemars 1.2.3, jsonschema 0.59.0 (native optional codec, default features disabled), @msgpack/msgpack 3.1.3, and Ajv 8.20.0. Cargo/npm lockfiles were updated.

Verification:

- `npm run check` — pass: generated-file comparison, TS, ESLint, Prettier/rustfmt, **143 Vitest tests**, Clippy with warnings denied, and **10 Rust tests** (9 protocol, 1 generation drift).
- `npm run build` — pass: generation, TS, native workspace, raw sim WASM, production frontend. Frontend JS is 180.54 kB / 50.35 kB gzip at this checkpoint; this measures the diagnostic bundle, not game performance.
- `cargo check -p protocol --no-default-features --target wasm32-unknown-unknown --locked --offline` — pass; the shared DTO/pure-validation surface does not require the native schema validator.
- CLI drift check — intentionally changed the generated types file, observed exit 1 and a drift reason, verified it was not overwritten, then restored the exact original bytes. The normal check passed afterward. The Rust drift test also detects extra files in a temporary generated tree.
- Real browser at localhost:5173 — visible T02 status and a successful **108-byte local Ping** diagnostic. Networking and gameplay remain explicitly labeled unimplemented.

Packet measurements from boundary fixtures:

| Rust-encoded case | Bytes | Budget |
|---|---:|---:|
| Three-frame InputBatch with maximum scope IDs/numeric endpoints | 425 | 1,100 |
| Driving OwnerState with maximum scope IDs/numeric endpoints | 593 | 1,100 |

The corresponding JS float64 encodings also passed the 1,100-byte budget test. A forced-float encoding of integer fields round-trips consistently through both decoders. These are representative maximum-field fixtures, not a measured gameplay bandwidth claim.

Design clarifications: document 05 now records ID/string bounds, finite scalar ranges, required-null handling, integer-marker semantics, and state consistency. This was the first implementation of the v1 draft; there was no deployed earlier wire version. A native schema check was added because Serde can otherwise accept positional arrays for structs. The shared-WASM types-only feature keeps that native validator out of simulation consumers.

No remaining T02 acceptance failures. No transport, world, movement, AI, or persistence implementation has been claimed.

## Progress review and next decision

**Completed foundations: T01 and T02.** This is a tested build/serialization foundation, not a playable game. The highest unverified integration is the actual net-crate browser connection, including WebRTC negotiation and connection lifecycle. The server still starts no listeners.

**Next chunk: T03 — actual transport spike.** Keep it bounded to a loopback Rust listener and a two-browser-context diagnostic: server-first hello, binary MessagePack round trips through both reliable WebSocket and unordered WebRTC, visible connection/RTT status, negotiation timeout, pre-open payload discard, bounded application queues, and reconnect/cleanup tests. Diagnostic handshake/profile placeholders must be clearly labeled; full game admission/auth/world allocation belongs to T07. Stop for user review when real two-channel communication is demonstrated.

After that, follow T04 (greybox content), T05 (native motor), and T06 (WASM movement). Building map/physics first would postpone the most important untested networking dependency. Public hosting remains outside scope. T03 has not been started during this review.

## T03 — Actual localhost transport

Status: **complete for the local diagnostic checkpoint**. T04 has not been started. The progress review above is historical; the user subsequently authorized T03.

Implemented the pinned net-crate server dependency and Cargo lock resolution (including its pinned WebRTC dependency). A dedicated Rust pump owns all crate handle calls. The application boundary caps peers, lifecycle/ingress/reliable queues and replaces stale diagnostic RTC output. It drops pre-open bytes, tags application connections with increasing generations, rejects stale keys, and performs idempotent close admission. The browser ports the pinned signalling format, serializes SDP/ICE processing, accepts only the server-created unordered `game` channel with zero retransmissions, separates binary application data from JSON signalling, enforces send pressure limits and times out negotiation after 15 seconds.

A temporary loopback handshake sends ServerHello before ClientHello, then a clearly labeled diagnostic Welcome. World/session scope is checked. Only hello/ping/leave are supported; unavailable operations close and clean up the diagnostic session. No actual player/entity/profile is allocated. Diagnostic build/content placeholders and a fixed local ticket are not authentication and must be replaced by T07 admission.

Pong delivery now allows RTC in the native and browser validators; a shared RTC Pong fixture was added. The resulting corpus is 73 positive and 64 negative cases. Nonce bit 31 requests the diagnostic return channel because net-crate does not expose ingress channel metadata. The browser knows where it sent a packet and verifies the matching channel, nonce and timestamp on receipt. This convention is not a gameplay routing feature.

Verification on 20 October 2026:

| Check | Evidence |
|---|---|
| Real browser ↔ Rust loopback | Both WS and RTC returned binary MessagePack; final page showed PASS with at least 10 replies per path, around 8–12 ms locally |
| Two concurrent browser pages | Both pages showed PASS independently; server logged active=2 and distinct session generations |
| Leave/reconnect and recycled IDs | Server observed peer ID 1 at generations 4 then 5; another page continued to pass |
| Server-originated kill/reconnect | Test server close changed the second page to Closed: WebSocket closed; first page continued passing; reconnect restored PASS on both paths |
| Cleanup | HMR and explicit disconnect removed prior sessions; server logged active=0 between test runs |
| Unit boundaries | 6 browser transport tests and 5 Rust transport tests: timeout, double close, unsubscribe/cleanup, signal ordering, binary type/channel rejection, buffered pressure, bounded queues, coalescing, pre-open discard and stale generations |
| Full checks | `npm run check` passed: generated drift, strict TS, lint/format, 150 Vitest tests, Clippy warnings denied and 15 Rust tests |
| Build | `npm run build` passed native workspace, raw WASM and production frontend; diagnostic JS 191.13 kB / 53.64 kB gzip |

Live tests establish the local two-channel gate. Failed ICE/timeouts and pressure/full-queue conditions were exercised deterministically at our boundaries, not by exhausting dependency internals or disrupting a physical network. Selected candidate-pair statistics were not collected; public/cross-network topology remains unverified. The browser serves as the current end-to-end readiness probe; HTTP readiness and service supervision remain T07 work. The server deliberately prints RTC readiness UNPROVEN at startup rather than claiming a constructor means ready.

Known limits are retained in document 04 and the [diagnostic guide](17-transport-diagnostic.md): opaque internal queues/panics, potential internal stale-ID events, no dependency shutdown/join/health API, and an asynchronous listener bind race despite our synchronous preflight. The latest-unreliable slot is diagnostic-only; per-entity replication coalescing is required for T09. No public deployment safety or playable-world claim is made.

**Next proposed chunk: T04 — greybox content package.** Bake the slice map, collision, content hashes, anchors and primitive assets from the existing world plan. Stop here for user review before starting it.

## T04 — Greybox slice content and inspector

Status: **complete**. T05 has not been started. The user authorized this chunk after the T03 checkpoint.

Implemented Rust-owned content DTOs, JSON Schema/TypeScript generation, source validation, deterministic baking, canonical hashing, immutable package publication and read-only drift detection. Source lives in `content/source/slice.json`; generated contracts live in `packages/content/src/generated`. The package includes collision, sim config, debug overlays, four primitive chunks, directed vehicle navigation, separate sidewalk/crossing navigation, six access spurs, 14 reservations, original landmark primitives, a compact visual/template and the courier template. Asset provenance is recorded under `content/`.

The standalone browser inspector at `/map` verifies index, manifest and asset integrity before rendering. It offers orbit/pan/zoom, top/isometric presets, collision/reservation/graph/route overlays and labels, with controls that collapse in a narrow panel. The whole slice fits initially. Visual/collision geometry shares its bake source; colored wireframes permit comparison. The default `/` transport page remains separate and lazy loaded. No game server or WebSocket/RTC session is used by the map inspector.

Deliberate design clarification: slice perimeter road centerlines shift 10 m inward to X `[-246,-128,-10]`, Z `[10,128,246]`, preserving complete roads/sidewalks inside temporary barriers. Named gameplay anchors are unchanged. The original full-city grid remains the T12 target. Content canonical JSON v1 restricts numbers to a six-decimal grid within ±1,000,000 and ASCII object keys; this makes Rust/JS hashes reproducible without claiming general RFC 8785 support. These decisions and exact APIs are documented in [18 — Content pipeline](18-content-pipeline.md).

Verification on 20 October 2026:

- Two separate `npm run content:bake` invocations produced identical hashes/files; the Rust determinism test also compares every output byte. Content hash: `27042774d0fe25334635b79da0bde579b57fcc9a3ab34748d1ce2f83b9179e38`. Sim-config hash: `2107d4412daf7286e6ef515aec6453991a587cde18923e87c52dec8a34d4808c`.
- `npm run check` passed: protocol/content drift checks, strict TypeScript, ESLint, formatting, **157 Vitest tests**, Clippy warnings denied and **24 Rust tests**. New tests cover native bake determinism, separate hash coverage, invalid overlapping spawn, disconnected/dangling navigation, references/driveways/corridors, invalid numbers, duplicate colliders, unsafe paths, altered assets, and mismatched visual faces. Browser content tests consume the actual Rust artifacts, verify canonical hashes/schemas/routes and enforce bounded concurrency/cancellation. Drift tests verify rejection without overwriting edited files.
- `npm run build` passed native workspace, raw WASM and production frontend. The inspector chunk is approximately **693 kB / 176 kB gzip**, including Three.js and content schemas. Vite reports its >500 kB chunk warning; this is recorded rather than hidden. It is lazy loaded separately from the transport page. This is no gameplay frame-rate/performance claim.
- Live browser: **Verified · 4 chunks · 35 colliders**. Inspected isometric and top views, toggled reservations and both navigation graphs, and checked the courier route overlay. Corrected distant surface flicker using camera depth/draw offsets, fitted the full map to the existing narrow viewport, and hid overlapping labels. No independent wide-window override or movement/drive-through test was performed.

Current measurements:

| Item | Result |
|---|---:|
| Slice | 256 × 256 m, four 128 m chunks |
| Static collision | 35 boxes: 27 buildings, 3 landmarks, 4 boundary walls, 1 ground |
| Primitive chunk records | 266 |
| Vehicle graph | 52 nodes / 92 directed edges |
| Pedestrian graph | 52 nodes / 162 directed edges |
| Package files, excluding outer index | 70,455 bytes |
| Outer package index | 292 bytes |
| Complete collision JSON | 3,772 bytes (below 2 MiB target) |

Limits: all T04 static colliders and parking are axis-aligned; nonzero collider/parking yaw is rejected. Primitive boxes are intentionally rough assets. Clearance tests use conservative box bounds; actual capsule/cast behavior starts in T05. Runtime native filesystem loading/readiness integration is T07; visual streaming and the full-city package are T12. T03 continues using diagnostic compatibility placeholders until real session admission is implemented. There is no walking, driving, job execution or persistence yet.

**Next proposed chunk: T05 — native character motor and collision queries.** Consume the baked collision/config in Rust, implement the fixed 30 Hz walking/jump/move-and-slide behavior and recorded trace tests. Stop here for user review before beginning it.

## T05 — Native character motor and collision queries

Status: **complete**. T06 has not been started. The user authorized this native-only checkpoint after reviewing T04.

Implemented `sim-core` character/config/collision/trace modules. It consumes the baked collision and simulation DTOs, converts settings to validated f32 constants, builds an immutable static Rapier BVH once, and exposes capsule movement/overlap plus filtered raycasts. Rapier 0.36.1 / Parry 0.31.1 are pinned in Cargo.lock; enhanced determinism is enabled without parallel/explicit SIMD features. A normal dependency-tree inspection shows no net-crate, Tokio or native JSON Schema validator dependency in sim-core.

The fixed 30 Hz motor uses existing protocol CharacterMotor/InputFrame types, normalized camera-relative movement, forward sprint with backward/strafe limits, acceleration/braking, air control, jump-edge/cooldown/gravity/fall cap, grounded/ceiling handling, low steps, ground snap and bounded shortest-arc facing. All motor state remains in the DTO; there is no hidden previous-input state. Session/tick deduplication remains the caller's responsibility. Queries return authored static IDs, not engine handles. Invalid state/input/config/geometry fails without mutating caller state.

Native testing found exact-contact/diagonal-floor cast instability and a grazing capsule/step-corner endpoint error. The shared adapter now uses separately swept movement phases, checked support-gap restoration and a strict endpoint overlap guard with bounded clipping. It retains the original overlap tolerance rather than accepting penetration. `endpoint_clamped` records that conservative correction; a dedicated regression test preserves seed 17/tick 205. These policies and the fallible implemented API are explained in [19 — Native character simulation](19-native-character-simulation.md).

Verification on 20 October 2026:

- **16 native simulation tests pass**, covering walk/sprint/backward/strafe/diagonal limits, fixed acceleration and neutral braking, jump/cooldown/landing/fall cap, weaker air control, ceilings, low/tall steps, snap policy, wall/corner sliding, camera/yaw/aim conventions, query filters/ray IDs, invalid inputs/config/colliders, the real baked spawn and the grazing-corner regression.
- Four deterministic seeds × 900 ticks (**3,600 stress steps**) remain finite and capsule-clear, with horizontal speed ≤7.01 m/s and at most 16 output contacts.
- Saved `tests/fixtures/simulation/character-trace.json`: **1,800 ticks / 60 seconds**, seven run-length input segments, complete initial motor/config/collision and **13 frozen complete motor checkpoints**. Per-tick replay checks capsule clearance. Repeating native replay compares identical motor values. Perturbing a saved checkpoint is rejected.
- `npm run sim:trace` — PASS, 13 checkpoints, **0.000000 m measured maximum native position error** against the saved fixture. The native verification threshold is 0.0001 m; this is not a native/WASM measurement.
- `npm run check` — PASS with **157 Vitest tests and 40 Rust tests**, generated protocol/content checks, strict TS, lint/format and Clippy warnings denied.
- `cargo check -p sim-core --target wasm32-unknown-unknown --locked` — PASS. `npm run build` also passed native workspace, raw WASM and production frontend. Browser bridge initialization/runtime agreement has not been tested or claimed.
- T04 package hashes remain unchanged. No map layout, protocol DTO or browser game-control change was needed.

A runnable native demonstration is `npm run sim:trace`. The candidate recorder example prints regenerated checkpoints to stdout and never overwrites the accepted fixture; checkpoint updates require review. No networked actor, WASM bridge, browser walking, vehicle motor, dynamic car proxy, collision damage, job or persistence implementation was added. The existing browser remains the map inspector and transport diagnostic. The pre-existing Vite inspector bundle-size warning remains unchanged; there is no new frame-rate/capacity claim.

**Next proposed chunk: T06 — WASM bridge and browser/native trace agreement.** Expose collision loading/reset/step/state through sim-wasm, generate/package the bridge, and run this trace in a real browser within 1 cm of native. Stop here for user review before beginning it. Actual browser gameplay controls/camera remain T08.

## T06 — WASM bridge and browser/native agreement

Status: **complete**. T07 has not been started. The user authorized the next stage after the T05 checkpoint and clarification that browser gameplay controls arrive in T08.

Implemented a local, versioned MessagePack adapter in sim-wasm, generated bridge-only TS/types and schemas, async TypeScript initialization and allocation ownership, and reproducible release web packaging with wasm-bindgen-cli 0.2.129. The CLI version matches the Rust ABI exactly; wasm-pack is not required. Root build/dev/check scripts build the real JS/WASM package before consumers need it. The generated package stays ignored; generated bridge contracts remain reviewable source artifacts.

The bridge loads validated collision/config once, builds one static BVH, restores complete character motors, steps exactly one tick, queries current state and vision-filtered static rays, and frees allocations. Invalid calls fail without partial world/actor commits. Step checks contiguous ticks and restored control epoch/entity; these checks do not replace future server admission. Empty vehicle-proxy sets are supported as a placeholder; valid non-empty sets explicitly fail until T10. There is no hand-ported TS movement or per-step static rebuild.

Design refinement recorded in documents 08 and [20 — WASM bridge](20-wasm-bridge.md): the actual T06 PredictedFrame carries the existing CharacterMotor, version, tick and control identity instead of inventing a whole gameplay EntityState. It is a separate local DTO namespace; wire envelopes and protocol version are unchanged. The static camera query is currently a ray; the planned camera-radius sweep remains T08.

The `/simulation` page verifies the frozen T05 fixture hashes, loads one world, and replays all 1,800 ticks through the public TS/MessagePack/WASM step path. It compares 13 complete motor checkpoints, requires ≤0.01 position/velocity error and ≤0.001 yaw error, checks exact grounded/cooldown, tests a static wall ray, and frees in finally. It exposes retry after failure and prevents simultaneous runs; navigation cancels at bounded yields. No server is needed.

Verification on 20 October 2026:

- `npm run check` — PASS: bridge/protocol/content drift checks, release WASM build, strict TS, ESLint/formatting, **163 TS tests**, Clippy warnings denied and **41 Rust tests**.
- Six new TS tests cover actual release WASM trace agreement, reset/load lifecycle, duplicate/skipped ticks, invalid schema/buffers, stale epoch/entity, failed reload/reset retaining state, static rays, explicitly unsupported non-empty proxies, free/use-after-free and asynchronous load failure/retry. The native byte adapter also matches frozen native checkpoints exactly.
- Live browser at `localhost:5173/simulation` — **PASS · 1800 ticks · 13 checkpoints**. Maximum reported position error **0.000002 m**, velocity error **0.000000 m/s**; static ray PASS. Mean serialized bridge + motor around **0.036 ms/tick** locally.
- `npm run build` — PASS, native workspace and packaged WASM/frontend. Live **production preview at localhost:4173/simulation** independently passes the same trace, with maximum position error **0.000002 m** and mean **0.035 ms/tick**. Screenshot saved at `/private/tmp/port-meridian-t06.jpg`. These are local diagnostic measurements, not multiplayer/frame-rate/capacity claims.
- WASM production asset approximately **1,265 kB raw / 358 kB gzip**, loaded on the simulation route. The existing lazy map-inspector >500 kB warning remains visible. Static content/config hashes and T05 accepted trace are unchanged.

Local tooling needed installation of the pinned wasm-bindgen CLI and declaration of direct client MessagePack/AJV dependencies. Both are reproducible from the documented pinned commands. The Vite dev server remains available on localhost:5173. No networked actor, browser movement controls, prediction reconciliation, dynamic proxies or vehicles were added.

**Next proposed chunk: T07 — authoritative sessions and world.** Implement real local admission, actor ownership/lifecycle, fixed server ticks, input admission and character motion using sim-core. Stop here for user review before beginning it. T08 remains the first third-person scene with keyboard/mouse controls.

## T07 — Authoritative localhost sessions and character world

Status: **complete**. T08 has not been started. The user authorized this chunk after the T06 checkpoint.

Implemented server modules for validated runtime content loading, bounded memory guest profiles/tickets, localhost HTTP control, a fixed-step scheduler and an ordered authoritative world/session store. The server loads the actual hash-addressed package, verifies every referenced asset plus package semantic checks, builds collision once, and capsule-checks 16 spawn slots before serving. Monotonic player IDs, opaque profile/session/world IDs and generation-tagged transport peers are distinct.

Guest admission now uses an HttpOnly cookie and 256-bit one-use, 30-second tickets. Valid hello allocates an inert Loading actor and emits Welcome plus a frozen reliable baseline. Matching ClientReady emits StartPlay and the initial private PlayerStatus; only then does the actor simulate. One active session per guest is enforced by cleanup and replacement. Disconnect, timeout, output failure, Leave and delayed old-generation events cannot retain ownership or input buffers.

The 30 Hz scheduler caps catch-up at five fixed steps and discards excess wall time without skipping tick IDs or changing DT. Authoritative input admission retains up to nine future ticks, ignores identical redundancy, rejects conflicts/further-future/old-epoch frames, discards late frames and selects exactly one frame or neutral for each simulation tick. Shared sim-core owns all movement. Owner states retire all input through the current server tick and are emitted at 10 Hz. Resync increments epoch/baseline, uses fresh visibility IDs, neutralizes velocity and freezes until new readiness. Gameplay actions are explicitly unavailable, with bounded duplicate result caching.

The browser root diagnostic now verifies content/WASM, requests a real guest/ticket, validates baseline count and ownership, acknowledges readiness and displays authoritative owner positions. A bounded 30-input movement probe tests server authority without adding T08 controls. Both dev and production preview proxy `/api` and `/health` to loopback port 3001. `/map` and `/simulation` remain server-independent.

Health readiness is deliberately an **explicit localhost development probe**: the browser reports actual matching WS and RTC pong receipt through its active guest/session endpoint, refreshing a five-second lease. Tick/pump health and overload are also checked. Bootstrap tickets remain available before readiness so the probe can connect. This trusted local report is not public authentication or unattended monitoring; document 07 is refined accordingly in [21 — Authoritative localhost sessions](21-authoritative-sessions.md). Memory profile lookup is synchronous and bounded; asynchronous disk persistence remains later work.

Live replacement testing found a real prerequisite regression: T03's one unreliable output slot allowed a same-tick OwnerState to overwrite the RTC probe Pong, eventually timing out an otherwise admitted player. The transport now retains four fixed stream slots (owner, snapshot, pong, cosmetic); a native regression verifies owner and pong survive the same tick. Whole-snapshot per-entity coalescing remains T09 work. Reliable output is admitted to the actor FIFO before kill so best-effort Reject/Disconnect can reach the peer; application cleanup remains immediate.

Verification on 20 October 2026:

- `cargo run -p server --locked -- --check-content` — PASS, actual baked package and all 16 character spawn slots.
- `npm run check` — PASS, generated bridge/protocol/content drift, strict TS, ESLint/formatting, **163 TS tests**, Clippy warnings denied and **56 Rust tests**. T07 adds 15 meaningful native tests: ten world scenarios, three admission/control/scheduler scenarios, runtime content tamper/missing/size/path checks and the same-tick unreliable-stream regression. Every FakeTransport output crosses the real protocol codec and byte limits.
- `npm run build` — PASS, native workspace, release WASM and production frontend. Content/config/trace hashes are unchanged. The pre-existing map inspector >500 kB chunk warning remains; WASM remains approximately 1,265 kB raw / 358 kB gzip. No multiplayer capacity/frame-rate benchmark is claimed.
- Initial live browser admission — PASS, real ticket/Welcome/baseline/StartPlay/status/owner states and both WS/RTC receipts. The 30-frame probe produced **4.000 m authoritative displacement**, then held the new position under neutral input. Resync advanced **epoch 1 → 2**, emitted baseline 2 and restarted with the same actor.
- Actual startup conflicts — occupied WS port 3000 and HTTP port 3001 each caused a separate server invocation to exit with failure before announcing readiness. The ordinary WS preflight race with net-crate's asynchronous internal bind remains a known limitation.
- Actual HTTP readiness — **503**, live process with zero players before a browser receipt report; **200**, one Playing actor after the live probe. The native control test also verifies readiness expiry, stale ticks and overload gating.
- Final fixed production preview — PASS, same-guest replacement closed the old development session with “guest joined again”; new player/session retained **80 WS and RTC pong receipts** through a fresh epoch-2 resync. Production movement again reported **4.000 m**. HTTP confirmed exactly **one session / one Playing player**. After Disconnect and lease expiry, HTTP returned **503 with zero sessions/players**, while liveness stayed true. Screenshot: `/private/tmp/port-meridian-t07.jpg`.

Native lifecycle tests cover separate guests, all 16 inert reserved slots, baseline ordering/count, single readiness transition, input timing/deduplication/conflicts, neutral braking, resync identity/visibility, request cache, observer show/hide, output-full cleanup, all timeout classes, invalid tickets/scopes, replacement and old-generation disconnects. The live browser checks do not claim 16 real WebRTC clients or two independent guest browser profiles.

Limits and exact module interfaces are recorded in document 21. T07 has no keyboard/mouse scene, camera, client replay/reconciliation, remote motion snapshots/AOI, vehicle motor, missions or durable profiles. The pinned net-crate revision and its internal queue/health/shutdown limitations remain unchanged. Ctrl+C discards memory profile/world state; no durable transaction-flush or graceful dependency shutdown claim is made.


**Next proposed chunk: T08 — browser scene, input and camera.** Render the third-person greybox character, add keyboard/mouse/pointer-lock controls and static camera obstruction queries, using real T07 admission and the shared WASM motor. Stop here for user review before beginning T08. The development client and T07 server are left available; the temporary production preview is stopped.

## T08 — Browser scene, input and camera

Status: **complete for the localhost walkthrough, 21 October 2026**. The user authorized this chunk after the progress review. T09 has not been started.

Implemented the root GameApp with explicit boot/menu/load/connect/authenticate/world/playing/resync/disconnect/error phases; verified content/WASM loading; a Three.js greybox district and local capsule; keyboard/mouse controls; pause/retry/leave menus; authoritative health/credits; diagnostics; and sensitivity, inversion, reduced-motion and Low/Medium settings. Extracted `net/session.ts` GameSession from the T07 diagnostic, adding typed asset/owner/status/phase callbacks and bounded tick scheduling. The scripted diagnostic remains available at `/diagnostic`; `/map` and `/simulation` remain standalone. README, current status tables and navigation now reflect T08.

Displayed movement comes directly from 10 Hz authoritative owner samples. Inputs are sampled at 30 Hz and sent once per future tick with three ticks of lead; there is no accumulated catch-up, client movement approximation or local authority. The browser query world uses the real WASM bridge. T09 still owns prediction/replay/reconciliation, input redundancy, adaptive lead, interpolation, AOI and remote motion rendering. This checkpoint therefore has visible movement latency/stepping.

Added a generated StaticSphereQuery and `spherecast_static` through native sim-core, sim-wasm, wasm-bindgen and the typed browser adapter. The camera sweeps a 0.2 m sphere against static physical/vision geometry (including movement-only ground), subtracts a 0.1 m margin, shortens immediately and eases outward over 0.2 s. Character fade/hide prevents close-camera obstruction. Existing ray queries, wire DTOs and content/config/trace hashes remain unchanged. Dynamic cars remain T10.

Input captures only the game canvas. Jump is a one-shot edge, menu keys retain normal browser behavior, and blur/hidden-tab/pointer-lock loss/menu focus clear held controls. Returning from a hidden tab requests resync. The in-app browser declined pointer lock; following the user's request to use that browser without desktop permissions, added and verified focused keyboard controls plus left-drag look as a fallback. Successful native pointer lock was not manually verified. No desktop permissions are required for the tested fallback.

Resources have explicit lifetimes: one app rAF/input timer/listener set; fresh graphics/session/WASM resources per join; abort/cancel/free/close on leave, failure or app teardown. WebGL context loss pauses controls, recreates graphics on restoration and requests resync, with an eight-second recovery deadline and Reload page action. Forced WebGL loss/recovery was not exercised live.

Verification on 21 October 2026:

- **`npm run check` — PASS on Node 24.19.0**, including seven generated bridge files, protocol/content drift checks, release WASM packaging, strict TS, lint/format, **168 TS tests**, Clippy warnings denied and **57 Rust tests**. Five new TS tests cover input edges/focus/blur/visibility/lock loss and two listener lifetimes, camera conventions/shortening/easing, and real release-WASM wall/ground sphere sweeps. One new native test covers wall contact, grazing corners, awnings, initial overlap and invalid radii. The saved movement trace still passes unchanged.
- **`npm run build` — PASS**: native workspace, contract/content generation, release WASM, type checking and Vite production bundle. The final navigation-label cleanup was also rebuilt with the client workspace command. Current WASM is approximately **1,297.52 kB raw / 362.13 kB gzip**; shared Three.js is **536.35 kB raw / 133.79 kB gzip**, retaining Vite's >500 kB warning. No download/frame-rate/capacity gate is claimed.
- Real in-app browser admission: guest ticket, baseline, StartPlay, owner samples and both WS/RTC receipts. Holding W for two seconds changed server feet from **[-183.00, 0.02, 177.00]** to **[-183.00, 0.02, 169.10]**, settling at Z 169.00. Shift+W for 1.5 seconds later moved approximately **10.4 m**. Space produced an airborne server sample followed by landing. Drag look visibly orbited around the owned actor.
- Camera beside a real solid building shortened from **4.53 m to 1.97 m** while the actor stayed fixed at **[-195.37, 0.02, 162.24]**; the solid wall stayed in front of the camera. The downward pitch limit shortened to approximately **2.00 m** against the physical ground. Native corner/awning tests and release-WASM floor regression independently verify radius clearance.
- Esc opened the menu; holding W while paused left authoritative feet unchanged as server ticks continued. Low quality and reduced motion applied without reconnecting.
- Two join/leave cycles on the same mounted app: first player 3; after Leave, actual `/health/live` reported **zero players / zero sessions**. Retry created fresh player 4; health reported exactly **one Playing player / one session**, then Leave returned to retry UI. Input lifecycle tests verify removed listeners cannot affect a subsequent mount. Hot reloads/build-triggered reloads also closed the prior sessions.
- Final running scene screenshot: `/private/tmp/port-meridian-t08.png`. Development client and local server remain available at localhost:5173 and loopback ports 3000/3001. Production compilation passed; the T08 production bundle was not separately exercised in a preview browser.

Known limits: primitive art; direct server-sample movement; no rendered remote movement, prediction, driving, missions, audio, persistent settings/profiles, control remapping or capacity benchmark. Native pointer-lock success and forced WebGL recovery remain explicit manual coverage limits. The tested browser uses the drag-look fallback. Existing pinned net-crate internals and lifecycle limits remain as documented in T07. The repository still has no commits; this chunk did not create a Git checkpoint.

**Next proposed chunk: T09 — prediction, reconciliation and replication.** Add input history/redundancy, timing/lead, local replay, remote timelines/visibility and snapshot budgets; verify two independent guest contexts see one another walking/jumping under seeded loss/reorder. Stop here for user review before beginning T09.

### T08 feedback — centred character and debug cross

The user requested horizontal character centring and a green screen-centre debug cross. Removed the 0.5 m shoulder offset from both the camera orbit position and look target, preserving radius collision and vertical framing. Added a non-interactive green cross at 50% viewport width/height, visible while controls are active and hidden in menus. Updated documents 08/22 to record the requested camera convention. The camera regression now projects the character centreline across multiple orbit/pitch angles, translated positions and collision shortening, requiring zero horizontal screen offset. Targeted camera test, strict TypeScript, changed-file lint and frontend production build pass on Node 24.19.0. Real in-app browser visual verification confirms the centred character and green cross. Screenshot: `/private/tmp/port-meridian-t08-centred.png`. This is a T08 correction; T09 remains unstarted.

### T08 feedback — head-height framing

Raised the look target to feet + 1.8 m so the capsule head sits just below the screen-centre green cross and the body occupies the lower half of the screen. Kept the collision/orbit pivot at feet + 1.45 m to retain its clearance inside the character capsule. Horizontal centring and sphere collision are preserved. Updated camera regression projects the default head/feet onto the screen and requires the head within 0.03 NDC of centre, below the cross, with feet in the lower half. Targeted camera test, changed-file lint and frontend production build pass. Verified the requested framing in the existing in-app browser; screenshot `/private/tmp/port-meridian-t08-head-framing.png`. Documents 08/22 reflect the revised framing.

## T09 — Prediction, reconciliation and remote replication

Status: **complete, 21 October 2026**. The user authorized this next stage after reviewing the T08 camera corrections. T10 has not been started. The approved centred/head-height camera, green cross and in-app drag-look fallback are retained.

Implemented:

- `net/clock.ts` uses the lowest RTT among eight matching pongs, half-RTT compensation and a bounded 2–6-tick lead (default 3).
- `simulation/prediction.ts` executes the actual shared WASM motor on a fixed 30 Hz accumulator, with a 128-entry epoch-scoped input/predicted-state history, neutral gaps, newest-three input redundancy, bounded lead adjustment, authoritative retirement/replay, 100 ms render-only correction decay and large-error/control/mode snaps. Fixed-tick motors agree across 30/60/144 Hz render schedules. Long frames, overflow and excessive drift request resync.
- `world/replica.ts` implements reliable visibility lifecycle, per-entity ticks, same-tick split acceptance, 32-sample timelines, three-tick interpolation delay/extrapolation limit, shortest-yaw interpolation, discontinuity snaps, stale colouring and 256 bounded tombstones. Resync preserves inert old presentation until the replacement baseline completes. Unknown/old-visibility snapshots cannot create ghosts.
- `net/session.ts` integrates scoped ownership, epochs, future-owner cache, status revisions, prediction, replica updates, 500 ms warning / 2 s input stop / 10 s close, and resync acknowledgement/retry bounded to one request per five seconds. Authenticated stale-session envelopes are ignored before lifecycle handling. Guest replacement now sends Disconnect with the old session ID; the native lifecycle test verifies that scope.
- `server/replication.rs` supplies 64 m spatial cells, 160/192 m relevance hysteresis, deterministic self/proximity priority, a 96-visible cap (current shard max 16 players), actual-MessagePack-size snapshot splitting, byte token buckets and per-entity snapshot coalescing. Owner state remains 10 Hz independently of ordinary snapshots. The pump drains owner before snapshots, preserves same-tick splits, separates pong/cosmetics, retires hidden pending records and clears stale output on baseline/control changes.
- Server admission and actual pump submissions enforce 60 KiB/s egress with a two-second bucket; ingress is 30 KiB/s with a two-second bucket, input batches have a strict rolling 60/s cap, and actions retain separate rate admission while remaining unavailable gameplay. Snapshots reserve owner capacity. Native counters expose admitted bytes, packet size/count, budget drops and queued refresh age. PlayerStatus adds its 1 Hz repair heartbeat.
- App rendering now uses predicted local poses and teal interpolated remote capsules. Diagnostics expose owner/predicted tick, RTT, lead, correction/p95, replay count, input ring, remote coordinates/staleness and application byte rates, plus a Resync world action. No second input timer or TS movement motor was introduced.
- Exact 127.0.0.1 loopback Origins were added alongside existing localhost Origins. This permits genuine independent hostname-scoped guest cookies for a development client and production preview in the existing browser; listener addresses remain loopback and wildcard Origins remain rejected.

Verification (Node **24.19.0**, Rust **1.94.0**):

- **`npm run check` — PASS: 178 TS tests / 64 Rust tests**, generated bridge/protocol/content drift, release WASM, strict TS, lint/format and Clippy with warnings denied. T09 adds ten TS tests and seven Rust tests, plus a scoped-disconnect assertion in the existing replacement regression. No acceptance requirement or existing test was removed.
- **`npm run build` — PASS**: native workspace, generated/baked assets, release WASM, TypeScript and Vite production bundle. WASM remains **1,297.52 kB raw / 362.13 kB gzip**. Shared Three.js remains **536.35 kB raw / 133.79 kB gzip** with the existing >500 kB warning. Content/config/trace hashes and net-crate revision remain unchanged.
- Real-WASM seeded application-delivery simulation: both runs complete **1,800 server ticks / 60 seconds**, real bounded input/owner encoding, reorder and duplicate scheduling, and no conflicting inputs or resyncs. At **100 ms RTT / ±20 ms jitter / 2% loss**, full-run correction **p95 0.000000 m, max 0.033351 m**; 2,400 RTC application packets scheduled and 1,070 late redundant input frames discarded. At **200 ms RTT / ±50 ms jitter / 5% loss**, correction **p95 0.033933 m, max 0.601377 m**; 2,396 packets scheduled and 1,884 late redundant frames discarded. The supported static-world p95 is below 0.25 m. These are synthetic application-delivery profiles using a WASM authority reference plus independently tested native admission, not physical impairment of the actual browser RTC link. Displayed p95 metrics use a bounded rolling-256 window; these reported test figures cover the full runs.
- Native tests verify actual ≤1,100-byte packing for 96 independent records, all split records surviving real transport coalescing, unrelated older records retained, newer visibility wins, hide/baseline retirement, spatial hysteresis/self priority/caps, strict byte and input-rate limits, a flood removing its actor, and observer re-entry with fresh visibility IDs. Seeded client replica delivery cannot resurrect hidden actors or erase actors omitted from one packet.
- Live browser pair: **localhost:5173 development** and **127.0.0.1:4173 production preview**, each using its own real cookie/ticket/session and WS/RTC pair. The first guest walked approximately **8 m** (Z 177 → 169); the production guest's interpolated remote sample reported Z **169.18** before settling. On the final server, the production guest walked approximately **6 m** (Z 177 → 171); the development guest reported the same remote movement. A production jump reported Y **0.74 m** locally while the other guest rendered Y **0.67 m**, then both returned to grounded samples. Both pages displayed exactly **Players 2 / remotes 1**.
- Live resync: development player 1 advanced **epoch 1 → 2**, retained its actor/position and one remote, and resumed a bounded new prediction ring; the production guest retained one instance of player 1. Disconnecting production player 2 changed the survivor to **remotes 0**. Retrying created fresh player 3, returning to **remotes 1**, with no old player 2. Real health reported **two Playing players / two sessions / ready true**.
- Loopback steady samples were roughly **10–11 KiB/s up / 9 KiB/s down** per guest, excluding socket/transport framing and content. Lowest recent RTT was roughly 13–22 ms and displayed ordinary correction p95 0.000 m. These are two-player samples, not full-population capacity measurements.
- Screenshots: `/private/tmp/port-meridian-t09.png` (production guest, both players) and `/private/tmp/port-meridian-t09-client.png` (development guest). The final client/server sources were rebuilt after the scoped-disconnect correction; both real guest views and local services are left available for review.

Current module interfaces, exact bounds and reproduction steps are in [23 — Prediction and replication](23-prediction-and-replication.md). README/current task tables are updated. Historical T08 direct-sample and input-timer descriptions are superseded by this entry.

Limits: current entities are player characters only. Vehicle proxy replay/seat attachment, cars/possession, mission-target relevance, idle-car/population rates and gameplay remain their future milestones. Full impairment/lifecycle and actual-RTC 16-player population/soak measurements remain T19/T20. Successful native pointer lock and forced WebGL restoration remain unverified; drag-look is verified in the existing browser. No cross-network/public-hosting claim or durable profile state was added. Dependency-internal net-crate queue/health/shutdown limits remain unchanged. The repository still has no Git commits; this checkpoint remains local uncommitted source.

**Next proposed chunk: T10 — cars and atomic possession.** Add the compact shared vehicle motor and proxies, server occupancy/entry/exit transactions, reliable ControlChanged handling and driving presentation; verify contention, blocked exits, stale ownership, collision tunnelling and disconnect release. Stop here for user review before starting T10. G2 remains partial until driving and the T11 courier loop are complete.

## T10 — Cars and atomic possession

Status: **complete, 21 October 2026**. The user authorized closing the driving milestone after the progress evaluation. T11 remains unstarted.

Prerequisites and repository state: T01–T09 were reviewed against the current source and successful checks. The existing T10 implementation was already present in checkpoint `e058771` when this continuation began; the earlier evaluation's no-commit observation was superseded by that checkpoint. This entry brings the README and task tables into agreement with the implemented and verified milestone.

Implemented scope:

- Shared native/WASM upright compact motor, acceleration/braking/reverse, steering, handbrake, swept static/car collision, reusable bounded vehicle proxies and a frozen 1,800-tick driving trace.
- Two authored parking slots become authoritative car entities. Reliable enter/exit transactions enforce occupancy, proximity, visibility, speed and safe exit geometry; successful possession changes control epochs and clears stale input. Cached action results, reliable replica refreshes, disconnect release, resync and idle-car replication are integrated.
- Browser interaction prompts, E entry/exit, driving inputs, predicted vehicle control, remote car interpolation, attached driver presentation and driving camera. Walking framing and the existing drag-look fallback remain intact. Interfaces and bounds are recorded in document 24 and the module diagrams.

Live regression and repair:

- Initial real driving exposed `invalid vehicle motor/input`: driving north from the first parking slot, then coasting, caused the generic collision controller to add vertical displacement. A new native regression reproduced Y = **0.0021356 m at tick 51**, exceeding the flat motor's 0.001 m validity bound.
- Horizontal car sweeps now ignore support surfaces below the car bottom, discard vertical sliding, and reject obstructed planar endpoints. Sub-millimetre contact displacement below 0.05 m/s is treated as stopped, preventing wall-contact numerical creep and native/WASM speed divergence. Character collision and motor validity checks were preserved.
- Added native and release-WASM regressions against the actual baked parking-to-street geometry, including coasting/braking. Regenerated the frozen driving checkpoints explicitly for this deliberate collision behavior correction; fixture inputs, configuration, template and world geometry remain unchanged. Kept the existing native/WASM position threshold below 1 cm and speed threshold below 0.0005 m/s.

Commands and results (Node 24.19.0, Rust 1.94.0):

- `npm run check` — **PASS**, 183 TypeScript tests / 72 Rust tests, generated bridge/protocol/content drift, release WASM packaging, strict TS, lint, formatting and Clippy with warnings denied.
- `npm run build` — **PASS**, native workspace, generated/baked assets, release WASM and production frontend. The existing Three.js chunk-size warning remains; this is not a frame-rate or capacity result.
- Targeted native vehicle and real release-WASM tests — **PASS** after reproducing and correcting the parking-route failure and contact jitter. The complete check includes both regressions and the 60-checkpoint trace.

Manual scenario evidence:

- Final server and clients used real net-crate WS/RTC: development `localhost:5173` and production `127.0.0.1:4173`, with independent hostname-scoped guest cookies.
- Development player 3 entered compact 1 at **[-152, 0, 176]**, advancing epoch **1 → 2**. Production player 4 approached the same left door and received **occupied**, retaining epoch 1. The observer showed the car plus player 3 attached at seat height.
- Player 3 selected Leave game; the server released the car. Player 4 then entered it successfully at epoch **1 → 2**. Retrying development created player 5 and restored **Players 2**, with no old player 3 replica.
- The production driver accelerated north, then applied the handbrake. Car 1 stopped at **[-152.00, 0.00, 141.01]**; the driver was attached at **[-152.00, 0.80, 140.76]**. The development observer independently reported those same car and seat coordinates. Both guests retained real WS/RTC receipts and displayed correction p95 **0.000 m** in these loopback samples.
- Production E exit restored on-foot control at **[-153.50, 0.02, 141.01]**, epoch **2 → 3**. Re-entry succeeded; an explicit vehicle resync later advanced epoch **5 → 6**, retained the car/seat position, and resumed the bounded prediction history. The earlier development run also verified reverse driving, successful exit/re-entry and a vehicle-preserving resync.
- Screenshot: `/private/tmp/port-meridian-t10-driving.png`, final production driver with two players and diagnostics. Blocked exits, simultaneous contenders, high-speed tunnelling, stale ownership and disconnect release additionally pass native/bridge regressions; no live blocked-exit obstacle fixture was injected.

Known limitations: greybox compact only, two authored cars, flat upright vehicle physics, no vehicle damage/respawn gameplay, missions, NPC traffic, durable profiles or public hosting. Dynamic collisions under network impairment and 16-client capacity/soak remain T19/T20. Native pointer-lock acceptance and forced WebGL restoration remain unverified. G2 remains partial until the courier loop is implemented.

Next action: **T11 — courier vertical slice**, after the user's milestone review. Stop here; no courier or broader-alpha work was added in this continuation.


### T10 feedback — handbrake priority and verification

The user requested verification that the handbrake works. Inspection found that Space supplied the correct held button, but W/S acceleration was applied before braking: with the configured 7 m/s² acceleration and 8 m/s² handbrake, a held accelerator reduced effective braking to 1 m/s². A new native regression reproduced a car starting at 6 m/s still travelling at 5 m/s after one second of throttle plus Space.

Corrected the shared native/WASM vehicle motor so a held handbrake overrides throttle to neutral, then applies the existing rolling resistance and handbrake deceleration. It now stops and holds in both directions even with W/S held; releasing Space restores acceleration. Steering, brake strength, collision and network authority remain unchanged. Updated the owning vehicle rules in document 06 and the usage guide in document 24. The HUD now displays authoritative speed in km/h and a Handbrake indicator while the local driving control holds Space.

Verification:

- Added a native forward/reverse regression comparing handbraking against coasting from 6 m/s: stop within 30 ticks / one second and less than 2 m; remain stationary for another second with throttle held; release and resume acceleration. This failed before the fix and passes afterward.
- Added a real release-WASM regression sampling the actual InputState W/S + Space controls, verifying stop/hold/release in both directions. Existing held-control, pause/focus cleanup and the unchanged 60-second native/WASM vehicle trace also pass.
- `npm run check` — **PASS**, 184 TypeScript tests and 73 Rust tests, release WASM, contract/content drift, strict TS, lint/format and Clippy warnings denied.
- `npm run build` — **PASS**, including native server and production frontend; existing Three.js chunk warning only.
- Live in-app browser using the rebuilt real Rust server: accelerated to **25 km/h**, then held **W + Space** for two seconds and observed authoritative **0 km/h**. Holding W + Space again retained zero speed; the UI visibly showed **Handbrake**. Releasing it and holding W restored **25 km/h**. Space alone then returned to **0 km/h**. No session/epoch change or disconnection occurred during these braking checks.
- Screenshot of the held brake and zero-speed HUD: `/private/tmp/port-meridian-handbrake-held.png`. Browser and server remain available for the user, with the test car stopped.

This is a T10 control correction; T11 remains unstarted.

## T11 — Courier vertical slice

Status: **complete, 21 October 2026**. The user authorized the next step after the handbrake correction. T12 remains unstarted. G2 now passes for the two-guest localhost greybox slice.

Prerequisites: T10 source, its stop/hold/release handbrake correction, real net-crate transport and existing content were reviewed. Native/WASM motors, possession, prediction and private status were retained. No new wire DTO or content bake/hash change was needed.

Implemented:

- `missions.rs` supplies the server-only courier lifecycle: opaque instance ID, content-owned destination/reward/duration, 30 continuous ticks inside 5 m below 2 m/s, failure/timeout precedence, internal reward-pending state, and a five-second terminal view. The world accepts only E/F action intent at the authored terminal, permits one live mission, evaluates authoritative character or controlled-car motion after simulation, and abandons jobs on session cleanup. Resync retains identity/deadline and interrupts dwell.
- The memory profile adapter atomically stores a unique `mission:<instance_id>` event with credits/completed-job count. Identical retries return committed values; conflicting profile/amount and bounds errors mutate nothing. Completion is private and follows commit. Events are bounded to 4,096 per process; profiles remain capped at 256. SQLite/durable recovery remains T17.
- PlayerStatus now uses real changing revisions and mission/profile data for initial readiness, lifecycle changes and its existing one-second repair heartbeat. Another guest's replicas contain no balance or private mission data.
- F is an edged terminal action; E on foot falls back to the terminal when no car takes priority. E still exits while driving; F can use a nearby terminal from the car. Added terminal and destination rings/poles, a private gold route from the baked road graph, distance/countdown/stopping instructions, committed success feedback, and an Esc-menu cancellation button. Guidance is presentation only; no client reports completion/dwell/reward.
- Updated frontend/backend module diagrams, task tables, README and owning memory-persistence wording. New [courier guide and saved demonstration](25-courier-slice.md) includes commands, lifecycle/bounds, limitations and three repository screenshots.

Verification:

- `npm run check` — **PASS**, 187 TypeScript tests / 78 Rust tests, contract/content/bridge drift, release WASM, strict TypeScript, lint/format and Clippy with warnings denied.
- `npm run build` — **PASS**, native workspace, generated/baked content, release WASM and production frontend. The existing large Three.js chunk warning remains. Native/WASM movement fixtures and content hashes are unchanged.
- Five new native world tests cover acceptance range, duplicate/cached actions and one job, private revisions, mission-preserving resync, on-foot and in-car completion, interrupted/too-fast dwell, cancellation/timeout/death/disconnect precedence, atomic/idempotent/conflicting/bounded rewards, five-second cleanup and profile values on rejoin. Three TS tests cover terminal key edges/pause cleanup and mission countdown/outcome guidance. Every fake transport result crosses the real wire codec.
- Live real WS/RTC pair: development `localhost:5173` player 3 and production `127.0.0.1:4173` player 4, independent cookies/tickets/sessions. Runner accepted near **[-172.89, 0.02, 179.62]** at tick **3468**, entered compact 1 (epoch 1 → 2), drove around city/Market obstacles, and stopped its center at **[-44.23, 0, 55.51]** inside the authored Market zone. At tick **7980**, after braking/continuous dwell, HUD displayed **Delivery complete! +150 credits**, balance **150**, with two players and the driver still in the car. The observed run took about **150 seconds**, within its 180-second deadline.
- The production observer independently reported car **[-44.23, 0, 55.51]** and driver seat **[-44.33, 0.80, 55.27]**, while keeping **0 credits** and no active courier. Both guests had real reliable/unreliable pong receipts and ordinary displayed correction p95 **0.000 m**. This is a two-player loopback sample, not a population or impairment benchmark.
- Owner Leave/Retry created fresh player 5 at spawn with exactly **150 credits** and no active mission. No duplicate reward appeared through the terminal view clearing and reconnect. The production guest separately accepted a job, used **Cancel courier**, displayed failure/cancellation feedback, and retained zero credits.
- Initial live admission exposed an overlooked client build-ID gate still expecting T10 after the server advanced to T11. Updated both expectations and rebuilt production before the successful two-guest run; no fallback or weakened version check was added.
- Saved evidence: `docs/evidence/t11-delivery.png`, `docs/evidence/t11-observer.png`, and `docs/evidence/t11-rejoin.png`. The live run used ordinary keyboard controls; native boundary tests arrange fixture actor positions explicitly. No live teleport/completion injection or fake RTC path was used.

Design clarifications: documents 07/09 now explicitly distinguish memory rewards retained through reconnect in a running process from durable T17 rewards retained across process restarts. This corrects an ambiguous future-persistence summary; T11 does not claim disk persistence.

Limitations: one original courier template and compact greybox district; terminal failure feedback combines failure/cancellation because the existing MissionView carries no failure reason. Real timeout/death boundary cases are automated, not additional live three-minute runs. Higher-capacity memory event exhaustion retains pending outcomes and requires restarting the memory server; durable recovery/UI remains later work. Traffic, combat, police, full city, 16-player soak, physical RTC impairment, successful native pointer lock and forced WebGL restoration remain outside this milestone's evidence.

Next action: stop for user review of the complete slice. **T12 — full map, navigation and content kit** is the next plan task; it was not started here.

## T12 — Full city and content kit

Status: **complete**, 21 October 2026. Continued after the user's approval of the next step. The runtime and inspector now use the full 768 m city: six named districts, 36 chunks, 385 static colliders, connected lane/sidewalk/access graphs, eight landmarks, three car templates and four public cars. Trial/recovery templates, repair and eight reserved recovery slots are authored and validated for later gameplay. The old slice remains a separately baked regression fixture. No traffic or pedestrian population was added.

Added hashed streamed visuals with 192 m loading / 256 m retention, at most four requests and 24 detailed resident chunks, two retries, timeout/cancellation, coarse fallback and buffer disposal. All simplified collision loads before Ready and never depends on visual residency. Added original district palettes, outside backdrop, baked minimap, template-aware prediction/seating/prompts and a taller van camera. Content schemas, generated packages and frontend/backend ownership diagrams were updated together.

Verification:

- `npm run check` — **PASS**, 194 TypeScript / 83 Rust tests, including release WASM, content/protocol/bridge drift, typing, lint/formatting and Clippy with warnings denied. Original slice traces remain unchanged.
- `npm run build` — **PASS**, native workspace, generated contracts/content, release WASM and production frontend. Existing Vite chunk-size advisory remains.
- Native full-city acceptance samples **17,818** graph positions for floor/clearance, runs all three vehicle motors against complete collision, sees five distinct landmarks from major intersections, and drives the continuous ring for **7,478 ticks / 2,780.3 m**. Malformed content, streaming bounds/retries/cancellation/disposal, corrupt distant assets, actual vehicle-template prediction and van camera height have regression coverage.
- Live traversal crossed Quay → Market Grid → Civic Center and the former slice boundary, moving beyond the initial chunk's keep radius. Observed 14–18 resident chunks, 94–114 KiB instance matrices, 1.4 MiB WASM, three geometries and zero settled failed/pending requests. Civic van entry succeeded; it reached 17 km/h and stopped under Space. This run preceded final hill/pavilion decoration and revealed the camera obstruction fixed afterward.
- Final package `1bbfceac46cadc4262a397c510856a4158cc7ca7425bcd7266e32d39bc076e63` admitted the browser with real WS/RTC, minimap and streamed detail. Final compact run advanced control epoch 2 → 3, reached **25 km/h**, and stopped under **W + Space** at car center **[-152.00, 0.00, 162.12]**, seat **[-152.00, 0.80, 161.87]**. Displayed correction p95 stayed 0.000 m. A separate clear-space reverse check reached **25 km/h**, then **S + Space** stopped the car at **[-152.00, 0.00, 169.17]**, well before the next parked car at Z = 192; the saved screenshot is this second check. The full map inspector independently verified 36 chunks / 385 colliders and all six district labels.
- `npm run content:audit -- docs/evidence/t12-budgets.json` records raw/gzip asset sizes and managed graphics bounds. Full city assets are **666,200 B raw / 62,477 B gzip**; collision **41,461 B**; conservative first play including all production code is about **722 kB gzip**, below 12 MiB. Retained instance matrices are bounded at **146,944 B**, and minimap pixels at **851,968 B**. These are asset/buffer accounting, not total browser heap/driver measurements.

Saved evidence: `docs/evidence/t12-city.png`, `docs/evidence/t12-handbrake.png`, `docs/evidence/t12-streaming.png`, and the budget JSON. [26 — Full city](26-full-city.md) documents ownership, reproduction and limits. Content provenance lists only original procedural assets.

Limitations: greybox presentation, flat vehicle physics, inactive trial/recovery gameplay, no traffic, combat or police. Final tall-camera behavior is covered automatically; the final-package live driving sample used a compact. Broader memory profiling, frame-rate/capacity gates and sustained soak remain T20. Native pointer lock and forced WebGL restoration retain their prior verification limits.

Next action: stop for milestone review. **T13 — Traffic and pedestrians** is next and has not started.

## T13 — Ambient traffic and pedestrians

Status: **complete, 22 October 2026**. The user approved proceeding after the progress assessment. Saved the completed T12 work as commit `bc53cec`, then implemented this milestone. T14 remains unstarted.

Implemented:

- Added `population.rs`: seeded, ordered population state; once-per-second hidden spawning/retirement; permanent vehicle-pool tags; 24 traffic and 32 pedestrian caps; parked capacity of 24 with eight recovery slots reserved. The four authored ordinary parked cars remain the current public parking stock. Empty parked slots are not populated with arbitrary off-road cars. Occupied traffic cars stay pinned and retain their original slot through player possession/disconnect.
- Directed lane travel-time paths use a FIFO capped at four jobs per tick and two requests per second per car. Five-Hz driving decisions feed the existing 30-Hz motor with proportional cruise control, a 12 m/s maximum, turn slowdown, three front obstacle rays, actor sensing, deterministic four-second intersection reservations, safe no-path stops and hidden stuck recovery. Nearby free authored public parking spots support Parking → Available without creating a replacement entity. AI-driven cars cannot be taken while stopped/yielding.
- Pedestrians use the shared character motor at 1.4 m/s along sidewalk/authored crossing edges, occasional waits, traffic checks before crossing and completion once committed. Added a graph-bound five-second flee hook for future authoritative observations; no shots/damage/police producer was added. Pedestrian replicas use the existing descriptor/state contracts, AOI and interpolation, with purple rendering and local diagnostic counts.
- Updated current architecture diagrams, README, task status and [27 — Ambient population](27-ambient-population.md). The content package and native/WASM movement fixtures are unchanged.

Verification and corrections:

- Final `npm run check` — **PASS**, **195 TypeScript / 92 Rust tests**, generated contract/content/bridge drift, release WASM, strict typing, lint/formatting and Clippy with warnings denied. Final `npm run build` — **PASS**, native workspace, generated/baked content, release WASM and production client. Existing Vite large Three.js chunk advisory remains.
- Seven population tests cover cap/reservation accounting, occupied pinning, deterministic motion, hidden/ranged spawning, shared-motor clearance, sensor braking, unreachable-route stopping, intersection ownership/expiry, actual parking/availability, crossing waits and flee expiry. The 2,100-tick seeded city traversal reaches **24 traffic / 32 pedestrians**, spawns **119** successive actors, observes **50 traffic actors moving over 20 m**, recycles **63** actors before its final empty-world retirement, and never processes more than **four path jobs per tick**. Every vehicle motor endpoint is collision-safe and remains at or below 12 m/s; pedestrian endpoints are capsule-clear. [Saved native transcript](evidence/t13-population-test.txt).
- Two world regressions cross the real codec: NPC show/snapshot replication, rejected AI-seat entry, available-car player driving and disconnect pool identity; and a saturated populated connection completing resync without exhausting reliable bandwidth. The fake world observer changes district position explicitly; this is not claimed as a live driving route or a real-time capacity benchmark.
- Live production verification exposed lifecycle pressure at populated density: Resync world closed the old client with **Incomplete baseline**. A native regression failed before the fix. Snapshot traffic now preserves **64 KiB inside the existing 120 KiB burst allowance** at both application and transport layers, without changing the 60 KiB/s rate; retired sessions cannot emit a dropped baseline's tail. The regression passes afterward.
- Investigation also reproduced an existing shortest-arc interpolation error at ±π: the real WASM proxy test failed with **Invalid bridge DTO/version** before normalizing the interpolated angle, and passes in both directions afterward. Receive failures now retain their concrete error detail instead of presenting every handler exception as an unspecified invalid message.
- Final production browser at `localhost:4173` used real WS/RTC, received 32 relevant pedestrians before resync, and completed **two populated resyncs, epoch 1 → 2 → 3**, remaining the same player 5. Saved diagnostics at tick **2418** show **68 WS / 68 RTC receipts**, **0.000 m correction p95**, 14 resident chunks, no pending/failed chunks and 1.4 MiB WASM. Ordinary keyboard walking afterward reached the Market Grid street, with purple pedestrians and moving cars visible. Screenshots: [street](evidence/t13-street.jpg), [resync](evidence/t13-resync.jpg); [displayed diagnostic transcript](evidence/t13-live-diagnostics.txt). This is one final local guest, not the later 16-player/impairment benchmark.

Limitations: original greybox visuals and simple waypoint driving; no overtaking, signal phases, combat, damage or police. Dense replication can still drop lower-priority snapshots within the configured budget; overdue records now sort by oldest refresh age before distance. Some distant actors can show the existing stale treatment. Visibility safety uses static occlusion from authoritative guest eye positions, not a trusted client camera report. Full population/frame/memory profiling, target network impairment, native pointer-lock and forced WebGL restoration retain their later verification scope.

Next action: stop for milestone review. **T14 — Pistol and damage** is next and has not started.

## T14 — Pistol, impacts and downing

Status: **complete, 23 October 2026**. Continued after the user's approval of the next milestone. T15 remains unstarted.

Implemented:

- `combat.rs` owns a server-only 12-round magazine, nine-tick fire cooldown, 45-tick reload, spawn protection, shot selection, damage cooldowns and respawn deadlines. The existing action cache makes repeated Fire requests idempotent. Reload cancels on driving/downing; respawn restores ammunition and health. Existing Shot, Impact, PlayerStatus and control-epoch DTOs remain the wire contract.
- Chest/muzzle-to-endpoint ray queries test static blockers/ground, exact car boxes and NPC capsules. Pistol damage is 25 with an 80 m limit; player capsules neither absorb nor receive shots. Another guest's occupied car blocks fire but cannot receive damage from the shooter, preventing indirect PvP through forced ejection/downing. Shot effects and hit feedback derive from authoritative events; no client hit/completion claims were added.
- Vehicle contacts expose whether their ID names a static collider or a dynamic car, so overlapping ID namespaces cannot corrupt damage attribution. Crash damage is clamped to 50 and applied once per counterpart per 15 ticks, including both cars where eligible. Relative box/capsule sweeps apply pedestrian impact damage above 4 m/s and exclude players. Nearby authoritative shots/impacts drive T13 fleeing. Pedestrians incapacitate for 300 ticks, then release their slot.
- Fatal car damage freezes the car, clears possession through actor-aware safe door/nearest supported spawn selection, cancels an active job and sends a reliable downed control epoch. A 150-tick safe respawn restores the player with another epoch and preserves their profile/economy. If all positions are blocked, the zero-health seated car freezes and retries without violating the disabled/driver invariant. Car respawn waits 900 ticks, then retries blocked parking every 30 ticks and considers compatible alternatives after 30 retries. Population pool tags remain unchanged.
- Added RMB-held / Q-toggle aim, edged left-click fire, R reload, focus/menu cleanup, a ray-aligned first-person camera, original procedural pistol, 32 pooled cosmetic effects, authoritative hit marker/ammunition/reload/protection/downed feedback, disabled-car indication and incapacitated NPC presentation. Updated current module diagrams, README, task statuses, procedural asset provenance and [28 — Combat guide](28-pistol-and-damage.md).

Verification:

- `npm run check` — **PASS**, **197 TypeScript / 102 Rust tests**, generated content/protocol/bridge drift, release WASM, strict typing, lint/format and Clippy with warnings denied. `npm run build` — **PASS**, native workspace, unchanged content package, release WASM and production frontend. The existing large Three.js chunk advisory remains.
- Eight new real-codec world tests cover duplicate fire/cooldown/empty magazines/reload delay and cancellation; transparent player hit volumes and occupied-car protection; fatal crash, job failure, inert downing, player/car respawn and stale-epoch rejection; walls and pedestrian incapacitation duration; static counterpart cooldowns; symmetric car contacts/no-PvP; real swept pedestrian impacts; and blocked parking respawn. Two shared query tests cover capsule/box rays, range, rotation, swept endpoints, relative motion and near misses. Client tests cover weapon edge/blur cleanup and matching camera/shot direction. Existing deterministic motors, courier, traffic and populated resync regressions remain passing.
- The new world pedestrian-impact test initially failed: resolving car overlaps before observing damage could move the pedestrian clear and erase the hit. Intended pedestrian movement now runs before vehicle movement, swept impact evaluation follows, and surviving actors recover afterward. The test passes without weakening hit detection or moving actors through static geometry.
- Final production browser at `localhost:4173` joined the real T14 WS/RTC server as player 5. Ordinary keyboard/mouse input entered aim, fired into clear space (**12 → 11 rounds**), displayed **Reloading**, restored **12 rounds**, and fired again. Measured ordinary mouse turn to the nearby compact produced **four server-confirmed hits**, reducing the magazine **11 → 10 → 9 → 8 → 7** and showing its disabled smoke marker. Walking to its left door displayed **Vehicle disabled · awaiting a clear parking respawn**; E returned `unavailable`. The guest then moved clear of the parking area; returning later showed **Car health 100**, and ordinary E entry/exit worked again. No live health, actor-position or completion injection was used.
- Saved browser evidence: [confirmed hit](evidence/t14-pistol.jpg), [disabled car](evidence/t14-disabled-car.jpg), [disabled interaction](evidence/t14-disabled-interaction.jpg), [restored car](evidence/t14-car-respawn.jpg). Native crash/respawn/civilian cases arrange fixture poses explicitly; these are not claimed as additional live demonstrations. This remains a local correctness sample, not a populated 16-player or target-impairment benchmark.

Limitations: original greybox weapon/effects, no audio polish, no lag compensation or detailed remote aiming, no explosion/PvP damage and no additional weapons. Police, crime/heat and arrest remain T15. Safe spawn/parking retries can defer respawn when candidates are occupied. Full real-time capacity, memory and impairment measurements remain T19/T20; native pointer-lock/forced WebGL restoration retain their prior verification scope.

Next action: stop for milestone review. **T15 — Police and heat** is next and has not started.

## T15 — Police and heat

Status: **complete, 23 October 2026**. The user approved the next milestone after assessment. Saved the verified T14 checkpoint as `252fc3b`, then implemented T15. T16 remains unstarted.

Implemented:

- Added `police.rs`: authoritative theft, acoustic shot, pedestrian harm and police-car damage observations; causal IDs shared across witnesses and related effects; bounded deduplication; private heat 0–3 with a 30-tick increase cooldown. New witnessed crimes refresh the known position even during cooldown. Sight checks run at 5 Hz / 70 m, loss of sight starts searching after 150 ticks, and each 600 unseen search ticks removes one heat level. Units retain the last observed point rather than tracking hidden targets.
- Two full-city patrol cars start on Quay/Civic road loops. Police have a separate eight-unit cap, one-unit-first allocation and five-second rotating fairness. Reinforcements require collision-clear, hidden candidates at least 40 m from every guest; missing candidates leave slots empty. Unit planning runs independently at at most 2 Hz and steering at 5 Hz, using the shared navigation and vehicle motor. Direct steering is restricted to 15 m with a full vehicle sweep. Stuck/surplus/wreck retirement is hidden-only; visible cars never teleport.
- A live assigned front anchor within 3 m, static sight and player/car speed below 0.5 m/s must remain valid for 90 continuous ticks. Arrest cancels the active job, clears heat/targets, safely releases a driver, stops controls/reload and increments the control epoch. Blocked exits freeze/retry coherently. Safe respawn after 150 ticks restores health/ammunition/protection with another epoch and retains credits. Downing/reconnect release police assignments. Local prediction now freezes disabled characters under held controls.
- Marked the second Quay compact secured. Its server tag persists when moved/respawned and uses the existing public descriptor label for its warning. Police cars replicate with the existing Police + VehicleState contracts, cannot be possessed, and render with pooled original blue beacon material. Added three heat pips, pursuit/search guidance, inferred arrest progress and arrest respawn feedback. Updated current architecture diagrams, README, task table, provenance and [29 — Police and heat](29-police-and-heat.md). New city package: `a322579fa1fedb5e3c058552fa96bcc8e22e0b21a36098b883b6cc35741c9939`; original slice and shared movement fixtures remain unchanged.

Verification:

- `npm run check` — **PASS**, **198 TypeScript / 112 Rust tests**, including contract/content/bridge drift, release WASM, strict typing, lint/format and Clippy with warnings denied. `npm run build` — **PASS**. Verification uses compatible bundled Node 24.22.1 / npm 10.9.2; the documented Node 24.19.0 pin remains. The existing large Three.js bundle advisory remains.
- Four module tests cover exact crime dedup/cooldown/search/reacquisition/decay, continuous arrest interruption, actual static occlusion vs acoustics/range and fatal police self-reporting, and collision-safe shared-motor patrols. Final 900-tick patrol traversal records **205.0 m / 203.3 m**, with the same visible unit IDs, clear vehicle endpoints and speed at or below 12 m/s. Target planning was separated from steering so the independent 2 Hz scheduler does not lose progress at waypoint transitions.
- Six real-codec world scenarios cover real shot spending/duplicates/private status/reconnect cleanup; unseen escape with a frozen known position and exact search expiry; mission failure/arrest/held-input inertness/credits/respawn epochs; 16 wanted guests sharing no more than eight fairly rotated police; secured theft/idempotency/warning descriptors/police possession rejection; and safe driver ejection with stale-epoch rejection. The capacity fixture arranges heat/time explicitly and is not a real-RTC load benchmark. A release WASM regression holds movement while arrested and verifies that movement resumes only after installing the respawn epoch.
- Live production browser on `127.0.0.1:4173` joined the actual WS/RTC server as player 7. Initial out-of-hearing shots correctly spent rounds without heat. Ordinary W movement reached **[-183.00, 0.02, 165.20]**, and another shot reduced ammo **10 → 9** and displayed **Heat ●○○ / Police pursuit**. At tick **1671** diagnostics showed **41 WS / 41 RTC receipts**, **0.000 m correction p95**, 14 resident chunks and zero settled pending/failed chunks. Ordinary S/A movement behind Quay buildings changed the HUD to **Searching**, then staying hidden returned it to **Heat ○○○ / Clear** without further firing. No live actor, health, heat or mission injection was used. After the final production rebuild/server restart, ordinary controls repeated the nearby shot transition **11 → 10 rounds / heat 0 → 1** with both transports connected; [final build screenshot](evidence/t15-final-pursuit.png). Screenshots: [pursuit](evidence/t15-pursuit.png), [search](evidence/t15-search.png), [escaped](evidence/t15-escaped.png).

Limitations: original greybox cars/beacons and waypoint driving; police do not shoot or exit into separate officers. Reinforcement, wreck retirement and safe respawn can wait for hidden/clear space. Arrest/driver/budget edge cases use native fixtures rather than additional live demonstrations. Full real-RTC 16-player performance, memory/soak and target impairment remain T19/T20. Native pointer-lock and forced WebGL restoration retain their previous verification limits.

Next action: stop for milestone review. **T16 — Remaining jobs and repair** is next and has not started.
