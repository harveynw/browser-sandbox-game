# 12 — Local deployment and operations

[Index](../README.md) · Optional later: [Public hosting](16-future-public-hosting.md)

## Required deployment

Run everything on the developer’s computer. This is the completion target selected by the user. No cloud account, domain, certificate, TLS proxy, TURN service, container orchestration, router forwarding, or public UDP configuration is required.

```text
Browser: http://localhost:5173
  assets -> Vite
  /api and /health -> Vite proxy -> 127.0.0.1:3001 HTTP control plane
  game socket -> ws://127.0.0.1:3000
  WebRTC -> 127.0.0.1:<ephemeral UDP port chosen by net-crate>
Rust server -> local content files and optional var/profiles.sqlite
```

Use the real net-crate transport even on localhost. A local game must still exercise client-server serialization, signalling, input authority, and replication. Multiple browser contexts connect to the same process and world.

Bind listeners to `127.0.0.1`, not `0.0.0.0`. Do not enable STUN/TURN or change router settings. The crate’s default host candidates are the intended loopback path; confirm through the actual two-browser spike.

T07 now implements the HTTP/Vite-proxy topology above and real memory guest admission. See [21 — Authoritative localhost sessions](21-authoritative-sessions.md) for the exact available commands and explicit browser-reported readiness gate. T17 implements persistence-only local config, SQLite profiles and bounded shutdown; setup/dev:all supervision and unattended probes remain later work.

## Required developer commands

These are interfaces for T01/T21 to implement, not commands already available:

```text
npm ci
npm run setup              verify toolchains; build generated protocol/WASM/content
npm run dev:all            supervise Rust server + Vite; stream labelled logs
npm run dev               Vite alone
cargo run -p server -- --config config/dev.toml
npm run build
npm run preview:local     built client plus API proxy, while local server runs
```

`dev:all` starts the server first, waits for readiness with a 20-second bounded timeout, then starts Vite. If either child exits unexpectedly, stop its sibling and print the failing process/port. Ctrl+C closes both and flushes persistence. Never leave orphan listeners after a failed launch.

The setup command checks required tools and explains missing ones; it must not silently install global toolchains or modify system configuration. Record the tested Rust stable toolchain, Node LTS version, wasm target, and WASM packaging tool version in the project README when implemented. Commit lockfiles.

## Local configuration

The implemented `config/dev.toml` selects persistence; the existing bindings and gameplay limits remain loopback-only constants:

```toml
[persistence]
mode = "sqlite"
path = "var/profiles.sqlite"
```

Relative paths resolve from the repository root. `cargo run -p server --locked -- --config config/memory.toml` selects disposable memory storage. Unknown keys/modes and missing SQLite paths fail startup. General network/auth/gameplay configuration remains outside this milestone. See [31 — Local profiles](31-local-profiles.md).

Keep browser origin consistent: use `localhost` in the page URL and the explicit numeric loopback socket endpoint supplied by `/api/join-ticket`. Using the dev proxy keeps guest cookies same-origin. Ensure Vite preview proxies the same API paths; do not serve the built page with `file://`.

Full local alpha uses `config/dev.toml` above. The slice is a native regression fixture; `config/memory.toml` changes persistence only, not the city or player limit. Tests choose temporary data paths and available ports; they must not overwrite the developer’s local profile database.

## Local guest identity

Use the same session interfaces as document 07: a local guest cookie, short-lived one-use join ticket, and per-connection session ID. Cookie `Secure` is false only in localhost config. No external identity provider or account creation UI.

Two tabs in the same browser context share the guest cookie and therefore replace the previous session by design. For two simultaneous players, use two browser profiles, a normal plus private context, or Playwright contexts. Explain this in the launch README and test UI; do not mistake intentional replacement for failed multiplayer.

Keep origin checks and wire validation in local development so the architecture is consistent. Pre-auth internet abuse resistance is deferred; do not advertise this listener publicly.

## Health and logs

- `/health/live`: process supervisor alive.
- `/health/ready`: valid content, persistence worker ready, complete local net-crate probe succeeded.
- `/api/build`: exact build/protocol/content/sim hashes.
- `/metrics`: loopback-only debug metrics.

`Server::new` returning is not proof that its listener bound. A complete transport probe catches the pinned crate’s background startup errors. On local port conflict, fail launch with the port and process name, rather than leave the browser loading.

Logs should contain tick and system timings, session lifecycle, action rejections, and errors. Do not log ticket/cookie payloads or all raw network messages. A local debug overlay shows RTT, tick, correction distance, entity count, draw calls, and bandwidth.

The pinned crate’s queue/panic limitations remain documented. Bounded application queues and tests are required. If one prevents ordinary 16-client localhost operation, fix that concrete issue in a tracked crate revision. Do not make the entire public-hosting hardening program a prerequisite to implementing the local slice.

## Persistence and reset

Full local alpha stores profiles and economic events in `var/profiles.sqlite`; add `var/` to gitignore. Use SQLite WAL with one worker. Migrations run before ready and fail explicitly.

`npm run reset:local` may be added only as a clearly named, explicit destructive command. It must stop/check the server first, show the exact local database path, and require a command flag such as `--confirm` before deleting saves. Ordinary startup/build/test must never reset saves.

Restart creates a fresh city/world ID and restores only committed profile balances/job counts on join. No vehicle possession, active missions, heat, or position persists. A consistency test must show one reward remains once after restart.

## Local completion checklist

1. Fresh checkout setup instructions produce a running app without machine-specific absolute paths.
2. One command launches server and client and reports their URLs/ports.
3. Two independent browser contexts join through actual WS/RTC and see each other.
4. Walk, drive, deliver a parcel, quit/rejoin, and restore saved credits.
5. Run 15 synthetic peers plus a real browser observer, with the populated city, keeping the total at 16 players.
6. Stop/start cleanly; no orphan listeners, stale sessions, or duplicate rewards.
7. Production frontend build runs locally through `preview:local`.

A pass does not imply internet deployability. Public configuration and its extra checks are preserved separately in [the future hosting appendix](16-future-public-hosting.md).
