# ECS migration qualification — 2026-11-06

Historical qualification of the initial storage/schedule migration. Its Runtime and coarse-adapter descriptions are superseded by the later native-system checkpoints in `docs/architecture.md` and `docs/diagrams/ecs-remaining.mmd`. Production startup, ticks, diagnostics and shutdown now use native ECS access; the legacy bridge has been deleted. Measurements and test totals below remain historical and are not claims about the final cleanup build.

The server migration is complete within its agreed scope: Rust 1.95.0, standalone Bevy ECS 0.20.1, a single authoritative actor store and an ordered live schedule. Existing gameplay modules remain together. Protocol, persisted profile schema, stable game IDs and content hashes are unchanged. No remote deployment was performed.

## Ownership and mutation boundaries

| State | Authoritative owner | Current writers / exceptions |
| --- | --- | --- |
| GameId and ID index | Actor ECS world, through EntityStore | Lifecycle helpers; GameId is immutable. Stable protocol IDs resolve internal generational handles. |
| Player aggregate | Player component | Sessions/admission, movement, combat, police arrests, jobs and repairs; these writers remain in their existing phases. |
| Vehicle aggregate | Vehicle component | Lifecycle/possession, movement, combat, police, jobs and repairs. Immutable authored templates are shared through Arc. |
| Pedestrian aggregate | Pedestrian component | Population director/AI, movement and combat. |
| Traffic and police Unit controllers | Components attached to their vehicle | Population and police respectively; lifecycle cleanup removes stale controllers. |
| Officer | Officer component | Police spawning/AI, movement and combat. |
| Wanted | Component attached to player | Crime processing, pursuit/arrest and officer engagement. Officer-local output overlays preserve same-pass escalation before committing. |
| VehicleHome, PoolMembership | Vehicle components | Population spawn/recycle/registration. Existing homes are preserved. |
| DisabledVehicle, Incapacitated | Vehicle/character components | Combat damage, recovery and lifecycle cleanup. |
| PlayerMission | Player component | Job acceptance, objective advancement, cancellation and reward lifecycle. |
| RecoveryReservation, SecuredVehicle | Vehicle components | Mission reservation/release and police security bookkeeping. |
| Navigation, intersections, RNG, work cursors, crime queue and metrics | Shared runtime/system state | Existing directors and phases; these coordinate multiple actors rather than duplicate actor state. |
| Pairwise impact cooldowns | CombatSystem | Combat; pair-keyed coordination remains shared. |
| Pending repairs | RepairSystem, keyed by profile | Persistence-backed operation tracking survives entity disappearance; not intrinsic actor state. |
| Sessions, Network | Non-send Runtime | Admission, publication and outer transport loop retain their original ownership/cadence. |
| Profiles | Existing control-plane owner; moved into scoped TickIo during a tick | Existing control mutex remains held. Restored before success or normal error return; never cloned. |
| MovementFrame, observation snapshots, replication snapshots, collision proxies | Derived scratch/cache | Rebuilt at the existing boundaries; no whole-entity copy-back. Tick scratch clears on success/error. |

The schedule host owns orchestration resources and a non-send Runtime. The only actor ECS world is Runtime.core.entities. Existing coarse phase adapters reach components through Runtime rather than native actor Query parameters on the schedule host. Shared world/config/content/collision state likewise remains grouped in the owned runtime. This is the explicit integration boundary for subsequent system decomposition, not a second authoritative actor world or per-category map bridge. One-writer enforcement and parallel execution are deferred.

Execution is single-threaded and explicitly ordered: admission → planning → movement → combat → police → failed peers → repairs → missions → publish → flush. Movement retains the tick commit and per-vehicle collision-proxy refresh in game-ID order. Structural changes are immediate; there are no deferred Commands. The first error suppresses every later phase and is returned to the caller; this preserves partial-progress semantics rather than adding rollback.

## Behaviour and build evidence

- Full `cargo test --workspace --locked` passed, including 108 server library tests; existing ignored tests remain ignored. The suite covers lifecycle/reuse, deterministic traversal, replication/budgets, reconnect generations, combat, pursuit/officer fire/surrender, jobs, repairs, persistence/restart and actual transports.
- New driver tests exercise real phase order, once-per-tick execution, failure at every phase, suppression of later work, profile restoration, scratch cleanup, following-tick recovery and tick exhaustion. Metadata lifecycle tests cover attachment and final-component removal.
- Production `npm run build` passed, including native workspace, WASM and frontend. Bridge/protocol/content checks, TypeScript, ESLint and all 154 Vitest tests passed. Bevy remains confined to the server dependency tree.
- Rust formatting and diff whitespace checks passed. The repository-wide strict checks are not entirely green: the reviewed server Clippy run has 19 inherited findings and no new diagnostic categories compared with the baseline on Rust 1.95. A fresh strict invocation also stops on three inherited content-crate diagnostics. Global Prettier flags the untouched `level-editor.html` and `previous.html` files. These are recorded exceptions, not silently passing checks.
- Manual two-client browser smoke on disposable memory storage verified simultaneous joins, replicated movement/nametags, a pistol hit (ammo 12→11, hit confirmation, victim health 100→75), wanted pursuit, courier acceptance/cancellation, vehicle entry and driving (17 km/h). The final brake/exit observation was interrupted and is not counted. Detailed officer shooting/surrender, reward completion and repair persistence were verified by automated gameplay tests rather than manually completed browser scenarios.
- After restoring the normal SQLite development configuration, browser leave/rejoin returned to Ready. The native local probe passed complete baseline, StartPlay, nonce-matched WebSocket/WebRTC receipts and readiness HTTP 200 through the frontend proxy. The updated local server and production preview are running for user testing.

## Equivalent-load comparison

Baseline is commit `745b409`, built in an isolated source archive using the same Rust 1.95 compiler as the migrated server. Both use identical baked packages, 16 synthetic players, 90 measured ticks and 480 shots with memory profiles. Each map/variant had one warmup and seven alternating release runs on the same developer machine. Values below are medians of per-run measurements; RSS includes the test process.

| Scenario | Baseline p50 / p95 | ECS p50 / p95 | Baseline / ECS peak RSS |
| --- | --- | --- | --- |
| Regression map | 1.295 / 3.878 ms | 1.501 / 4.060 ms | 10.34 / 11.73 MiB |
| Full city with ambient actors/patrols | 1.588 / 4.398 ms | 1.848 / 4.660 ms | 14.53 / 15.53 MiB |

The full-city p95 increased by 0.262 ms (~6%), p50 by ~16%, and RSS by 1 MiB. ECS is not a speed improvement at this stage. Investigation found immutable vehicle templates being deeply copied in AI observations; sharing those templates through Arc reduced avoidable copying. Measurements remain comfortably below the 33.3 ms tick budget in this short synthetic workload. They do not establish 64-player capacity or replace a sustained production soak. See the [raw rounds and trace hashes](ecs-migration.json), including measurements before template sharing.

Captured entity states and outbound messages match the baseline exactly after replacing only world_id and session_id values with null. Message contents, game IDs, peers, delivery and ordering remain in the comparison: 9,118 records on the regression map and 9,278 on the full city. JSON egress is identical (1,313,906 and 1,320,031 bytes/s respectively). Raw traces remain temporary local artifacts; only counts and hashes are checked into the evidence file.

To reproduce the current workload:

```sh
cargo test -p server --lib --release --locked weapon_presentation_load_16_players -- --ignored --nocapture
PM_ECS_MAP=city cargo test -p server --lib --release --locked weapon_presentation_load_16_players -- --ignored --nocapture
PM_ECS_MAP=city PM_ECS_TRACE=/tmp/ecs-city-trace.json cargo test -p server --lib --release --locked weapon_presentation_load_16_players -- --ignored --nocapture
```

For baseline comparisons, archive the stated commit into a temporary directory, use the same compiler/content, and apply only the neutral map/trace instrumentation from the ignored test. Compile both release test executables before measuring to exclude compilation, alternate runs and use `/usr/bin/time -l` for peak RSS on macOS. Trace capture is separate from timing runs. These results qualify migration behaviour and local readiness; later system decomposition, capacity testing and remote deployment remain separate work.
