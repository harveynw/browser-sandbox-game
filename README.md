# Port Meridian

Implementation specification for an original GTA-inspired, third-person, multiplayer 3D browser game. The server is authoritative Rust and uses [harveynw/net-crate](https://github.com/harveynw/net-crate). The browser uses TypeScript, Three.js, and a shared Rust movement library compiled to WebAssembly.

**Status: local functional acceptance and handoff complete (T22); T20 performance remains deferred by user. Protocol v3.** Play at [localhost:5173](http://localhost:5173/): WASD/arrows, Shift sprint, Space jump, mouse look, Esc menu. On foot: RMB / Q aim, left click fire, R reload. If pointer lock is declined, drag the scene to look. Characters use an original articulated rig with four clothing palettes, with simpler distant silhouettes and interpolated server snapshots; local movement uses the shared Rust/WASM motor with authoritative reconciliation. Press E at a car’s left door to enter; WASD drives, Space applies the handbrake, and E exits when stopped. Find the cyan Quay terminal and press E / F to accept the courier job. Follow the gold route to Market, stop inside the delivery ring for one second, and earn 150 credits. At a cyan terminal, F opens job choices; the north ring trial requires driving. In the green Ironworks repair zone, stop and press F for a 50-credit repair. Esc opens the job cancellation menu. Tools remain at `/map`, `/simulation`, and `/diagnostic`. See the [implementation log](docs/implementation-log.md), [netcode guide](docs/23-prediction-and-replication.md), [client guide](docs/22-browser-walkthrough.md), [content guide](docs/18-content-pipeline.md) and [server guide](docs/21-authoritative-sessions.md). See the [courier demonstration](docs/25-courier-slice.md) for the verified delivery and screenshots. The networking revision remains `77b401b1376b5c8bcb98da2884efcb1e63082ba4`.

The [full-city guide](docs/26-full-city.md) records navigation, streaming, provenance, budgets and verification. The [ambient population guide](docs/27-ambient-population.md) covers traffic, pedestrians and pool limits. The [combat guide](docs/28-pistol-and-damage.md) covers aim/fire/reload, damage and safe respawns. The [police guide](docs/29-police-and-heat.md) covers witnessed crimes, fair pursuit, search, arrest and respawn. The [jobs and repairs guide](docs/30-jobs-and-repairs.md) covers time trials, reserved vehicle recovery and queued repair payments/refunds. The [local profiles guide](docs/31-local-profiles.md) covers SQLite saves, restart recovery and repair compensation. The [presentation guide](docs/32-presentation-and-ux.md) covers animated characters, vehicle detail, gesture-gated sound, the M city map, remappable controls, quality and accessibility settings. The [impairment guide](docs/33-impairment-and-lifecycle.md) maps lifecycle regressions, seeded delivery profiles and the real UDP loss/backpressure test. The [protocol simplification](docs/34-protocol-simplification.md) replaces the runtime schema/scanner pipeline with direct Serde/MessagePack and records the existing loss-test failure reproduced with the previous codec. The [RTC regression repair](docs/35-rtc-loss-regression.md) corrects that harness issue and records passing loss, blackout recovery, backpressure and reconnect tests. The [capacity guide](docs/36-capacity-and-performance.md) records the populated benchmark, instrumentation and measured input-burst fix. G4 remains incomplete; the user has deferred network-performance work to revisit their net crate. The [local-launch guide](docs/37-local-launch.md) covers setup, supervised development/production preview, real startup checks and clean shutdown. The [final handoff](docs/38-local-acceptance-and-handoff.md) records two-guest production gameplay, saved rewards across restart, police escape, the repaired patrol replacement bug and the exact gate/compatibility limits.

## Run the walkthrough

Prerequisites: Rustup and **Node 24** (24.19.0 is recommended in `.nvmrc` and `.node-version`; Node 24.22.1/npm 10.9.2 were used for T21 verification). The project pins Rust **1.94.0** in `rust-toolchain.toml`, including rustfmt, Clippy, and the `wasm32-unknown-unknown` target. The combined launcher verifies that the project toolchain is already installed and prints install guidance if it is missing; it does not change the global default. Use your Node version manager to select the pinned Node version before installing dependencies; Node 23 is unsupported.

```sh
rustup toolchain install 1.94.0 --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown
cargo +1.94.0 install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run dev:all
```

Open [localhost:5173](http://localhost:5173/) after `[ready]` appears, choose **Play**, then **Resume / mouse look**. This command builds and supervises the Rust server plus Vite, verifies real WS/RTC receipts and stops both on Ctrl+C. `npm run setup` prepares the same artifacts without listeners. For a locally served production build use **`npm run preview:local`** and open [localhost:4173](http://localhost:4173/). The combined commands require macOS/Linux process groups; macOS is verified. Stop existing separate-terminal servers/previews before launching; occupied ports are reported, never taken over.

Use `npm run preview:local -- --temporary` for an isolated test database; its path is printed and retained after stopping. Normal play uses `config/dev.toml` and `var/profiles.sqlite`. Saved credits survive restarts. Two tabs share a guest cookie and intentionally replace each other; use separate normal/private browser contexts or profiles for two simultaneous guests. Keep the same hostname/context when returning to saved play.

WASD/arrows walk, Shift sprints, Space jumps, Esc opens the menu. If mouse capture is declined, hold the left mouse button and drag to look. The world continues while paused. **Diagnostics** shows prediction, RTT, correction, entities, rendering and encoded traffic, with a bounded performance capture and **Resync world** control. Tools remain at `/map`, `/simulation`, and `/diagnostic`.

| Command | Current behavior |
|---|---|
| `npm run setup` | Verify installed tools; build generated protocol/content/bridge, WASM, frontend, release server and real startup probe |
| `npm run dev:all` | Prepare, supervise server + Vite on 5173, verify HTTP/WS/RTC, clean up on Ctrl+C/failure |
| `npm run preview:local` | Same lifecycle with the production client build on 4173 |
| `npm run config:test` | Generate and print a disposable SQLite test configuration |
| `npm run test:local` | Seven real-loopback/process supervision checks |
| `npm run dev` | Frontend only: generate bridge/build WASM, then Vite on 5173; run Rust separately |
| `npm run check` | Generated drift, strict TS, lint/formatting, 111 TS tests (including release WASM/seeded netcode), Clippy, and 138 Rust tests (actual socket regressions run separately) |
| `npm run build` | Generate contracts/content, compile native Rust/package WASM, type-check, bundle frontend |
| `npm run sim:trace` | Replay the native 60-second character trace against 13 saved checkpoints |
| `npm run build:wasm` | Package release WASM + web JS/declarations using pinned wasm-bindgen CLI |
| `cargo test --workspace --locked` | Run the workspace Rust scenario, protocol, simulation and content tests |
| `cargo run -p server --locked` | Start authoritative WS/RTC on 127.0.0.1:3000, local HTTP on 127.0.0.1:3001 and SQLite profiles |
| `npm run protocol:generate` / `protocol:check` | Generate contracts/fixtures or detect drift without modifying files |
| `npm run content:generate` / `content:check` | Generate content contracts or detect contract/baked-package drift |
| `npm run bridge:generate` / `bridge:check` | Generate separate local bridge TS/schemas or detect drift |
| `npm run content:bake` | Validate/bake city and regression slice, generate contracts and publish both immutable indexes |
| `npm run content:audit` | Report city downloads and managed-buffer budgets after building |

Protocol tests now run in both languages; the temporary zero-test allowance is removed. The release WASM bridge is packaged with wasm-bindgen-cli 0.2.129; wasm-pack is not needed. Local bridge DTOs are generated separately from network envelopes. Content baking is implemented; runtime authoring lives in `content/source/city.json`; the slice remains a regression fixture. The pinned net-crate dependency is active. The localhost issuer uses HttpOnly guest cookies and expiring one-use tickets backed by SQLite profiles by default (`config/dev.toml`, `var/profiles.sqlite`). Committed credits/job counts survive restart; world state does not. Use `--config config/memory.toml` for disposable profiles. [21 — Authoritative sessions](docs/21-authoritative-sessions.md) documents the interfaces and limits.

Simulation usage: [19 — Native character simulation](docs/19-native-character-simulation.md) and [20 — WASM bridge](docs/20-wasm-bridge.md). Open [localhost:5173/simulation](http://localhost:5173/simulation), click **Run trace**, and expect PASS for 1,800 ticks / 13 checkpoints. The browser and production bundle agree with native checkpoints within 1 cm. No game server is required for this diagnostic.

The map inspector needs only `npm run dev`: open [localhost:5173/map](http://localhost:5173/map). Expand its layers panel to compare collision, reservations and navigation. The Rust server is needed for the walkthrough and `/diagnostic`. Readiness at `/health/ready` stays 503 until a real client (the startup probe or a playing browser) reports receipt on both channels, expires after five seconds without fresh reports, and is explicitly a trusted localhost probe.

## Acceptance scope

Verified locally on the in-app Chromium browser (Chrome/156 UA), including Medium at 1280 × 720, real driving courier completion, private police heat/search/escape and exactly-once saved reward after server restart. Standalone Chrome/Edge/Firefox/Safari and subjective audio quality remain unverified here; native Chrome access was not approved. G4 network/performance/reference-hardware/soak results remain deferred, not passed. See the final handoff for the evidence and limits.

## Start here

The [frontend and backend module diagrams](docs/diagrams/README.md) show the current implemented architecture. Update the affected Mermaid source alongside changes to modules, ownership, threads or significant runtime interactions.

Read the documents in this order. Implement one task from the implementation plan at a time.

| Document | Purpose |
|---|---|
| [01 — Product and game design](docs/01-game-design.md) | Scope, player loop, controls, mechanics, progression, exclusions |
| [02 — City and content](docs/02-world-and-content.md) | Measured map layout, streets, collision, missions, assets, authoring |
| [03 — Architecture](docs/03-architecture.md) | Stack, repository layout, ownership, processes, dependencies |
| [04 — net-crate integration](docs/04-net-crate-integration.md) | Verified crate API, browser signalling, hazards, required hardening |
| [05 — Protocol and shared types](docs/05-protocol.md) | MessagePack contract, exact message shapes, generation, validation |
| [06 — Simulation and netcode](docs/06-simulation-and-netcode.md) | Fixed ticks, physics, prediction, reconciliation, interest management |
| [07 — Server interfaces](docs/07-server-interfaces.md) | Modules, function contracts, tick order, authority, persistence |
| [08 — Client interfaces](docs/08-client-interfaces.md) | Rendering, camera, input, network state, WASM bridge, UI |
| [09 — Gameplay systems](docs/09-gameplay-systems.md) | Vehicles, combat, NPCs, police, missions, economy state machines |
| [10 — Art, audio, and UX](docs/10-art-audio-ux.md) | Visual direction, asset budgets, animation, HUD, accessibility |
| [11 — Testing and performance](docs/11-testing-and-performance.md) | Automated checks, network scenarios, playtests, measurable gates |
| [12 — Local deployment and operations](docs/12-deployment-and-operations.md) | Local launch, saves, debug metrics, shutdown, and reset |
| [13 — Implementation plan](docs/13-implementation-plan.md) | Ordered tasks, exact outputs, dependencies, acceptance criteria |
| [14 — Agent handoff](docs/14-agent-handoff.md) | Copyable implementation prompts, stop conditions, reporting |
| [15 — Decisions and sources](docs/15-decisions-and-sources.md) | Rationale, alternatives, unresolved experiments, source references |

Optional future reference: [16 — Public hosting](docs/16-future-public-hosting.md). This is outside the requested localhost scope.

## Fixed decisions

- One 16-player shard; one compact 768 × 768 m city; desktop keyboard and mouse first.
- First playable slice: two players, one 256 × 256 m district, walk/run/jump, enter/exit a car, drive, complete one delivery.
- Complete alpha: city exploration, three car classes, delivery/time-trial/vehicle-recovery jobs, modest traffic and pedestrians, a three-level police response, one pistol, session money, guest profiles.
- Rust owns wire DTOs. Generate TypeScript declarations and JSON Schemas; use MessagePack maps with `rmp-serde` and `@msgpack/msgpack`.
- 30 Hz simulation and input generation; 10 Hz replication; independent render frequency.
- Share movement code through Rust/WASM. Predict only the locally controlled actor. Authority, missions, damage, NPC decisions, and money remain on the server.
- Initially use full state records with no snapshot-delta dependencies. Add optimizations only after measuring.
- Localhost is the required deployment target. The pinned crate is the baseline; public hosting, TLS/TURN, and public transport hardening are future work, not completion gates for this project.

## Specification rules

“Must” is an acceptance requirement; “target” is a benchmark goal; “later” is outside the complete-alpha scope. Values in the owning document override summaries elsewhere: document 05 owns the wire format, 06 owns simulation timing, 02 owns map measurements, and 13 owns delivery order. Change an owning document and its affected tests together.

Code examples in the design documents describe **interfaces to implement**; the current commands above describe the scaffold that exists. Commands in later plan tasks must be implemented before those tasks are reported complete. Multiple browser contexts on one machine will demonstrate local multiplayer once implemented; access from other machines is optional future work.

Do not expand the project into an MMO, a realistic vehicle simulator, a full GTA recreation, or a proprietary asset extraction project. Use original names, map geometry, and licensed or original assets.
