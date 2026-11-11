# Server migration to Bevy ECS

Handoff for Sol. Written 2026-11-06 for branch `ecs`.

## Current status

The initial storage/schedule stages and the subsequent full native-system decomposition are complete. Startup, ticks, diagnostics and shutdown now use the same authoritative ECS world; legacy resource-removal adapters are deleted. The original scope and dated instructions below are historical and were expanded by subsequent user-approved refactor stages.

Stages 1–6 were completed within the initial migration scope. Rust 1.95 was explicitly authorised, superseding the original 1.94 constraint. The single actor world, metadata components and live ordered schedule are implemented. See the [qualification report and ownership matrix](../evidence/ecs-migration.md) and [measurements](../evidence/ecs-migration.json) for behaviour comparisons, browser coverage, performance overhead and inherited lint exceptions. The local SQLite server and production preview are ready for testing. System decomposition has since been completed; remote deployment remains outside this work.

The dated checkpoints below are historical; their transitional stores and “remaining” lists describe earlier states, not the current architecture.

## Objective and scope

Move authoritative server entity storage to `bevy_ecs::World` and simulation execution to an explicitly ordered Bevy schedule. Preserve existing gameplay, networking, persistence and tick semantics. Keep the current movement, population, police, combat, mission, repair and chat modules and their responsibilities together. Decomposing those systems is a subsequent project.

This is a data-ownership and execution migration, not a gameplay redesign. Do not add NPC drivers, change pedestrian fleeing, introduce a new universal event bus, or enforce one-writer ownership by extracting new gameplay systems in this work. Document current writers and exceptions so the later decomposition has a clear starting point.

Use standalone `bevy_ecs`; the renderer, browser prediction, `sim-core`, protocol DTOs and net-crate remain independent of Bevy. Keep protocol schema, wire IDs, persistence schema, population limits and content hashes unchanged unless a separately documented defect makes that impossible. Do not deploy remotely without a deployment request.

## Current implementation to preserve

Start by rereading these files, since the checkout may evolve:

- `crates/server/src/world.rs`: players/vehicles in ordered maps, clock, collision, content, configuration and ID allocator.
- `entities/`: player, vehicle, pedestrian and weapon data.
- `systems/population/mod.rs`: traffic and pedestrian storage, pools, navigation, intersection reservations and seeded RNG.
- `systems/police/mod.rs`: wanted state, police vehicles, officers, navigation and coordination state.
- `systems/combat.rs`, `missions/mod.rs`, `repairs/mod.rs`: entity-associated state mixed with shared bookkeeping and economic operations.
- `tick.rs`, `systems/movement/mod.rs`: actual tick order and intermediate state.
- `sessions/mod.rs`, `replication/mod.rs`, `control.rs`, `main.rs`: lifecycle, transport, snapshots, diagnostics and runtime ownership.

Current tick order:

1. Guard against tick-counter exhaustion.
2. Receive session traffic/actions and perform admission.
3. Movement preparation: gather inputs, spawn/direct population and patrols, refresh collision proxies, capture previous positions, make AI decisions.
4. Movement update: pedestrians, vehicles, players; commit the next tick; resolve vehicle impacts and pedestrian recovery.
5. Combat update.
6. Police finish-tick processing.
7. Disconnect peers with unsafe movement results.
8. Repairs.
9. Missions, using pre-movement vehicle state.
10. Publish replication.
11. Replication flush, including its existing lifecycle consequences.

The outer loop separately polls net-crate, runs the fixed-step scheduler, updates control/metrics and flushes transport. Keep catch-up limits, overload admission gating, wall-clock timestamps and transport cadence intact.

Critical details:

- `world.tick` advances inside movement update today, not at the beginning of admission. Preserve the distinction between current and next tick.
- Vehicles currently run in increasing game entity ID order, updating collision proxies as each vehicle commits. Bevy query order is not an equivalent replacement.
- RNG consumption, path-work cursors, target tie-breaking and limited work budgets can also depend on iteration order.
- A fallible phase returning `Err` stops subsequent phases. Do not replace that with log-and-continue or a default panic handler.
- Startup patrols exist before readiness/initial baselines; shutdown resolves sessions and outstanding economic work.

## Target architecture

### Entity storage

One Bevy world is the authoritative owner of all live simulation entities: players, vehicles, ambient pedestrians and officers. Attach identity and coherent components to these entities. Traffic/police control data belongs to the relevant vehicle entity, rather than a second authoritative vehicle record.

Use coarse components initially. Existing character/vehicle state and equipment structs may be wrapped in server-local components to avoid adding Bevy derives/dependencies to protocol or simulation crates. Existing structs may temporarily become aggregate components during migration, but the final representation must expose identity, physical state and optional role/controller state independently enough to query by capability. Do not atomise every scalar into its own component.

Preserve distinctions between player-specific metadata, civilian navigation and officer behaviour. Sharing a component representation does not require merging their algorithms.

Inventory every entity-keyed map. Move intrinsic entity state into components; retain genuinely shared indexes, pairwise collision cooldowns, queues, route graphs and caches as resources. A resource containing all the old authoritative entity maps is a transitional bridge, not completion.

### Identity and references

Keep `protocol::EntityId` as the existing stable game/wire identity. Add a server-local identity component and a lookup from game ID to Bevy `Entity`. Preserve existing allocation/exhaustion behaviour. Bevy entity handles never appear on the wire or in persisted profiles.

Keep peer/session keys and profile IDs distinct. For this migration, existing game-ID references can remain in relationships such as driver, vehicle and target, resolved through the lookup. There is no requirement to rewrite all relationships to Bevy handles at once.

Central lifecycle helpers update components, ID lookup, collision proxies and relevant bookkeeping together. Tests must cover despawn/reuse and stale references. A newly reused ECS slot must never become the target of an old game-ID reference.

### Shared resources and service boundaries

Store world clock/configuration/content/collision, simulation metrics, seeded RNG and shared gameplay bookkeeping as clearly named resources. Keep derived collision structures and replication snapshots explicitly derived; they must not become competing mutable copies of entity state.

Retain the external transport polling loop and persistence worker. During the foundation stage, settle ownership of `Sessions`, `Network`, `Profiles` and control state before migrating gameplay. Bevy resources require owned/static data; borrowed arguments from the current `tick::step` cannot simply be inserted into a resource.

Prefer runtime-owned service handles/resources where their types permit it, with non-send access or exclusive adapters where necessary. If admission or transport publication remains outside the Bevy schedule, keep it in the documented tick wrapper at precisely its current position. Repairs/missions must retain their existing persistence acknowledgement and failure semantics. Do not move effects to a later tick merely to simplify borrowing. Do not use unsafe lifetime extensions, blanket interior mutability or a global mutex around all ECS state.

### Schedule

Use an explicit, sequential schedule with ordered phase sets corresponding to the existing tick. Select the pinned Bevy version's single-threaded executor and specify dependencies; single-threaded execution alone does not establish order. Keep one invocation of each existing phase per simulation tick, including the existing internal subdivisions of movement and police.

Coarse exclusive systems/adapters are acceptable during this migration. Keeping gameplay modules together does not require wrapping the entire legacy tick in one Bevy system. The final schedule must expose its distinct existing phases, timings and failure gates.

Keep `MovementFrame`-equivalent data as tick-local scratch with a defined reset and lifetime. Clear it before every tick and after abort, preventing reuse of previous positions, failures or impacts from an earlier run.

Preserve immediate spawn/removal visibility where current logic relies on it. Use direct exclusive mutations initially when appropriate; any deferred commands require documented application boundaries and tests. Do not assume that adding `Commands` preserves timing automatically.

Use a first-error outcome resource or the selected version's equivalent error plumbing. Gate every subsequent phase after failure and return the error through the existing server API. Explicitly prevent pending deferred work or replication from executing after an aborted phase. Preserve current partial-progress semantics; this migration does not introduce full tick transactions.

Retain direct calls and existing event queues between gameplay modules. New event-driven system decomposition is out of scope.

## Implementation stages

Complete each stage with a passing build and relevant tests before continuing. Record completed work, checks and remaining limitations in this document.

### Stage 1 — Inventory, baseline and compatibility

- Verify branch and preserve unrelated working-tree changes.
- Record entity/component/resource ownership, all relevant iteration ordering and phase mutations. Decide service ownership and error/deferred-work handling described above.
- Run the current server/simulation tests and capture existing failures separately. Record representative tick-time and memory measurements on the same local scenario for comparison.
- Select and exactly pin a standalone Bevy ECS release compatible with Rust 1.94, using only necessary features. Check native server compilation and confirm no Bevy dependency enters `sim-core`, protocol or the WASM build.
- Verify the exact scheduler APIs against that release. At research time, latest `bevy_ecs` 0.20.1 declares Rust 1.95; do not blindly use `latest` or silently bump the toolchain. If a toolchain upgrade is necessary, document it as an explicit prerequisite, including remote build implications.

Exit: dependency choice builds, the ownership/execution design is recorded, and the baseline is reproducible. No gameplay conversion yet.

### Stage 2 — ECS foundation and identity

- Introduce a server-owned simulation container, components/resources, ID lookup and lifecycle helpers.
- Implement testable spawn/read/update/despawn operations and sorted-by-game-ID iteration where order matters.
- Establish the service boundary without moving externally visible effects between phases.
- Add a minimal schedule exercising resource access, tick scratch reset, first-error propagation and deferred mutation policy.

Exit: safe entity identity/lifecycle and schedule infrastructure work in focused tests. Old entity containers may still exist only for entity categories not yet migrated.

### Stage 3 — Migrate the authoritative data model

- Migrate players and vehicles, then ambient pedestrians/officers and their associated traffic/police state in coherent batches.
- Adapt existing gameplay functions and callers to ECS-backed access while retaining their algorithms and module boundaries.
- Move remaining intrinsic per-entity state out of system-owned maps; keep shared coordination state in resources.
- Update sessions, combat queries, vehicle possession, spawning, cleanup, collision proxy generation and startup/shutdown paths alongside each batch.
- Adapt replication and diagnostics to the migrated storage as needed to keep every intermediate stage runnable. Keep network DTO conversion at the boundary.

No category may have both an authoritative legacy map and ECS copy. Temporary facades must read/write ECS directly; do not copy entities into maps, run old systems and copy everything back each tick.

Exit: all live entity state has a single ECS owner, callers compile, and existing gameplay/lifecycle tests pass. The old explicit tick can remain the driver until the next stage.

### Stage 4 — Move tick orchestration into the schedule

- Register the existing phases in their current order and invoke the schedule exactly once for each fixed simulation step.
- Preserve admission/preparation boundaries, tick advancement, impact timing, failed-peer handling, persistence operations and replication order.
- Carry forward timing instrumentation without double-counting.
- Preserve sorted gameplay iteration and deterministic tie-breaking explicitly, including incremental vehicle collision-proxy updates.
- Test phase failures and structural changes, not just successful ticks.

Exit: a real Bevy schedule drives the simulation phases; the outer network/fixed-step loop retains its responsibilities; no second legacy tick path remains active.

### Stage 5 — Boundary verification and cleanup

- Finish removal of migration shims, duplicate stores and obsolete ID allocation paths.
- Verify snapshots, baselines, tombstones, interest management, reliable ordering and budgets retain existing behaviour.
- Verify reconnect, player removal while driving, vehicle removal, NPC removal, death/respawn, arrest and mission/repair persistence.
- Preserve `/api/metrics`, `/police`, health endpoints, shutdown and operator diagnostics.
- Update architecture documentation and record final resource/component ownership and schedule order.

Exit: the server data model and schedule are ECS-native, with existing gameplay systems still recognisable and ready for later decomposition.

### Stage 6 — Qualification and local handoff

- Run relevant full Rust server/simulation/content suites, protocol/content/bridge checks, type checks and the normal production build. Report unrelated pre-existing lint/format failures separately.
- Run real transport tests and a local two-player browser smoke test: join, move, drive, shoot, damage, disconnect/reconnect. Exercise traffic, police pursuit/shooting/surrender, missions and repair persistence.
- Compare representative gameplay traces and replication outputs with the baseline. Ignore only explicitly nondeterministic identifiers/timestamps, not simulation differences.
- Compare tick-time percentiles and memory under equivalent load. Investigate material regressions; parallel execution is a later optimisation, not the default remedy.
- Restart the local server after major completed changes, and leave the final local build running for user testing. Remote deployment requires a separate request.

Exit: validation evidence and known limitations are recorded, and the user can test the migrated server locally.

## Required regression coverage

Prioritise meaningful behavioural checks:

- The phase trace matches current ordering; tick advances exactly once at the original semantic boundary.
- Fatal failures stop all later phases and publication; subsequent invocations cannot consume stale scratch or commands.
- Same-tick admissions/spawns/removals are visible to exactly the same consumers as before.
- Game IDs resolve to the correct entity through spawn/despawn/reuse; references and replication removals remain valid.
- Equivalent worlds with differing ECS storage/archetype history produce the same ordered simulation results.
- Vehicle input precedence remains player → police → traffic → neutral.
- Swept impacts retain pre-movement data; collision queries observe the same per-vehicle update ordering.
- Session disconnect and shutdown preserve save/economic semantics and clear possession/target references.
- Replication contains the same public state, equipment/appearance and health, without exposing ECS internals.

Existing coverage lives in server module tests, `crates/server/tests/{replication,persistence,control,actual_transport,capacity}.rs`, their support/scenario fixtures, and `crates/sim-core/tests/`. Extend these rather than replacing them with tests that only assert Bevy can spawn a component.

## Definition of done

- Bevy owns simulation entities and component storage, and an explicit Bevy schedule drives tick phases.
- No duplicate authoritative player/vehicle/NPC maps survive behind resources.
- Gameplay systems have been adapted, not decomposed or redesigned.
- Wire identities/contracts, persistence and simulation behaviour are preserved.
- Execution order, failure policy, lifecycle visibility and state ownership are documented and covered by tests.
- Local build is ready to test; evidence and remaining limitations are recorded.

## References

Use documentation for the chosen pinned version when implementing:

- [Standalone Bevy ECS concepts](https://docs.rs/bevy_ecs/latest/bevy_ecs/).
- [Schedule and explicit ordering](https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/struct.Schedule.html).
- [Deferred command application](https://docs.rs/bevy_ecs/latest/bevy_ecs/schedule/struct.ApplyDeferred.html).
- [Bevy ECS 0.20.1 package metadata, including Rust 1.95 requirement](https://docs.rs/crate/bevy_ecs/0.20.1/source/Cargo.toml).

## Implementation progress — 2026-11-06

The user has authorised upgrading to Rust 1.95. The repository toolchain/MSRV and current setup instructions now use 1.95.0. Server dependency `bevy_ecs` is pinned to 0.20.1 with default features disabled and only `std`; this supersedes the original Stage 1 requirement to stay on 1.94. Browser/protocol/shared simulation crates do not depend on Bevy.

Baseline before migration: server library tests on Rust 1.94 passed 99 tests with one ignored test. `cargo check -p server` passes on Rust 1.95; the existing two unused-mut warnings in movement remain. Historical evidence files retain their original toolchain labels.

`crates/server/src/ecs/mod.rs` introduces identity/lifecycle storage and schedule foundations, with tests for duplicate IDs, stale handles, ordered iteration and failure-gated execution. It is deliberately not wired to the live tick yet: legacy maps remain the sole live authority, avoiding duplicate entities. This is the start of Stage 2, not completion of the ECS migration.

Remaining: service ownership integration, tick-local scratch/reset, baseline performance capture, authoritative entity migration (Stage 3), live schedule integration (Stage 4), boundary cleanup and qualification (Stages 5–6). Continue from the existing tick rather than enabling an empty schedule. No ECS performance improvement is claimed at this point.

Foundation validation on Rust 1.95: server library suite passed 102 tests with one ignored (99 existing plus three foundation tests). The identity component is immutable so ordinary component updates cannot invalidate its index. No production tick switch has been made.

The full production build (native workspace, WASM, TypeScript and Vite) also passed on Rust 1.95. The local debug server was started with the existing development database.

### Player/vehicle storage checkpoint — 2026-11-06

Live `World.players` and `World.vehicles` now use `ComponentMap<Player/Vehicle>` instead of `BTreeMap` entity storage. The adapter owns Bevy component data and a game-ID-to-handle index; there are no shadow copies or per-tick copy-back. Player and vehicle aggregate structs are server-local components. Population vehicle access and its test fixtures use the same adapter. ID order is preserved for reads and mutable traversal, replacement preserves the ECS handle, and removal despawns the component entity.

This is a transitional Stage 3 implementation: each independently borrowed category currently owns an ECS world. Consolidation into one world, finer coherent components, NPC/controller migration and removal of the map-shaped adapter are still required by the final architecture. Do not mark Stage 3 or the overall migration complete. The live tick still uses the original explicit orchestration. The separate `EntityStore`/schedule foundation remains unused by gameplay until consolidation.

Validation: full `cargo test -p server --locked` passed (including storage lifecycle regression, replication, persistence and actual transport suites); normal `npm run build` passed. Existing compiler warnings remain. No remote deployment performed.

### NPC and controller storage checkpoint — 2026-11-06

Ambient `Pedestrian`, traffic `Traffic`, police `Unit`, `Officer` and player `Wanted` state now derive server-local Bevy components and use the ECS-backed ordered adapter. Their old authoritative `BTreeMap` stores are removed. Shared navigation graphs, crime queues, intersection reservations, metrics and scheduling cursors retain their existing ownership and behaviour.

Mutable ordered traversal now supports the existing NPC update loops directly, sorting by game ID rather than ECS insertion/archetype order. A regression exercises deletion/reinsertion and mutable iteration/retain, checking that removed entities leave neither an index entry nor an ECS entity behind.

The same transitional limitation remains: independent category adapters currently own separate worlds. This checkpoint does not satisfy the single-world architecture or activate Bevy tick execution. Next architectural work is consolidating these components into one authoritative world and adapting callers to borrow component views, followed by wiring the scheduled phases and service boundaries. Do not keep adding new gameplay to the transitional adapters.

NPC checkpoint validation: full server tests passed (104 library tests plus integration suites; existing ignored tests remain ignored), production build passed, and Rust formatting/diff checks passed. The storage regression counts actual gameplay components because Bevy also allocates internal bookkeeping entities. Local server restarted with this checkpoint.

### Shared-world consolidation checkpoint — 2026-11-06

`World.entities` now owns the single Bevy ECS world for all actor and controller components. The temporary `ComponentMap` implementation and category-specific worlds are removed. Player/vehicle/pedestrian/officer records are component aggregates, traffic and police unit components attach to the corresponding vehicle, and wanted state attaches to the player. `EntityStore` has one game-ID index; immutable ordered projections hold references, not copies or separate indexes.

All gameplay, session, replication, diagnostics and fixture callers now access this shared store. Lifecycle tests cover attaching components to an existing entity, preserving other components when one is removed, despawning after the last component is removed, stale handles, deterministic traversal after archetype changes and storage reuse. Actor/controller mutations remain immediate. Driver possession and presentation are staged to avoid overlapping mutable component borrows without unsafe code.

Read-only vehicle/player/pedestrian observations are captured within AI decision passes to preserve access separation while retaining existing algorithms. They never replace authoritative state or get copied back wholesale. Officer authorization/cooldown writes use a small per-target overlay and commit before `step_officers` returns; subsequent officers in the same pass see those writes, preserving the original escalation and shot pacing.

Validation: full server tests passed (104 library tests and all non-ignored integration tests), including replication, persistence, combat, pursuit, surrender and actual transports. Production build also passed. Remaining work: connect the live tick to the Bevy schedule and service/resource ownership, migrate remaining intrinsic metadata as appropriate, and complete performance/browser qualification. This checkpoint completes consolidation, not the overall migration. Keep gameplay systems together.

### Live Bevy tick schedule checkpoint — 2026-11-06

`tick::TickDriver` now runs the live simulation through the ordered Bevy phase sets. The old `tick::step` function is removed; production `main.rs` and gameplay test fixtures use the driver. Existing gameplay modules retain their responsibilities, timings and internal ordering. Startup still creates patrols before readiness, shutdown resolves economic work, and the main loop retains its fixed-step/catch-up policy, overload admission gate and transport poll/flush cadence.

Runtime/service ownership is explicit and safe: a non-send runtime resource owns the core world, sessions, systems and network on the original thread. A scoped non-send IO resource owns the existing Profiles value during the schedule, restored to the control-plane caller before returning any normal success/error. The existing main-loop control mutex remains held during that scope. Profiles are moved, never cloned; no borrowed references are stored as static resources. The schedule host is an orchestration world without actors, and the sole actor/component world remains `Runtime.core.entities`. Existing coarse adapters use that runtime, rather than native actor Query parameters on the schedule host; this is a documented integration boundary for later decomposition.

A first-error resource suppresses subsequent phase sets. All gameplay structural mutations remain immediate, with no deferred command buffers. Tick scratch holds only this tick's MovementFrame and gameplay timing; it resets at tick start and is cleared on return, including failure. The clock still commits inside movement. No recursive dispatch or parallel execution was introduced.

Validation: full server tests passed (107 library tests plus all non-ignored integration suites). Three new driver tests verify real phase order/once-per-tick execution, every phase failure and suppression of later work, profile restoration, scratch cleanup, recovery on the following tick, and tick-counter exhaustion. Existing regression tests now execute through the live Bevy driver. Normal production build and formatting checks passed.

Remaining qualification: equivalent-load performance/memory measurements and manual multi-client/browser testing; remaining entity-associated metadata/resources can be refined before system decomposition. No remote deployment performed.

Local acceptance after restart: `local_probe` passed through localhost:4173 with complete baseline, StartPlay, nonce-matched WebSocket and WebRTC receipts, and `/health/ready` returning 200. The probe disconnected normally. Server and frontend are left running for manual testing. This is native-client transport acceptance, not the pending manual browser/multi-client qualification.

### Final qualification — 2026-11-06

Remaining intrinsic entity metadata now lives in named components: vehicle homes, pool membership, disabled/incapacitated state, missions, recovery reservations and secured vehicles. Shared navigation, pairwise cooldowns, queues, metrics and profile-keyed persistence operations retain their appropriate shared ownership. Immutable vehicle templates use Arc to avoid repeated deep copies in AI observations.

All six migration stages meet their documented exit criteria. The qualification report records passing workspace/build checks, equal baseline traces, equivalent-load timing/RSS, actual browser/transport coverage and inherited strict-lint/format failures. Browser smoke covered movement, damage, driving, pursuit, mission acceptance/cancel and reconnect; detailed police engagement and durable economic flows were covered by automated scenarios. This is short local qualification, not a sustained capacity soak. No remote deployment performed.
