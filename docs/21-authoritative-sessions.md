# 21 — Authoritative localhost sessions and character world

[Index](../README.md) · [Protocol](05-protocol.md) · [Server design](07-server-interfaces.md) · [WASM bridge](20-wasm-bridge.md)

T07 replaces the T03 placeholder admission. Rust now owns guest admission, players, character motors, input scheduling and lifecycle. T08 now provides the root third-person walkthrough; the scripted session diagnostic is at `/diagnostic`. T09 now supplies prediction/reconciliation, interest management and remote snapshots; see [23 — Prediction and replication](23-prediction-and-replication.md).

## Run

Use README's pinned tools/dependencies, then two terminals:

```sh
cargo run -p server --locked
npm run dev
```

Open [localhost:5173/diagnostic](http://localhost:5173/diagnostic), click **Connect**, and expect a real Welcome, baseline, StartPlay, private status and advancing owner ticks. **PASS** also requires matching pong receipt on both WS and RTC. **Movement probe** schedules 30 forward inputs over one second and logs the displacement reported by the server; it does not change client positions. **Resync baseline** requests another readiness handshake and control epoch. Disconnect removes the actor. Reconnect creates a fresh session/entity; the memory guest profile persists only until process exit.

Two tabs share the localhost guest cookie, so the second valid join replaces the first. To test two simultaneous players, use separate browser profiles/private contexts. Same-guest replacement is expected and explicitly displayed. The map and `/simulation` inspectors remain independent of the server.

Production preview is `npm run build`, then `npm run preview --workspace @port-meridian/client`, with the Rust server still running. Both Vite modes proxy `/api` and `/health` to the control plane and preserve the browser's Origin. Keep the browser origin `localhost`, not numeric loopback. The returned game socket uses numeric loopback intentionally.

`cargo run -p server --locked -- --check-content` verifies published files, simulation config/collision and all 16 spawn slots without binding listeners. It never rebakes content. No configuration flags other than this check are currently supported.

## Runtime ownership

| Module | Responsibility |
|---|---|
| `server::content_loader` | Read immutable package index/manifest/assets with budgets, verify every hash, run native package semantic validation, construct simulation-safe content. |
| `server::admission` | OS-random guest secrets/profile IDs, hashed-secret lookup, one-use expiring tickets, bounded in-memory profiles, active-session registration and local probe reports. |
| `server::control` | Bound loopback HTTP issuer, strict local Origin/Host checks, health/build endpoints and diagnostic probe receipt reports. |
| `server::world` | Monotonic entity IDs, ordered players/sessions, admission phases, fixed-step input selection, baselines, ownership/status, lifecycle, timeouts and cleanup. |
| `server::scheduler` | Monotonic accumulator, at most five catch-up steps, excess wall-time discard and persistent-overload admission gating. |
| `server::transport` | Existing net-crate pump, bounded decoded ingress/output, local generation keys and close requests. Only this pump calls net-crate methods. |
| Client `net/diagnostic.ts` | Load verified content/WASM, acquire cookie/ticket, negotiate transport, validate baseline and scope, acknowledge readiness, inspect actual owner messages and probe both binary channels. |

`World::step(transport, profiles, now_ms, allow_joins)` drains at most 128 events, processes terminal events before gameplay, admits messages in stable peer-key/FIFO order, steps playing characters in ascending entity ID, synchronizes lifecycle, emits owner state every third tick, and flushes bounded output. Tests use FakeTransport with the **real server encoder/decoder** on every output. The world does no socket/file/profile disk I/O; admission's memory lookup is immediate and bounded. A disk-backed asynchronous profile worker remains T18 work.

The host passes monotonic milliseconds for deadlines; simulation advances only by `sim_core::DT_SECONDS`. Catch-up never consumes extra frames per actor or skips simulation tick IDs. Three consecutive excess-backlog iterations mark overload and reject new admission/tickets until recovery. T07 does not implement the future whole-game module graph (NPCs, vehicles, money, missions or collision damage).

## Content and spawn authority

The server reads `apps/client/public/content/slice.json` and its hash-addressed package. Index is capped at 4 KiB; manifest/assets at 2 MiB each; aggregate package at 16 MiB. Absolute/traversal asset references and repeated asset names fail. All referenced asset bytes and manifest canonical hash must match. The existing native content validator checks collision, graph connectivity, clear reservations and visual/collision agreement. Simulation settings and BVH construction are additionally validated before either listener becomes ready.

The first authored spawn anchors a 4 × 4 grid of 2 m-spaced character slots inside its clear plaza. All 16 positions are capsule-checked at boot. Slots are allocated independently from monotonically increasing entity IDs and released on cleanup. Loading actors remain completely inert. Playing actors use the same static collision/config and shared character motor already tested in T05/T06. There is no player-to-player or vehicle collision yet.

## HTTP endpoints and identity

Rust binds WS/RTC to `127.0.0.1:3000` and HTTP to `127.0.0.1:3001`. HTTP binds synchronously; the WS adapter retains its preflight bind. Ordinary conflicts on either port terminate startup with failure; no ready announcement is printed. The net-crate internal asynchronous-bind race remains a documented dependency limitation.

| Endpoint | Behavior |
|---|---|
| `GET /api/build` | Actual protocol version, `t07-local` build ID and baked content/config hashes. |
| `POST /api/session` with `{}` | Reuse a valid guest cookie or create a memory profile; return profile defaults. |
| `POST /api/join-ticket` with `{}` | Require guest cookie; return one-use ticket, 30,000 ms expiry and WS URL. A newer ticket invalidates this guest's earlier unconsumed ticket. |
| `POST /api/transport-probe` | Require active guest/session plus `{session_id, ws:true, rtc:true}`. Record a trusted localhost browser's actual receipt report for five seconds. |
| `GET /health/live` | 200 only while world ticks are recent and the application pump is healthy. |
| `GET /health/ready` | 200 only while live, not overloaded, and the localhost browser receipt report has not expired. Includes tick/session/player/loading/playing counts. |

All responses are uncached JSON. POST requires a Content-Length bounded to 1 KiB, `application/json`, exact allowed Origin and local Host; no wildcard CORS is enabled. Allowed browser Origins are the exact localhost and 127.0.0.1 origins on ports 5173, 4173 and 3001. T09 adds the literal IPv4 loopback variants so two hostname-scoped guests can be demonstrated in one browser. Other Origins remain rejected; no wildcard CORS or non-loopback listener was added. Cookie is `pm_guest`, HttpOnly, SameSite=Strict, path `/api`; Secure is deliberately absent on localhost HTTP. Secrets are 256 random bits, only their SHA-256 lookup hashes are retained. Tickets are also 256 random bits, stored hashed, consumed once and scoped to this process/shard. Profile/session/world IDs are distinct random strings; entities are separately allocated nonzero integers. No secrets are logged.

Memory caps are 256 profiles and 64 pending tickets; expired tickets are pruned during issuance. Transport/world cap is 16 connections including AwaitHello and Loading. Loading players reserve an actor slot. There is no account provider, durable profile, payment or economic mutation in this stage.

Readiness is a deliberate **local development probe**, not unattended public monitoring. Startup remains unproven until a browser has received both pongs and reports that observation through the guest-authenticated HTTP endpoint. The diagnostic refreshes it only while receipts are fresh. Bootstrap ticket issuance is allowed before readiness so this probe can join. A local guest can fabricate a report; this is an operator diagnostic, not a security boundary. A process supervisor should use an independent trusted probe before public deployment. No claim is made that net-crate exposes internal worker health or ingress channel metadata.

## Admission, lifecycle and output

```text
Opened -> AwaitHello (5 s)
valid one-use ticket + matching build/content -> Loading (30 s)
Welcome -> BaselineBegin -> EntityShow(s) -> BaselineEnd
matching ClientReady -> StartPlay -> PlayerStatus -> Playing
ResyncRequest -> epoch + 1, neutralize velocity, Loading/new baseline
Leave / timeout / close / output failure / replacement -> immediate world cleanup
```

Welcome does not mean the actor can move. Only a matching ClientReady starts play; duplicate readiness cannot start twice. Baseline count is frozen and sent through one reliable FIFO. Every observer visibility cycle gets a fresh increasing ID, including resync; baseline IDs/epochs also increase. Lifecycle changes append after the baseline. T07 makes all current player descriptors visible, bounded by 16, without implementing AOI yet. Character movement is sent to the owner at 10 Hz. T09 adds relevance-filtered remote snapshots and browser interpolation.

The initial private PlayerStatus follows StartPlay in the reliable FIFO and contains only that owner's memory profile. Unreliable owner messages may arrive between those two reliable deliveries. Status defaults are clear pursuit/heat, 12-round magazine and no mission/reload/respawn/protection timer. Gameplay actions return an explicit Unavailable (or phase/epoch/request error); a bounded 32-result cache prevents duplicate requests from becoming new operations. No action grants money or vehicle ownership.

Resync clears queued inputs, increments epoch, zeroes velocity and freezes the actor until new readiness. Baseline IDs/visibility counters never reset within a session. Resync requests are limited to one accepted request per second. Old-epoch inputs cannot move the actor after resync.

Cleanup removes the actor, slot, session, input/action buffers and active profile/session registration immediately, emits observer hides, then requests transport closure. Delayed Closed and old-generation messages are harmless at the application boundary. One guest's newer valid join first removes the old ownership/session; an invalid ticket cannot replace it. The old peer receives a best-effort replacement Disconnect before kill.

The pump drains already admitted reliable output before kill so Reject/Disconnect can enter the actor FIFO first. Closure stops new ingress/output immediately. Application cleanup does not wait for a crate Closed event or for delivery confirmation. Unreliable output has four fixed stream slots (owner, snapshot, pong, cosmetic), so a same-tick owner update cannot overwrite a probe pong. This bug was found in live guest replacement and preserved as a regression test. T09 must still replace whole-snapshot coalescing with bounded per-entity replication scheduling.

## Input admission

Before step S, admit only a Playing session's matching epoch frames with `S ≤ tick ≤ S+8`. Buffer at most those nine distinct ticks. Already simulated frames are counted/discarded; further-future frames are counted/rejected. Identical retransmitted ticks do nothing; conflicting values for an already buffered tick are counted/rejected while retaining the first frame.

The server applies **one** selected frame for S or shared-core neutral input retaining current facing. Packet volume, redundancy and omitted frames cannot advance simulation time. Missing input brakes movement; jump is not reconstructed. `consumed_through_tick` equals current server tick, including neutral gaps. World/session scope is checked before mutation. This per-frame admission deliberately accepts usable frames from a three-frame redundant batch even when its older frames are late; the all-or-nothing T02 helper alone is not the authoritative batch algorithm.

## Verification and limits

Native tests cover inert loading, readiness/output ordering, duplicate/skipped/future/conflicting/old-epoch frames, bounded rings, resync, timeout classes, replacement/reconnect generations, observer lifecycle, full-output cleanup, request caching, all 16 reserved slots, tickets/cookies/origins, readiness expiry/overload, scheduler catch-up and runtime content tampering/path/budgets. The transport regression retains both an owner update and pong in the same tick.

Live checks exercise real net-crate WS/RTC admission, server motor movement and braking, resync, same-guest replacement, disconnect/reconnect and both port conflicts. See the implementation log for observed results. `npm run check` includes all native checks plus the existing TS/protocol/content/release-WASM suite; `npm run build` produces the packaged production client.

T08 builds the walkthrough against these admission/owner interfaces through the reusable `net/session.ts` GameSession. It displays direct authoritative owner samples without a second local authority. T09 adds replay rings, clock/prediction/reconciliation, remote replication and bounded byte admission/coalescing. Vehicles, missions, SQLite and public hosting remain later work.

The local HTTP worker is intentionally small; it is not a public abuse-resistant HTTP service. Slow-body and HTTP queue hardening, unattended readiness supervision, dependency-internal queue/failure handling and bounded graceful worker shutdown remain later operations/transport work. With memory-only profiles, Ctrl+C ends the process and discards all profile/world state; no durability or transaction-flush claim is made.
