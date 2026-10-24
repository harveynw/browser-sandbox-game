# 04 — net-crate integration

[Index](../README.md) · Contract: [Protocol](05-protocol.md)

## Verified baseline

Source inspected on 20 October 2026: [commit 77b401b](https://github.com/harveynw/net-crate/tree/77b401b1376b5c8bcb98da2884efcb1e63082ba4). This document describes that checkout, not the older cached GitHub README. No crate tests or live browser connection were run while preparing this design.

**Current target: localhost.** Existing API integration, application bounds, and local failure handling are required. The public hardening list and cross-network experiments below are deferred; perform them only if public hosting is requested or a concrete crate bug blocks normal local play.

Use:

```toml
net = { git = "https://github.com/harveynw/net-crate", rev = "77b401b1376b5c8bcb98da2884efcb1e63082ba4" }
```

The dependency itself pins a Git revision of `webrtc`; preserve the resulting Cargo lockfile.

## Existing API, not proposed API

Verified in [server/mod.rs](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/server/mod.rs), [event.rs](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/event.rs), and [queue.rs](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/src/queue.rs):

```rust
let (mut server, mut events) = net::Server::new("127.0.0.1:3000");
let batch: Vec<net::Event> = events.pop_all();
server.send_reliable(peer_id_u32, bytes::Bytes::from(encoded_vec));
server.send_unreliable(peer_id_u32, bytes::Bytes::from(encoded_vec));
server.kill(peer_id_u32);
// broadcast exists, but game replication should use per-recipient sends.
```

`Event` has `Open(u32)`, `Closed(u32)`, and `Received(u32, Bytes)`. `Identifier` is defined as `u32` internally but is not re-exported from `lib.rs`; our adapter uses its own `PeerId(u32)`. `Server` and `EventQueue` are cloneable handles. Send methods take `&mut self`.

Important behaviors:

- Reliable application traffic is WebSocket **binary**. WebSocket **text** is JSON signalling.
- The server creates a WebRTC data channel named `game`, `ordered = false`, `max_retransmits = 0`.
- `Event::Open` follows the data-channel open event, not the TCP/WebSocket handshake.
- The received event does **not** identify the source channel. The game cannot enforce reliable-vs-unreliable ingress routing from this API alone.
- Binary WebSocket application messages can reach `Received` before `Open`; this is covered by the crate’s readiness tests. Discard them until the adapter has observed `Open`.
- WebSocket and RTC share a connection lifecycle. There is no supported WebSocket-only session.
- Each peer binds an ephemeral UDP port on the configured bind IP. TCP port 3000 does **not** imply UDP port 3000.
- The crate has no built-in TLS listener.

## Browser connector

Port the connection logic from the pinned [JavaScript example](https://github.com/harveynw/net-crate/blob/77b401b1376b5c8bcb98da2884efcb1e63082ba4/example_clients/javascript/client.js) into a typed adapter. Keep signalling distinct from our game codec.

1. Create `RTCPeerConnection({ iceServers: [] })` for loopback proof and a WebSocket with `binaryType = "arraybuffer"`.
2. Receive text `{sdp: string|null, candidate: RTCIceCandidateInit|null}` from the server.
3. Process text messages on one serialized promise chain. Apply offer with `setRemoteDescription`, create and set local answer, then send text `{type:"answer", sdp:answer.sdp}`.
4. Send local candidates as `{type:"ice", candidate:candidate.toJSON()}`. Ignore the null end-of-gathering marker.
5. Add remote candidates only after the remote description is applied. Preserve the promise chain even though WebSocket delivery itself is ordered.
6. Accept exactly the server-created `game` channel. Do not create another channel. Verify unordered delivery and zero retransmissions; set `binaryType = "arraybuffer"`.
7. Mark the adapter transport-ready once the data channel is open. Start game authentication only after receipt of `ServerHello` on WebSocket binary; this avoids the two endpoints’ open-notification race.
8. Reject unexpected text/data types, channel labels, malformed signalling, or negotiation taking > 15 seconds.
9. Any socket/channel terminal failure closes both, clears timers/listeners, and notifies the app exactly once.

No MessagePack message may enter the signalling parser. No JSON game message may be sent as WebSocket text.

## Application adapter and backpressure

Implement the interfaces in [Server interfaces](07-server-interfaces.md) and [Client interfaces](08-client-interfaces.md). A send “accepted” result means admitted to our queue, never guaranteed remote delivery.

Queues:

- Simulation → pump: reliable FIFO of at most 256 messages/512 KiB per peer; overflow disconnects that slow peer after local cleanup.
- Simulation → pump: replaceable latest unreliable record per `(peer, entity)` and owner state; at most one replication interval’s data. Do not queue stale snapshots.
- Pump → simulation: at most 128 decoded controls and 32 actions per peer, plus a small lifecycle queue. Apply per-peer work quotas so one connection cannot consume the tick.
- Browser: stop unreliable sends when RTC `bufferedAmount > 64 KiB`; retain only newest three input frames. Stop accepting new actions and disconnect with a clear reason if WebSocket `bufferedAmount > 256 KiB` for 2 seconds.

Application queues limit our allocations. They do not bound allocations made inside the current dependency. This remains a documented local limitation and a future public-release requirement.

## Known risks found in source

| Finding | Evidence | Handling |
|---|---|---|
| Public send/kill use `blocking_send(...).expect(...)` | `src/server/mod.rs` | Call only from dedicated synchronous pump; never directly in an async Tokio task or fixed tick |
| Several connection actor queues use `try_send(...).expect(...)` | `src/server/connection.rs` | Queue saturation can panic a task; upstream hardening required |
| `EventQueue` is an unbounded `VecDeque` | `src/queue.rs` | Frequent draining helps normal load; cannot defend pre-drain flood without crate changes |
| Kill removes connection before termination event is processed | `ActorMessage::Kill`, termination branch | Do local session cleanup immediately on our own kick; do not wait indefinitely for a promised `Closed` |
| Peer IDs can be reused | max-current-ID + 1 allocation | Track local connection generations and never equate transport ID with profile/entity ID |
| Bind occurs after constructor returns, on an internal thread | `Server::new` | Readiness needs an actual connection probe; constructor return is not successful listening |
| No application ingress channel tag | `Event::Received` | Validate by message kind and session phase; all messages must remain safe if sent on either channel |
| Binding uses `(bind_ip, 0)` for RTC, no public RTC config constructor | `src/server/webrtc/config.rs` | Internet topology needs a separate deployment spike and configurable RTC settings |
| Detailed binary/signalling events are logged | `connection.rs`, server event handling | Do not enable verbose dependency logs in release; remove sensitive payload logging before exposing public authentication |

Potential stale events around recycled peer IDs need stress tests. An adapter generation cannot fix an internal event misattributed to a newly recycled ID. Public release should use monotonic IDs or generation-tagged events in the crate.

## Required upstream/fork work before public alpha

These are **proposed additions**, not APIs assumed by the game:

1. Configurable RTC bind addresses, advertised/reachable candidates, and an explicit UDP range or multiplexed UDP socket if supported by the underlying library. Expose STUN/TURN configuration only after verifying support at the pinned `webrtc` revision.
2. Bounded ingress, max WebSocket frame/message size, max RTC application payload, handshake/signalling size limits, and handshake timeout before allocating unbounded resources.
3. Non-panicking send APIs with queue-full/closed results; coalesce or drop stale unreliable sends; propagate reliable failures and worker failure to the host.
4. Defined close semantics: exactly one terminal event, including explicit kill; no stale event can affect a later connection.
5. Readiness/startup failure and shutdown APIs. Consider ingress channel metadata, but game correctness must not rely on it.
6. Pre-upgrade origin/admission validation or a reverse-proxy admission check, plus removal/redaction of payload logs.

Implement these in a separately reviewed revision of net-crate or a clearly tracked fork retaining the crate. Update this document, the pinned revision, and adapter tests together. Do not silently replace it with another networking library.

Local controlled testing can proceed before these changes. A public “ready” claim cannot.

## Acceptance spike

- Actual Rust server plus two real browser pages, not a mock WebSocket server.
- Round-trip non-UTF-8 MessagePack bytes over each channel; establish that only `game` exists.
- Send application data before `Open` and verify no player/session is created.
- Join, leave, kill, reconnect, failed ICE, duplicate close, stalled receiver, full queue, and rapidly reused IDs.
- Readiness fails if TCP bind or RTC initialization fails.
- Future public-hosting gate only: repeat on two machines and on two different networks using the proposed public topology.
- Record selected ICE candidate pairs and UDP firewall rules, with credentials redacted.

The final cross-network result is an experiment, not something that can be inferred from loopback success.

## T03 implementation checkpoint

The exact pinned crate now powers the localhost Rust listener and typed browser adapter. Two actual browser pages passed concurrent WS/RTC MessagePack probes; local reconnect, recycled peer IDs, and server-originated kill/reconnect also passed. The existing dependency did not require a fork. See [the diagnostic guide](17-transport-diagnostic.md) and [implementation log](implementation-log.md) for evidence and the distinction between live checks, deterministic boundary tests, and remaining dependency limitations. The original design-time observations above remain valid.


## T19 regression checkpoint

The unchanged pinned crate now has an explicitly run native localhost regression through the real game adapter and WebSocket/RTC peer, with an actual UDP loss/blackout relay, configured test-peer send-buffer pressure and five server-kill/reconnect cycles using recycled IDs. Native world tests also inject old-generation close/data across twelve rejoins. [Coverage and captured evidence](33-impairment-and-lifecycle.md) distinguish these checks from application scheduler tests and from the dependency's still-opaque internal queues and untagged numeric events. Passing this local stress sample does not prove internal races absent or make a public-release claim.
