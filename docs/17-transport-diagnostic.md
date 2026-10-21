# T03 — Local transport diagnostic

**Historical checkpoint:** T07 supersedes the placeholder admission and root-page controls below. Current launch/admission/probes are documented in [21 — Authoritative localhost sessions](21-authoritative-sessions.md). Transport binary signalling and limitations remain applicable.

This is a networking spike, with no world, game player, money, real profile, or authentication. It retains the exact pinned net-crate; no networking fork or substitution was needed.

## Launch and readiness

Use the pinned toolchains from README. In separate terminals:

```sh
cargo run -p server --locked
npm run dev
```

Open http://localhost:5173 and click **Connect**. The app waits for the server-created `game` channel (unordered, zero retransmissions), then exchanges ServerHello → ClientHello → diagnostic Welcome. It probes both binary paths once per second. **PASS** means this browser received matching MessagePack responses on both WS and RTC. A TCP port or constructor return alone never establishes RTC readiness. Open a second page and repeat; each connection must receive its own diagnostic session ID.

This browser page is the T03 loopback readiness probe. HTTP health/readiness endpoints, unattended readiness supervision, guest tickets and actual world baselines belong to T07. A probe failure must not be reported as ready. Server startup announces RTC readiness as UNPROVEN; it cannot infer delivery from queue admission.

The diagnostic uses `ws://127.0.0.1:3000` and loopback UDP ports allocated by net-crate. No STUN, TURN, TLS or public interfaces are configured. Stop each process with Ctrl+C. Restarting the server requires reconnecting the browser; no automatic retry or old-session reuse occurs.

## Controls and failure checks

- **Disconnect** closes both paths and cancels timers/listeners. **Connect** creates a fresh transport and handshake.
- **Test server close** requests an operation deliberately unavailable in T03. The server removes the diagnostic session and issues `kill`; the browser closes both transports. It does not rely on the crate reporting Closed after kill.
- Closing a tab or Vite HMR disposes the session. Server logs include peer ID, local generation and active session count, without payloads, SDP, tickets or credentials.
- Without a server, the page reports WS failure. If RTC never opens, the adapter closes at 15 seconds. Hello has a 5-second server deadline; an admitted diagnostic session expires after 10 seconds without a ping. Pending browser probe replies expire after 3 seconds.
- Try two pages, disconnect/reconnect the highest numbered peer repeatedly, and confirm the other page keeps passing. Reused crate IDs must receive new generations and session IDs.

## Implemented boundaries

`crates/server/src/transport.rs` owns a dedicated synchronous pump; gameplay never calls blocking net-crate handles. Application queues admit at most 16 peers, 32 lifecycle notices, 128 controls and 32 actions per peer. Reliable output is capped at 256 messages / 512 KiB per peer. Each pump pass sends at most 16 reliable messages per peer. Ingress drain rotates among peers. Malformed traffic or overflow stops admission and schedules local close/kill. Pre-open bytes are discarded without creating a session. Stale application PeerKeys cannot send to a reused crate ID.

Unreliable output currently retains **one latest diagnostic packet per peer**. Before T09 replication, replace this slot with bounded per-entity/owner coalescing; the current slot is not a general replication scheduler. The browser queues no application packets internally: it refuses RTC sends above 64 KiB buffered, and reliable sends above 256 KiB. A reliable stall lasting 2 seconds closes the connection. Three-frame input history belongs to the later prediction/input layer.

The temporary nonce convention is documented in [05](05-protocol.md). Pong now validates on both delivery channels. No schema shape or message variant was added. Every other existing delivery restriction remains enforced.

## Verification and limits

Automated tests exercise 15-second timeout, duplicate close, cleanup/unsubscribe, invalid binary data, extra RTC channel, ordered signalling, browser backpressure, reliable queue saturation, ingress saturation, coalescing, pre-open discard and stale generations. The shared Rust/TS corpus includes RTC Pong. These are deterministic boundary tests, not a claim that real ICE loss or dependency queue exhaustion was induced.

Live verification uses the actual Rust listener and two actual browser pages. Record results in the implementation log. Repeat PASS after a server-originated close and reconnect. No mock transport counts as evidence for the live gate.

The pinned crate still has internal unbounded queues, blocking/panicking sends, asynchronous bind failures, reusable internal IDs without generation tags, and no worker shutdown/join or health API. Our preflight bind detects ordinary occupied-port errors but cannot remove the race between preflight and the internal bind. The pump can detect its own panic, not every internal dependency failure. Drop signals pump shutdown without waiting indefinitely on a dependency blocking send. The future process supervisor must treat loss of end-to-end readiness as failure. See [04](04-net-crate-integration.md) for the remaining crate hardening work.

A local generation protects application work; it cannot repair an internally misattributed stale crate event. A few successful reconnects are not proof against every internal ID race. Selected ICE candidate pair statistics and cross-network/firewall experiments are not collected by this diagnostic and remain follow-up evidence before any public-hosting claim. Full session/gameplay admission and graceful service shutdown remain T07 work.
