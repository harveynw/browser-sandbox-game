# 13 — Implementation plan

[Index](../README.md) · Copyable prompts: [Agent handoff](14-agent-handoff.md)

## Delivery rules

Implement in the order below. Each task should leave a buildable repository and a runnable demonstration of the features completed so far. Current results and verification live in [the implementation log](implementation-log.md). The user requested a check-in after each major chunk; stop at the task boundary and await their decision before continuing.

One task per agent request is the default. If a task needs multiple commits, keep its acceptance boundary intact. If it is too large, split it into explicit substeps in the implementation log before coding; do not replace its acceptance tests with TODOs.

Use the real crate early. Finish a small greybox game at T11 before investing in city art or complex AI. Localhost is the final deployment target. Tasks for public infrastructure are intentionally absent.

## Progress and dependency table

| Status | Task | Depends on | Outcome |
|---|---|---|---|
| Complete | T01 Workspace and commands | — | Repeatable Rust/TS build |
| Complete | T02 Protocol/code generation | T01 | One source of wire types, cross-language fixtures |
| Complete | T03 Real transport spike | T02 | Browser ↔ Rust over WS and RTC |
| Complete | T04 Greybox content package | T02 | Slice map, collision, hashes |
| Complete | T05 Native character motor | T04 | Fixed-step collision-aware movement |
| Complete | T06 WASM prediction bridge | T05 | Same movement in browser |
| Complete | T07 Authoritative sessions/world | T03, T05 | Server spawn, inputs, baselines, ownership |
| Complete | T08 Browser scene and controls | T04, T06, T07 | Third-person greybox walkthrough |
| Complete | T09 Prediction/replication | T08 | Two-player synchronized movement |
| Complete | T10 Cars and possession | T09 | Shared driving and atomic entry/exit |
| Not started | T11 Courier vertical slice | T10 | Complete short gameplay loop |
| Not started | T12 Full city and content kit | T11 | Six districts, navigation, streaming visuals |
| Not started | T13 Traffic and pedestrians | T12 | Bounded city population |
| Not started | T14 Pistol and damage | T13 | Authoritative hits, health, downing |
| Not started | T15 Police and heat | T14 | Witnesses, pursuit, search, arrest |
| Not started | T16 Remaining jobs and repair | T15 | Time trial/recovery/repair rules |
| Not started | T17 Local profile persistence | T16 | Restart-safe credits and idempotency |
| Not started | T18 Presentation and UX | T17 | Coherent art, camera, audio, HUD |
| Not started | T19 Impairment and lifecycle suite | T18 | Robust under loss/reorder/reconnect |
| Not started | T20 Capacity and performance | T19 | Measured 16-player budgets |
| Not started | T21 One-command local launch | T20 | Reliable local setup/build/run/stop |
| Not started | T22 Final acceptance and handoff | T21 | Reproducible tested local alpha |

The dependencies describe the recommended execution order; they are not permission to delegate or run overlapping edits automatically.

## T01 — Workspace and commands

Read 03 and 12. Create Cargo/npm workspaces, minimal crates and client entry point, strict TS config, formatting/lint config, toolchain pins, gitignore, and lockfiles. Add `docs/implementation-log.md` with all task statuses initially not started except this task once verified.

Provide `npm run dev`, `npm run check`, `npm run build`, and `cargo test --workspace`. Early build may render a blank diagnostic page; do not claim gameplay. Add explicit `xtask` command skeletons that fail clearly for unfinished work, rather than returning fake success.

Verify native compilation, TS checking, and test harness launch. Record tested tool versions and pin compatible dependencies. Install/use the WASM target through ordinary setup instructions; do not silently change a user’s global toolchain.

**Done:** clean-checkout commands are documented and the skeleton builds. No “hello world multiplayer” claim.

## T02 — Protocol, validation, generation, fixtures

Read 05 completely. Own `crates/protocol`, protocol exports in `xtask`, `packages/protocol`, and protocol fixtures. Implement every documented DTO variant even if its feature handler comes later. Use strict serde payload structs, tagged enums, finite/range validation, and bounded MessagePack structure scan.

Generate TypeScript and JSON Schema 2020-12. Implement browser codec/validation, positive and negative fixtures, both language directions, and `check-generated`.

Replace the T01 `xtask` failure stubs/tests with real generation checks, wire generation/drift verification into the root build/check commands, and remove Vitest's temporary `--passWithNoTests` allowance once the protocol suite exists.

**Done:** G0 protocol portion passes; all variants round-trip; sizes are measured; an intentionally edited generated file fails drift check. Report encoded maximum InputBatch/OwnerState sizes.

Do not implement gameplay or use handwritten duplicate TS types.

## T03 — Actual net-crate transport

Read 04 and transport interfaces in 07/08. Create `NetCrateTransport`, BrowserTransport, and a diagnostic two-context page. Use the pinned crate and port its actual signalling format; split JSON text from binary application data.

Implement application queue limits, pump thread, ready/close behavior, timeouts, and lifecycle logging without secrets. Establish ServerHello before ClientHello. A minimal diagnostic session may answer only ping/hello; unimplemented gameplay requests reject as unavailable.

**Done:** real WS and RTC byte round trips, 15-second negotiation timeout, pre-open payload discard, disconnect cleanup, repeated reconnect, and a loopback readiness probe. G1 passes locally.

Do not work on public IPs, TLS/TURN, or replace the crate. If pinned behavior blocks normal use, record a minimal reproducer before making a tracked dependency fix.

## T04 — Greybox slice content

Read 02. Create content DTOs, schema generation, validator, deterministic bake command, slice manifest, shared collider package, navigation graphs, and primitive visuals. Include initial reservations/anchors, compact template, courier template, and temporary slice boundary.

Implement canonical hashing and collision/visual debug overlay data. Validate road corridors, references, clear spawns, and mission connectivity.

**Done:** bake twice yields identical hashes; all coordinates/units agree; invalid overlapping spawn is rejected; slice can be inspected without a game server.

## T05 — Native character simulation

Read character sections of 06 and the coordinate rules in 02. Implement CollisionWorld/CollisionQueries and the character motor in Rust with fixed DT. Preserve complete motor state in DTOs.

Tests: walking/sprinting, diagonal normalization, acceleration, jump edge, grounded state, walls/corners/ceilings, step height, and neutral input. Store a 60-second input trace with expected checkpoints.

**Done:** no networking dependency in sim-core; bounded valid inputs produce finite, collision-safe states. Do not implement vehicle dynamics yet.

## T06 — WASM bridge

Implemented: see [20 — WASM bridge](20-wasm-bridge.md) and the implementation log for bindings, limits, measured native/browser agreement, and reserved vehicle work and the T08 sphere-query extension.

Read WASM interface in 08. Compile sim-core through sim-wasm and package its generated JS/WASM. Implement collision loading, reset, one-step, state query, vehicle-proxy placeholder support, and camera static query.

Export bridge DTOs separately from wire messages. Run the T05 trace in a real browser and native Rust.

**Done:** native/WASM paths agree within 1 cm; no per-step static-world rebuild; no hand-ported movement code in TS. Production bundling initializes the WASM asynchronously and handles load failure.

## T07 — Authoritative sessions and world

Implemented: see [21 — Authoritative localhost sessions](21-authoritative-sessions.md) and the implementation log for concrete APIs, real-browser verification, local readiness scope and T08 handoff.

Read 05, 06, 07. Implement server world store, fixed tick scheduler, local guest session/ticket endpoints with memory profile adapter, admission phases, baseline, ClientReady/StartPlay, input admission, and server character motion.

Add FakeTransport for deterministic world tests. Create/remove player entities and enforce one session per guest. Use distinct profile/entity/peer/session IDs. Implement status baseline after StartPlay, resync, and disconnect cleanup.

**Done:** joining allocates one inert loading actor, playing starts only on readiness, stale/duplicate/future inputs cannot accelerate it, disconnect removes it, port conflicts fail readiness.

## T08 — Browser scene, input, and camera

Implemented: see [22 — Browser walkthrough](22-browser-walkthrough.md) and the implementation log for real-server walking, camera clearance, input lifecycle, browser fallback and remaining T09 latency limits.

Read 08 and relevant 10. Build GameApp phases, manifest/WASM loader, Three.js scene, local character model, keyboard/mouse input, pointer lock, settings basics, camera collision, and minimal HUD.

Consume actual server session/baseline; retain authoritative data separately from local presentation. Local movement can initially use direct authoritative samples while prediction is completed in T09, but state the temporary latency limitation.

**Done:** camera follows a controlled actor, respects walls, input releases on blur, no scroll/fire leakage into menus, errors/timeouts have retry UI, join/leave twice leaves no duplicated handlers.

## T09 — Prediction, reconciliation, and replication

Implemented: see [23 — Prediction and replication](23-prediction-and-replication.md) and the implementation log for concrete modules, measured impairment correction, real two-guest browser evidence and remaining vehicle/capacity limits.

Implement all timing/epoch/replay/interpolation/AOI rules in 05/06. Add 128-tick input history, future-tick input schedule, owner correction smoothing, remote timelines, visibility IDs, independent snapshot packet splitting, byte budgets, and per-entity tick comparisons.

**Done:** two independent browser contexts see each other walk/jump; seeded loss/reorder tests cannot create ghosts or speed hacks; static-world target impairment correction is measured. Export debug metrics for later tuning.

G2 is partially satisfied here; driving/job remain required.

## T10 — Cars and atomic possession

Read vehicle motor in 06 and possession in 09. Add compact vehicle simulation to native/WASM, car proxies, server occupancy transactions, ControlChanged, safe entry/exit, car rendering, driving camera, and contextual prompts.

Use one compact template first. No realistic suspension, car flipping, passenger seat, or drift physics expansion.

**Done:** two contenders produce one driver; blocked exits fail correctly; stale inputs/owner snapshots cannot take back a previous actor; 30 m/s collision test does not tunnel; disconnect clears the driver.

## T11 — Courier vertical slice

Read courier rules in 01/09. Add terminal interaction, one active mission, destination dwell, timeout/failure, memory economic adapter, private status, HUD guidance, and success feedback.

Drive `terminal_quay → delivery_market` in the actual two-context test. Duplicate completion must not award twice.

**Done:** G2 fully passes. Save a short manual demonstration and exact commands/results in the log. Stop expanding features until walking, driving, and the delivery loop feel usable. This milestone is a useful standalone deliverable.

## T12 — Full map, navigation, and content kit

Expand content to six districts, road ring/graphs, three car classes, landmarks, time-trial/recovery reservations, and all named zones. Remove slice boundary only in full-map config.

Implement visual chunk loading/unloading, manifest/asset hash verification, low-detail backdrop, minimap bake, asset provenance, and resource disposal. Keep all simplified collision preloaded.

**Done:** full map is traversable with no collision holes, all routes validate, asset/memory budgets are recorded, and unloaded visuals do not remove physics.

## T13 — Ambient population

Read population/AI rules in 02/09. Add traffic and pedestrian state machines, graph steering, intersection reservations, low-frequency decisions, pooled population limits, offscreen spawning, and stuck recovery.

**Done:** 24 traffic/24 parked/32 pedestrians respect pool accounting; cars do not cross buildings; population counts stay bounded during traversal; NPC controls use the shared motors. Do not implement police yet.

## T14 — Pistol, impacts, and downing

Implement server weapon/cooldown/reload/raycast rules, NPC and vehicle health, vehicle impact observations/damage, downed state/respawn, Shot/Impact effects, and no-PvP rules.

**Done:** duplicate fire causes one effect on ammo/health, walls block shots, cooldown is authoritative, car damage cannot harm other players, respawn is safe and cancels a mission. No lag compensation or additional weapons.

## T15 — Police and heat

Add two patrol units, witness/acoustic crime detection, deduplication, heat 0–3, fair capped pursuit assignment, last-known-position search, arrest dwell, and respawn cleanup.

**Done:** a witnessed crime starts pursuit; an unseen player can escape; heat decays as specified; at most 8 police; no visible teleport; arrest requires continuous valid conditions. Add readable heat/pursuit HUD.

## T16 — Time trial, recovery, and repairs

Implement remaining mission variants, ordered swept checkpoint gates, reserved recovery cars, garage safe exit, request/dwell validation, repair debit/refund with delayed memory-worker test, and F interaction while driving.

**Done:** missing checkpoints cannot win; eight recovery reservations cap correctly; destruction/cancel/disconnect releases them; repairs cannot debit twice or repair the wrong car after disconnect.

Still use the persistence interface with a memory adapter; real disk implementation is next.

## T17 — Local SQLite profiles

Implement schema/migrations, cookie-secret hashing, profile load/update, worker thread, idempotent economic transactions, bounded retries, committed-status publishing, compensation, and restart recovery. Update full local config to SQLite.

**Done:** credits survive restart/rejoin, duplicate mission event awards once, insufficient funds rejects transactionally, repair interruption refunds once, tests use temporary databases. No position/job/vehicle persistence.

## T18 — Coherent presentation and UX

Finish assets, palettes, character clips, car wheels/lean, camera aiming/driving transitions, audio pools, minimap/full map, contextual tutorial, controls remapping, quality presets, reduced motion, and readable HUD/error screens.

**Done:** first-time user completes a job from in-game prompts; no missing asset license; audio requires gesture and mutes correctly; low-quality/resized play works. Detailed remote aiming remains excluded by v1 protocol.

G3 passes after all gameplay and UX checks.

## T19 — Impairment and lifecycle regression

Implement the full matrix in 11 using seeded packet scheduler plus real local transport tests. Cover pre-open data, baseline ordering, same-tick packet splits, epochs, stale sessions, backgrounding, queue saturation, resync, and ID reuse.

**Done:** every documented regression has a reproducible test or clearly recorded manual evidence where automation is infeasible. Fix failures without weakening authority or bypassing real RTC.

## T20 — Capacity and performance

Run 15 actual-net-crate synthetic clients plus one browser observer with full population. Add timing/byte/memory instrumentation, soak tests, and frame profiling. Synthetic peers must implement real signalling; WebSocket-only load is insufficient.

Optimize measured bottlenecks: AOI priorities, idle rates, path queue, instancing, allocation reuse, and chunk disposal. Do not add delta compression or Web Workers unless measurements justify a separately specified change.

**Done:** G4 passes or exact unmet metrics remain explicitly incomplete. Record hardware, browser version, build, seed, duration, p50/p95/p99, and packet sizes. Do not substitute an unpopulated scene benchmark.

## T21 — Local launch and build packaging

Implement `setup`, `dev:all`, `preview:local`, temporary test config generation, child-process supervision, loopback readiness, and clean shutdown. Document normal/private browser-context testing and local save location.

**Done:** fresh documented setup launches through one command; production client build runs locally; occupied ports and missing toolchains give actionable errors; Ctrl+C leaves no listeners. No cloud/TLS/TURN work.

## T22 — Final acceptance and handoff

Run the full document 11 script on the local production build, verify save/restart, inspect visual/audio/camera behavior, and review every task status. Complete the implementation log and README usage instructions with measured results and known limitations.

**Done:** G0–G5 pass locally, no placeholder accepted feature, no unreviewed change to the specification, all claims tied to evidence. Link the local URL and commands; distinguish implemented alpha features from future public-hosting work.

## If a task fails

Record the failing gate, exact command/scenario, shortest error excerpt, likely owning module, and next action. Keep the task incomplete. Fix within that module before adding new scope. Change these documents only for a deliberate design correction, never to disguise a failed implementation.
