# Local setup, launch and operations

[Documentation index](../README.md#documentation) · [Persistent profiles](persistence.md) · [Verification](testing.md)

The supported deployment is a single local server with a Vite development client or locally served production bundle. No cloud account, public binding, domain, TLS proxy or NAT service is configured.

## First checkout

Install Node 24 with your preferred version manager. `.nvmrc`/`.node-version` recommend 24.19.0; the launcher accepts compatible Node 24 releases. Install Rustup and the pinned project tools explicitly:

```sh
rustup toolchain install 1.94.0 --profile minimal --component rustfmt,clippy --target wasm32-unknown-unknown
cargo +1.94.0 install wasm-bindgen-cli --version 0.2.129 --locked
npm ci
npm run dev:all
```

Open **http://localhost:5173/** after `[ready]` appears, choose Play, then Resume / mouse look. Ctrl+C in that terminal stops the server and frontend. Startup never resets saves or silently installs global toolchains. Cargo/npm builds can download their lockfile dependencies. The first native/WASM build can take several minutes; the twenty-second readiness deadline begins after building, when server launch starts.

`npm run setup` performs the same tool checks and build preparation without opening listeners. It generates protocol/content/bridge artifacts, builds release WASM and the frontend, and compiles the release server and native startup probe. `dev:all` and `preview:local` prepare automatically, so a separate setup invocation is optional. Build commands also work independently.

Verified tools: **Node 24.22.1, npm 10.9.2, Rust 1.94.0, wasm-bindgen-cli 0.2.129**, macOS 14.5 arm64. Process-group supervision supports macOS/Linux; Linux has not been exercised on this machine. Windows supervision is explicitly unsupported; the launcher fails before spawning children there. Separate-terminal Rust/Vite commands remain available, subject to the project's existing platform dependencies.

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
