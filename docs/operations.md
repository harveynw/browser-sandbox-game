# Local setup, launch and operations

[Documentation index](../README.md#documentation) · [Persistent profiles](persistence.md) · [Verification](testing.md)

The supported deployment is a single local server with a Vite development client or locally served production bundle. No cloud account, public binding, domain, TLS proxy or NAT service is configured.

## First checkout

Install Node 24 with your preferred version manager. `.nvmrc`/`.node-version` recommend 24.19.0; the launcher accepts compatible Node 24 releases. Install Rustup and the pinned project tools explicitly:

```sh
rustup toolchain install 1.95.0 --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown
cargo +1.95.0 install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run dev:all
```

Open **http://localhost:5173/** after `[ready]` appears, choose Play, then Resume / mouse look. Ctrl+C in that terminal stops the server and frontend. Startup never resets saves or silently installs global toolchains. Cargo/npm builds can download their lockfile dependencies. The first native/WASM build can take several minutes; the twenty-second readiness deadline begins after building, when server launch starts.

`npm run setup` performs the same tool checks and build preparation without opening listeners. It generates protocol/content/bridge artifacts, builds release WASM and the frontend, and compiles the release server and native startup probe. `dev:all` and `preview:local` prepare automatically, so a separate setup invocation is optional. Build commands also work independently.

Original verified tools: **Node 24.22.1, npm 10.9.2, Rust 1.94.0, wasm-bindgen-cli 0.2.129**, macOS 14.5 arm64. Process-group supervision supports macOS/Linux; Linux has not been exercised on this machine. Windows supervision is explicitly unsupported; the launcher fails before spawning children there. Separate-terminal Rust/Vite commands remain available, subject to the project's existing platform dependencies.

## Commands

| Command | Behavior |
|---|---|
| `npm run setup` | Check installed tools/dependencies and prepare every required build artifact |
| `npm run dev:all` | Build and supervise release Rust server + Vite development server on 5173 |
| `npm run preview:local` | Build and supervise release Rust server + production Vite preview on 4173 |
| `npm run dev` | Frontend-only Vite flow; start Rust separately |
| `npm run config:test` | Generate a unique temporary SQLite config and print its path; does not start anything |
| `npm run test:local` | Explicit real-loopback launcher/process lifecycle tests |
| `npm run check` | Generated-code, WASM, typing, lint/format and Rust/TS suite |

Both launch commands accept:

```sh
npm run preview:local -- --temporary
npm run dev:all -- --config /absolute/path/to/server.toml
npm run preview:local -- --no-build
```

`--temporary` creates a distinct config/database in the operating system's temporary directory. Use a private/separate browser context for that test database: guest cookies belong to a particular database, and joining a different database through the same hostname/context can replace the original guest cookie even though the original save file remains intact. Its location is printed, retained after stopping for inspection, and never merged into normal saves. `config:test` creates the same config for manual Rust-server runs. `--config` chooses an existing persistence config; use it instead of `--temporary`. `--no-build` is an explicit reuse option after setup; it checks required files exist but cannot prove reused files are current. Omit it after changing source or dependencies. Ordinary launch rebuilds first.

## Startup and ownership

`tools/local.mjs` validates tools, arguments and TCP ports **3000**, **3001**, and **5173/4173** before launching children. An occupied port is reported with its number, OS error and an inspection command. It never kills a pre-existing process or falls back to another port. Permission failures are distinguished from occupied ports.

After building, it starts the server, waits for `/health/live`, starts Vite with the explicit client workspace root, then checks the proxied liveness route and frontend HTML. Vite and Rust bind only **127.0.0.1**. The recommended page hostname remains `localhost` for consistent guest-cookie scope.

The final readiness step runs `target/release/examples/local_probe`. It uses the frontend's HTTP proxy and normal guest/ticket admission, performs real net-crate WebSocket signalling and WebRTC exchange, verifies the complete baseline and StartPlay, obtains nonce-matched binary replies on **both** channels, reports those receipts through the existing local control API and requires proxied `/health/ready` **200**. It then sends Leave and closes its sockets. The example uses the pinned native WebRTC test client and ordinary admission endpoints. It does not test browser rendering or claim internet reachability.

The probe has its own named guest, **Local Startup Probe**, and a private cached cookie at `var/probes/<config-path-hash>.cookie` (created with mode 0600 on Unix). This avoids allocating a new durable probe profile at every normal launch. The ignored `var/` directory contains these local secrets; do not publish them. Probe cookies are independent of browser guest cookies. Startup probes briefly consume one player slot and leave before `[ready]` is printed.

`/health/ready` has a five-second receipt expiry. A successfully completed startup probe is a point-in-time check; readiness may expire while no real browser is playing. It is not a background probe consuming one of the sixteen player slots.

The twenty-second deadline covers server/frontend readiness and the real transport check, excluding compilation. Startup failure, an unexpected server/frontend exit, SIGINT or SIGTERM stops owned process groups. Shutdown allows eight seconds for graceful Ctrl+C handling/persistence flush, then escalates to termination and kill if required. Descendant process groups are included; the launcher never signals unrelated listeners. Logs use `[build]`, `[native]`, `[server]`, `[frontend]`, `[probe]`, `[ready]` and `[shutdown]` labels. Cookie/ticket payloads are not logged.

## Saves and two-player testing

Normal launch uses `config/dev.toml` and **`var/profiles.sqlite`**. Credits and completed jobs persist; player position, vehicle control, mission and heat do not. Restart creates a fresh city. Keep the same browser hostname/context to retain that guest's cookie. Use `--temporary` for disposable tests; do not delete normal saves to fix a launch failure.

Two tabs in the same context and hostname share a guest cookie and deliberately replace one another. Use a normal and private browser window, two browser profiles, or independent browser contexts for two simultaneous guests. Both connect to the same server. `localhost` and `127.0.0.1` also have separate cookie scopes, though separate contexts are clearer for testing.

For frontend-only work, `npm run dev` plus `cargo run -p server --locked` remains supported. These manually started processes are not owned by the supervisor and must be stopped in their own terminals before using a combined launcher. The local flow requires no cloud account, TLS proxy or STUN/TURN service.


## Health and diagnostics

- `/health/live`: ticking world and no observed transport boundary failure.
- `/health/ready`: live/available world and storage with recent actual WS/RTC receipts.
- `/api/build`: protocol, build, content and simulation hashes.
- `/api/metrics`: cached read-only timing, queue, population and encoded-byte counters.

Metrics/readiness are operator diagnostics, not world-mutation APIs. Diagnostics routes `/diagnostic`, `/simulation` and `/map` provide transport, shared-motor and content checks. See [testing](testing.md).

## Experimental remote playtest

See [deployment operations](../deploy/README.md) for the Ubuntu service and Caddy configuration. Optional `[network]` settings supply `public_origin` (an HTTPS origin without a path) and `webrtc_host` (a concrete local interface IP reachable by clients). Admission then restricts POST origins to that hostname, sets Secure cookies and returns its `/ws` WSS URL. Internal HTTP/WS remain loopback; RTC UDP binds the separately configured IP. Default configs retain local behavior. Public readiness requires actual browser WS/RTC receipts; localhost startup probes do not validate this mode.

The `ecs` migration branch now pins Rust 1.95.0 and server-only Bevy ECS 0.20.1. Install the toolchain using the commands above before building this branch, including on the Linux deployment host. Historical performance evidence remains labelled with its original toolchain.

## ECS stage 1 browser checkpoint

The server now schedules directly on the actor world; player movement uses native queries. Other gameplay phases retain a temporary exclusive adapter, and inter-system messages are a later stage.

Use `npm run dev:all` with Node 24 to rebuild and launch this checkpoint, then open http://localhost:5173/. Existing saves remain in `var/profiles.sqlite`. Suggested manual checks: join, walk and aim, enter/drive/exit a vehicle, fire a weapon, start a job, then leave and rejoin. The normal launcher verifies local transport startup; no before/after performance or trace comparison is required for this checkpoint.

## ECS stage 2 browser checkpoint

Vehicle and ambient pedestrian motors now join player movement as native ECS systems. AI input is read from police/traffic components, cars retain stable processing order with per-car collision refresh, and pedestrian overlap recovery runs after impact damage. Planning and combat consequences still use the temporary adapter.

Launch with `npm run dev:all` under Node 24 and open http://localhost:5173/. Check walking near traffic, entering/driving/exiting a vehicle, collisions with cars and pedestrians, and police pursuit. Normal saves are retained. This stage uses the existing gameplay tests plus focused native-motor ordering/failure tests and the normal local startup probe; no baseline comparisons are performed.

## ECS stage 3 browser checkpoint

Combat crime reports, noise and player-elimination announcements now use typed ECS messages and same-tick subscribers. Launch with `npm run dev:all` under Node 24 and open http://localhost:5173/.

Suggested checks: shoot near police and watch wanted response; shoot near pedestrians and watch them flee; collide with a police car or pedestrian; eliminate another player and verify one chat announcement in each browser. Use separate browser/private profiles for two players. The checkpoint retains normal SQLite saves and includes message-delivery, deduplication, failure-cleanup, gameplay and local connection checks without baseline comparisons.


### Vehicle impact message checkpoint

Vehicle movement now delivers contact results through ECS messages. Start normally under Node 24 and refresh/rejoin at http://localhost:5173/. Check crashes against walls and vehicles, repeated contact damage cooldown, pedestrian impacts, and police response after hitting a police vehicle. Previous-position snapshots and damage-before-overlap-recovery ordering are retained. Damage transactions still use the transitional exclusive adapter. Saves are unchanged.


### Native impact calculation checkpoint

Refresh/rejoin at http://localhost:5173/ after starting under Node 24. Test wall and vehicle crashes, repeated-contact cooldowns, pedestrian hits, and police response. Collision and damage calculations now run as native ECS systems; health and other gameplay consequences still run through the existing adapter. Contact damage is applied before pedestrian sweep calculations, preserving disabled-car behaviour. Saved profiles are unchanged.


### Native impact health checkpoint

Refresh/rejoin at http://localhost:5173/. Test wall and vehicle crashes, a fatal crash while driving, pedestrian impacts, and police response. Native ECS systems now apply impact health changes and attach disable/incapacitation cleanup components. Occupant protections and the first cleanup deadline are preserved; player/control consequences remain on the transitional adapter. Tests cover native health handling without legacy managers, cleanup timers, message consumption, and existing gameplay scenarios. Saves are unchanged.


### Impact consequence message checkpoint

Refresh/rejoin at http://localhost:5173/. Check visible/audible crash effects, a fatal crash while driving, respawn, and crashing during an active courier/recovery mission. Impact presentation, occupant recovery, and crash mission failure now have scheduled message consumers. Blocked exits retry without resetting respawn deadlines; control/visibility refresh remains behind the shared replication adapter. Tests cover blocked recovery and fatal-crash mission/epoch behaviour. SQLite saves are preserved.


### Completed impact flow checkpoint

Refresh/rejoin at http://localhost:5173/. In two browser profiles, check that a fatal vehicle crash ejects the driver, both players see the unoccupied destroyed car and downed character, and the driver respawns. Also try a crash during an active mission. Control replication now runs natively; stale inputs are cleared and fresh control/visibility versions prevent old packets restoring occupancy. Native two-observer tests cover reliable refresh ordering, no replay, stale identities, and counter exhaustion. Build the server and `local_probe` together before launching with `--no-build`.


### Pedestrian lifecycle checkpoint

Refresh/rejoin at http://localhost:5173/. Incapacitate a pedestrian by shooting or a vehicle impact, remain nearby, and check that the body disappears after roughly ten seconds. With two browsers, check that it disappears for both. Pedestrian expiry now despawns through native ECS Commands and removes its game-ID entry; existing replication handles client removal. Player respawn and vehicle restoration remain unchanged. Tests cover the exact expiry boundary, visibility to the next scheduled system, stale deletion safety, orphan cleanup, and gameplay expiry.


### Destroyed-vehicle restoration checkpoint

Refresh/rejoin at http://localhost:5173/. Destroy an ordinary parked vehicle, then keep its original parking spot clear: restoration becomes eligible after roughly 30 seconds. Standing near the spot should delay restoration until it is clear, with retries roughly once per second. Player respawn remains unchanged. Native tests cover exact deadlines, blocking, retry increments, one-time restoration delivery, and reservation release; the existing multiplayer blocked-parking scenario remains covered.


### Player respawn and protection checkpoint

Refresh/rejoin at http://localhost:5173/. Test a fatal crash and a PvP death: the player should return with full health/equipment, cleared mission/police state, and temporary protection. Check both browsers see the correct actor/control state and that firing ends protection as before. Blocked safe-spawn searches retry without rescheduling the deadline. Respawns now run after all firing for the tick. Keep RUST_BACKTRACE=1 on the local launcher to capture networking panics.


### Completed lifecycle checkpoint

Refresh/rejoin at http://localhost:5173/. Test PvP death, police gunfire, fatal vehicle damage, active-mission cancellation, respawn, temporary protection, and reload/equip completion. Death entry and weapon timers now use native ECS systems; firing and hit resolution remain the next migration stage. RUST_BACKTRACE=1 remains enabled on the local launcher. Native tests check stale/living death targets, duplicate requests, and no replay across separate schedule checkpoints, alongside the full gameplay suite.


### Automatic-fire intent checkpoint

Refresh/rejoin and test holding/releasing an automatic weapon trigger, reloading/equipping, and dying while firing. Eligibility and trigger-lease selection now run natively; hit resolution remains on the existing handler. Backtraces remain enabled.


### Automatic hit-tracing checkpoint

Refresh/rejoin and test automatic fire against players, pedestrians and vehicle cover, including targets behind walls and multiple targets in line. Automatic hit tracing now runs natively; current target eligibility is rechecked during ordered damage application. Manual and police fire retain their existing handlers.


### Shot presentation checkpoint

Refresh/rejoin at http://localhost:5173/. Test manual and automatic gunfire, hit feedback, and police gunfire from two browser clients. Shot effects now pass through a native ECS message consumer. Delivery tests cover the source outside observer range, a nearby observer, exclusion of a distant observer, stale messages, and no replay between checkpoints. Backtraces remain enabled.


### Complete weapons checkpoint

Refresh/rejoin at http://localhost:5173/. Test pistol and automatic fire, equip/reload, targets behind walls/vehicles, police gunfire, death and respawn. With two clients, check hit feedback and that a player killed before their queued shot does not fire. The complete weapons execution path now uses native ECS systems. Native regression coverage includes sequential damage visibility, duplicate actions, packet ordering, stale entity/epoch rejection, shot recipient selection and retirement. The next diagram stage is AI/planning. Keep RUST_BACKTRACE=1 when launching.


### Complete AI and planning checkpoint

Refresh/rejoin at http://localhost:5173/. Test traffic stopping/yielding, pedestrians crossing/fleeing, police pursuit and search, officer deployment/gunfire, surrender/arrest, and driver ejection/respawn. The AI/planning stage is now one native ECS checkpoint. Existing population, pursuit, officer and arrest regression tests exercise the same query algorithms used in production. Keep RUST_BACKTRACE=1 enabled.


### Complete missions and repairs checkpoint

Refresh/rejoin at http://localhost:5173/. Test courier completion/cancellation, time trials, recovery-car delivery and safe ejection, and repair dwell/payment. Check credits and mission status across rejoining. Regression tests include delayed debit refunds, duplicate requests, blocked recovery exits, committed rewards and stale/replayed reward messages. The next stage is sessions/player actions. The launcher retains RUST_BACKTRACE=1.


### Session lifecycle and possession checkpoint

Refresh/rejoin at http://localhost:5173/. Test joining/leaving, rejoining the same profile, entering/exiting vehicles, two players competing for one seat, and changing appearance. Existing regressions cover profile replacement, twelve recycled peer generations, timeout cleanup, recovery cleanup and resync freezing. Native possession coverage additionally rejects stale entities and confirms duplicate actions produce only one control transition. Backtraces remain enabled. Session protocol and replication adapters are the next stage.

### Complete replication and I/O checkpoint

Refresh/rejoin at http://localhost:5173/. With two browser clients, test joining, movement visibility, chat, vehicle entry/exit, changing appearance, leaving, and rejoining the same profile. Confirm the old connection closes on replacement and the other client removes the departing player. Resync/reconnect and profile persistence remain covered by automated scenarios. Output-failure regression coverage checks immediate actor retirement, observer EntityHide, and suppression of queued packets after a reliable budget failure. The live tick now uses native ECS queries/messages throughout; only startup/inspection/shutdown scaffolding remains. Keep RUST_BACKTRACE=1 enabled.


### Final ECS cleanup checkpoint

Refresh/rejoin at http://localhost:5173/. Test two-client joining, movement, vehicles, weapons, missions/repairs, leaving and rejoining. `/api/metrics` and `/health/live` retain their existing fields; detailed metrics still refresh once per second. Startup verifies all player/vehicle spawn geometry and initial patrols before opening the game loop. Graceful shutdown runs actor cleanup and bounded persistence draining; ordinary loop errors also attempt the drain. The full native ECS migration is complete; test fixtures retain isolated convenience wrappers. Backtraces remain enabled.


### Replication publication decomposition

Refresh/rejoin at http://localhost:5173/. With two clients, check moving players/vehicles, entering and leaving visibility range, the player map, private status, and reconnect/resync. Publication now uses separate scheduled responsibilities while retaining visibility at 30 Hz, owner/snapshot updates at 10 Hz, and periodic map/status at 1 Hz. Immediate action/lifecycle updates retain their existing timing. Backtraces remain enabled.


### Population decision decomposition

Refresh/rejoin at http://localhost:5173/. Check traffic driving, stopping/yielding and parking; pedestrians walking, crossing and fleeing after gunfire; and population appearing/recycling as you travel. Routing remains 30 Hz with four path jobs per tick; driving and pedestrian decisions remain 5 Hz. Backtraces remain enabled.


### Police driving decomposition

Refresh/rejoin at http://localhost:5173/. Check ordinary patrols, pursuit on foot and in a vehicle, multiple pursuing cars, officers deploying, and police recovering around obstacles. Goal selection, routing, steering, obstacle sensing and recovery now have separate functions inside the ordered police driving system. Backtraces remain enabled.


### Police perception decomposition

Refresh/rejoin at http://localhost:5173/. Check wanted heat after witnessed crimes, pursuit and losing sight behind buildings, stopping for arrest on foot/in a vehicle, and respawn. Visibility cadence, arrest dwell and safe vehicle-exit retries are unchanged. The checkpoint separates sensing and pursuit decisions while retaining the existing ECS arrest messages and consumers. Backtraces remain enabled.


### Repair transaction decomposition

Refresh/rejoin at http://localhost:5173/. Repair a damaged vehicle while stationary in a repair zone; verify the dwell, one payment and restored health. Try interrupting the dwell and leaving/rejoining. Existing regression scenarios cover delayed debit after disconnect, refunds, insufficient funds and duplicate/retried operations. Backtraces remain enabled.


### Final decomposition checkpoint

All six focused refactors are complete. Refresh/rejoin at http://localhost:5173/ for the final browser test. Exercise courier completion/cancellation, time-trial gates, recovery delivery and driver ejection, reward credits and rejoining; also check repairs, police pursuit/arrest, traffic/pedestrians and two-client visibility. Automated scenarios cover blocked recovery exits, reward replay/stale identity, recovery cleanup and durable profile restore. Backtraces remain enabled.


### Player lifecycle messages checkpoint

Refresh/rejoin at http://localhost:5173/. With two clients, check one join announcement, chat-history replay, one leave announcement, reconnection/profile replacement, and vehicles/police releasing a departed player. Resync and loading-only disconnects should not announce another join/leave. Native checkpoint tests verify pre-despawn subscriber access, duplicate suppression, stale readiness validation and message retirement.


### Chat participant checkpoint

With two clients at http://localhost:5173/, check chat history, the one-second sending limit, join/leave announcements and reconnecting. Resync keeps the current player's chat state; rejoining creates fresh state. Participant data is removed by entity despawn rather than a shared-map cleanup hook.
