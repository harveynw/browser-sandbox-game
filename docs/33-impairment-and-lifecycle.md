# 33 — Impairment and lifecycle regression

T19 completes local regression coverage on 24 October 2026. Browser/server build IDs are `t19-local`. Network version 2, content, WASM bridge and SQLite schema remain unchanged. This milestone adds failure tests and session hardening; T20 owns the 16-peer populated capacity and soak measurements.

## Running the suites

Use the documented Node 24 and Rust toolchains:

```sh
npm run check
cargo test -p server --test actual_transport --locked -- --ignored --nocapture
npm run build
```

The first command runs **240 TypeScript / 135 Rust tests** and compiles the actual transport test. The real socket test is explicitly ignored by the default native runner because it binds loopback TCP/UDP; the second command runs that additional test. It uses ephemeral ports and in-memory diagnostic envelopes, never the running game's profile database. A ten-second deadline bounds negotiation/individual receipts. No external network, HTTP throttling, substitute game server or release test endpoint is involved.

## Deterministic delivery

`packages/protocol/tests/support/impairment.ts` supplies a seeded scheduler with bounded pending storage, reliable FIFO, RTC loss, jitter, delay/reordering, duplicates and bursts. `apps/client/src/world/impairment.test.ts` runs all six profiles from document 11 against real MessagePack encode/decode and `ReplicaStore`, using separate same-tick packets for two actors. Seed is `0x19c0ffee`; each run covers 600 authoritative ticks, hide/re-entry with fresh visibility, late traffic, stale presentation and an old-visibility future-tick packet. Repeating a seed produces the same delivery order and counters.

| Profile | RTT / jitter | RTC loss | Reorder / duplicate | Result exercised |
|---|---|---|---|---|
| Loopback | 2 / 0 ms | 0 | 0 / 0 | Independent split snapshots advance |
| Broadband | 50 / 10 ms | 1% | 1% / 1% | Loss does not remove unrelated records |
| Supported | 100 / 20 ms | 2% | 5% / 1% | Bounded, finite presentation and visibility |
| Stressed | 200 / 50 ms | 5% | 10% / 0 | Delayed records cannot resurrect hidden actors |
| Severe | 500 / 100 ms | 20% + 300 ms bursts every 3 s | 20% / 2% | Bounded presentation; stale state holds |
| Disconnect | no delivery | 100% on both paths | — | Existing actors become stale; no queued catch-up |

These application tests establish protocol/lifecycle invariants, not subjective smoothness. Existing `packages/bridge/tests/prediction.test.ts` additionally drives real WASM motors and serialized three-frame input batches for 1,800 ticks under supported/stressed delivery, with correction thresholds, bounded history and no conflicting input application. Frame-schedule and vehicle tests retain 30/60/144 Hz equivalence, car entry/exit epochs and old-owner rejection.

## Regression inventory

| Required regression | Reproducible coverage |
|---|---|
| Snapshot before spawn, hide/old snapshot, fresh visibility on re-entry | `world/replica.test.ts`, six-profile `world/impairment.test.ts` |
| Same-tick split, intervening loss, entity-specific freshness | Replica tests, `server/tests/replication.rs`, native transport split-snapshot test, six-profile scheduler |
| Old input/owner epochs after car entry/exit | `bridge/tests/vehicle.test.ts`; world `simultaneous_entry…` / `exit_reclaims_player…`; T19 twelve-rejoin scenario |
| Duplicate reliable action; stale session on reconnect | World cached action/mission/combat scenarios; `bridge/tests/session.test.ts` ignores wrong world/session and closed adapter callbacks |
| Ten-second render pause; no giant catch-up or held control | Real-WASM session test requires fresh complete baseline/epoch, stops input batches during resync; input tests cover blur, visibility and pointer-lock cleanup; native missing-input neutralization tests |
| Slow receiver / burst sender / bounded queues | Browser RTC pressure and WS stall/recovery tests; signalling flood bound; native reliable/control/lifecycle bounds; actual UDP blackout plus native test-peer send-buffer saturation |
| Pre-open binary, missing hello, invalid ticket, load timeout | Native `transport::tests::preopen…`, world `loading_hello_and_playing_timeouts…` / `unknown_peers…`; browser session inert-loading timeout and async cancellation |
| Kill / rapid reused peer ID / late close/data | Twelve native world rejoins with stale-generation close/data, five actual server-kill reconnect cycles with stale-generation sends/kills |
| Baseline ordering/resync | Session tests reject premature StartPlay, missing self, count mismatch, wrong/null baseline show, overlap and duplicate show; server resync baseline/visibility/epoch tests |

## Fixes established by the tests

The browser previously allowed `StartPlay` while a baseline was still open, accepted an untagged live show inside a baseline and had no monotonic baseline guard. It now requires completion with the right count/self, rejects overlapping/replayed baseline begins, and invalidates readiness when requesting resync. A closed session ignores captured late callbacks. The shared simulation and server authority rules are unchanged.

The asynchronous browser signalling chain could retain arbitrarily many messages behind a stalled SDP operation. It now accepts at most 32 pending signals and 65,536 characters per signal, rejects overflow before retaining another message and ignores completion after close. Tests also cover repeated RTC pressure without application queuing and reliable-pressure recovery before the stall deadline.

## Actual UDP/RTC evidence

The test-only peer in `server/tests/support/rtc.rs` uses the same pinned WebRTC implementation as net-crate. It negotiates WebSocket signalling and the real unordered `game` data channel. A loopback UDP relay rewrites both sides' advertised candidates and forwards actual ICE/DTLS/SCTP datagrams. Once negotiation succeeds it drops every seventh datagram, then blocks UDP entirely while a nonce-matched WebSocket echo succeeds. A nonce-matched RTC echo after restoring forwarding proves recovery. Matching nonces avoids treating previously buffered pongs as recovery.

Recorded on **arm64 macOS 14.5**, Rust 1.94, the unchanged net revision `77b401b1376b5c8bcb98da2884efcb1e63082ba4`:

- **30/50 RTC receipt windows** under every-seventh-datagram loss; **34 dropped / 197 forwarded datagrams** across the impaired probe. These are datagram/application observations, not an RTT or packet-loss percentage benchmark.
- WebSocket remained usable during UDP blackout and RTC resumed afterward.
- Five server-originated kill/reconnect cycles recycled peer ID 0 with generations 1–5. Stale-generation output returned Closed and a late old-generation kill did not close the replacement peer.
- A sixth peer with a configured **65,536-byte test-client send limit** queued **655 packets / 65,500 outstanding bytes** under blackout, then returned `ErrSendBufferFull`. It was closed after saturation. This verifies real RTC backpressure for that configured peer, not the server dependency's opaque queues.

A final production-browser smoke joined the matching SQLite-backed build, retained 150 credits/one completed job, moved normally and completed an explicit Resync world. At epoch 3 it showed WS 26 / RTC 26 receipts, 0.000 m correction p95 and zero pending/fallback chunks. [Live resync](evidence/t19-live-resync.png).

[Captured actual transport run](evidence/t19-actual-transport.txt). Datagram counts can vary with real scheduling; the test asserts progress, actual drops, recovery, bounded outstanding bytes and generation isolation rather than requiring an exact count.

## Limits and next gate

Application generation tags protect the adapter/world boundary. The pinned dependency's own events carry only a numeric peer ID; a falsely attributed internal stale event cannot be distinguished by this adapter. The rapid real reconnect test passed, but does not prove all internal races impossible. Its opaque internal queues and blocking send remain documented in document 04; no dependency fork or public hardening claim is introduced.

The six-profile scheduler and real UDP relay complement each other: the complete timing matrix is deterministic application delivery; the actual transport run proves UDP loss/blackout, recovery, configured RTC pressure and kill/reconnect behavior. It is not a six-profile real-network performance benchmark or a browser compatibility matrix. Browser UI/first-job gameplay evidence remains recorded in T17/T18; this milestone's real transport regression is native. No new two-browser combat/job playthrough or subjective network-feel assessment is claimed. T20 must measure the actual 16-player populated shard, target impairment, frame/tick/byte/memory budgets and soak duration.
