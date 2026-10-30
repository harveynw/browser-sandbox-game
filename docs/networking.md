# Networking, admission and replication

[Documentation index](../README.md#documentation) · [Protocol](protocol.md) · [Performance limits](performance.md)

## Transport boundary

The server uses `harveynw/net-crate` at **a25516439f95d421d3dc752be1cbded8150c8805**, with its pinned WebRTC dependency retained in Cargo.lock. The server uses ICE-lite with the WebSocket listener’s loopback IP explicitly passed as its WebRTC host; peers bind ephemeral UDP ports on that IP without external STUN/TURN servers. The browser uses standard WebSocket/RTCPeerConnection signalling and full ICE against that server. WS carries signalling and reliable binary messages; the `game` RTC channel is unordered with zero retransmits. The application does not replace RTC with WebSocket-only gameplay.

`Network` owns the net crate handles, peer state and bounded application queues. The world drains lifecycle/messages, queues outgoing envelopes and requests disconnects directly. The game loop polls the net crate's events and flushes bounded application output directly; the net crate runs its own networking thread. Peer keys combine the crate's numeric ID and an application generation. Stale queued sends, closes and kills are ignored after generation replacement. The dependency's own untagged numeric events cannot be retroactively disambiguated by the application; see the limitations below.

## Local guest and session lifecycle

HTTP `/api/session` creates/reuses an HttpOnly guest cookie. Durable profile preparation can return **202**, then **200** after worker completion; the browser polls with a five-second cancellable deadline. `/api/join-ticket` issues a bounded, one-use, memory-only ticket with a **30-second** lifetime and the numeric-loopback WS endpoint.

The server sends ServerHello; the client checks protocol/build/content/simulation hashes and sends ClientHello with the ticket and display name. Committed profile/name loading completes before allocating the player and Welcome. AwaitHello → Loading → Playing is authoritative. Loading clients cannot move or invoke gameplay. A complete baseline, ClientReady and StartPlay precede input processing. The owner gets private status independently of public replicas.

A guest can have one active session. Same-hostname tabs share the cookie and replace one another. A new connection/session/world never accepts old scope or resumes possession, mission, heat or position. Disconnect releases owned cars, jobs and reservations. Resync is reliable, limited to one request per five seconds, and installs a fresh baseline/control epoch without changing guest identity.

## Simulation input and output

The world runs 30 Hz. It orders peers and entities deterministically rather than letting packet arrival order decide simultaneous ownership. Input for the next tick is consumed once; old epochs and late/far-future ticks are ignored and counted. The future window is bounded to next tick +8. Missing input is neutral.

Ingress uses **30 KiB/s**, a two-second burst, a strict **60 input batches per rolling second**, and a separate **10 actions/s, burst 20** limit. Excess input batches are discarded/counted so delayed legitimate bursts can recover; byte/queue limits still retire sustained floods. Actions retain server authority and idempotency.

Owner state is sent at **10 Hz**. Moving/occupied actors replicate at 10 Hz; unoccupied cars idle for a second use **2 Hz** updates. Spatial indexing uses 64 m cells, 160 m entry and 192 m retention radii, with self retention and a **96-visible-entity** cap. Mission relevance is handled by the world; nearest entities and overdue refresh age determine priority with stable ID ties.

Snapshots are packed by actual MessagePack byte size, including envelope scope. Each record is complete and never split across the **1,100-byte** application packet cap. Source ticks are independent per record, preserving same-tick packet splits. Omission never means deletion. Reliable visibility generations prevent stale packets from resurrecting hidden actors.

Egress uses **60 KiB/s** with a **120 KiB** burst per peer. Snapshot admission reserves **64 KiB** of that burst for baseline/lifecycle work. The transport also meters encoded bytes submitted after coalescing/repacking. Reliable exhaustion retires a session instead of emitting an incomplete baseline. Unreliable data can be dropped/coalesced safely.

## Application queues

| Bound | Policy |
|---|---|
| Peers | 16 |
| Lifecycle events | 32; overflow fails the boundary |
| Per-peer control ingress | 128 |
| Per-peer action ingress | 32 |
| Per-peer reliable output | 256 messages / 512 KiB |
| Pending snapshot records | Latest per entity/visibility, up to 96 |
| Other unreliable output | Independent latest owner, Pong and cosmetic streams |

Owner traffic is drained before snapshots. Baselines/control changes clear stale pending owner/snapshot state; hide removes matching records. A latest-only Pong is unsuitable for counting individual datagram delivery: loss tests use independent snapshot records instead.

## Client recovery

The client clock uses the lowest RTT among eight matching samples and half-RTT compensation; default lead is three ticks, then clamped to two through six. Reconciliation and replay are described in [simulation](simulation.md).

Remotes interpolate at estimated server tick minus three ticks, using shortest-arc yaw. They extrapolate velocity for at most three ticks, then hold; after 500 ms without a fresh sample they appear stale. Only a reliable hide removes them. Replica storage is bounded to 96 entries, 32 samples each and 256 tombstones. Mode/death/control discontinuities snap.

At 500 ms without authenticated traffic the HUD reports delay; at two seconds prediction/input stop; at ten seconds the session closes into retry UI. Long frames/backgrounding request resync rather than replaying unbounded backlog. Teardown aborts requests, clears input/history/replicas and releases WASM/graphics resources.

## Operational boundary

All listeners are loopback. Startup readiness requires actual admission, complete baseline, WS and RTC nonce receipts; a successful TCP bind alone is insufficient. [Operations](operations.md) describes the native probe and five-second readiness expiry.

Application bounds do not bound every dependency-internal queue or guarantee socket freshness. The crate's internal actor buffering, blocking sends, untagged numeric event reuse and limited shutdown/health APIs remain integration constraints. Its send and kill calls can block the game loop; transport health reports observed boundary failures and cannot detect an idle actor failure. Measured target impairment produces excessive RTC delay and memory growth; supported-network performance is **not qualified**. [Performance](performance.md) records the measurements for transport review.

Public/LAN serving, TLS, TURN/STUN, cross-network reachability and public authentication/abuse hardening are outside supported operation. Loopback success does not establish internet deployability. The authoritative gameplay and local protocol remain unchanged by that deployment boundary.
