# 35 — RTC loss regression repair

T19 is complete again following the post-v3 investigation on 24 October 2026. The failure was caused by incorrect assumptions in the diagnostic harness. The correction changes test code and a native regression test; it requires no net-crate fork, transport substitution or protocol revision. Network v3 and build `protocol-simple-local` remain current. T20 capacity/performance work has not started.

## What the failing test measured

The old probe sent fifty pings over RTC and counted distinct Pong replies. Our adapter deliberately stores only the latest pending Pong per peer. Delayed input arriving in a burst can therefore replace replies before UDP transmission. A direct no-network regression now demonstrates that fifty queued Pongs become one pending Pong, while fifty independent snapshot records survive the same adapter and packet budget.

Instrumentation confirmed the mismatch: all fifty probe requests reached the server, **26 Pongs were replaced deliberately**, and twenty unique replies appeared. Other runs fell below the threshold. Those counts measured application coalescing as well as network delivery, so they could not diagnose a codec/RTC failure reliably. [Instrumented old-Pong run](evidence/t19-pong-coalescence.txt).

The single global “drop every seventh datagram” schedule also coupled loss to the bidirectional DATA/acknowledgement cadence. One trace dropped seventeen client-to-server datagrams and zero server-to-client datagrams. Such a correlated pattern can suppress progress even though its aggregate loss fraction looks modest. This is useful as a stress case, but is not the independent packet-loss profile in the documented matrix.

## Corrected checks

- The independent-loss probe responds with one snapshot record per request, using distinct entity IDs 10–59. It exercises the actual bounded per-entity queue, MessagePack packet packing and real RTC/UDP path. It asserts zero replaced probe records and retains the original **more than fifteen of fifty unique replies** requirement within eight seconds. The threshold was not lowered.
- UDP loss uses separate xorshift streams for the two directions, seeded `0x19c0ff01` / `0x19c0ff02`, dropping with probability one in seven in each stream. A deterministic unit test checks repeatability, loss counts and independence from reverse-direction traffic. Handshake traffic is unimpaired; directional counters are sampled for the probe interval.
- The original global periodic schedule remains as a separate actual RTC stress test. It records progress without promising a delivery percentage under correlated loss. It requires bounded outstanding client data, a nonce-matched WS reply through UDP blackout, and a nonce-matched RTC reply within the ten-second receipt deadline after forwarding resumes.
- The healthy, independent and periodic profiles each retain five server-kill/reconnect cycles with recycled peer IDs and stale-generation isolation, plus a sixth peer saturating its configured 65,536-byte native client send limit. Pressure must reject the next send without exceeding that bound.

The snapshot responses are transport-test data, not injected live gameplay state. Queue coalescing in the production adapter is unchanged. The new native test locks down why a latest-only stream is unsuitable for individual-delivery counting.

## Commands and results

```sh
npm run check
cargo test -p server --test actual_transport --locked -- --ignored --nocapture --test-threads=1
npm run build
```

- Full check: **110 TypeScript / 134 Rust tests pass**, including drift checks, release WASM, strict typing, ESLint/formatting and Clippy with warnings denied. The default runner separately reports three ignored socket tests; it does run the new deterministic loss-schedule test.
- Explicit actual transport suite: **three tests pass**, healthy, independent seeded loss and periodic recovery. [Complete captured run](evidence/t19-rtc-repaired.txt).
- Independent-loss run: **43/50** unique records, fifty requests received, zero probe replacements and zero outstanding client bytes at the end. During the probe, client→server received/dropped/forwarded was **77/14/63**; server→client was **68/7/61**. No unknown relay source appeared during the probe. UDP blackout retained WS; RTC recovered afterward. Five reconnects and bounded send rejection also passed.
- Two further independent-loss runs passed with **49/50** and **48/50**, again zero probe replacements/outstanding bytes. [Repeat one](evidence/t19-rtc-repeat-1.txt), [repeat two](evidence/t19-rtc-repeat-2.txt).
- The retained periodic profile received **41/50** in the complete run and passed its bounded-send, blackout/recovery, reconnect and backpressure assertions. Earlier low-progress periodic traces remain part of the investigation; the stress test does not equate correlated loss with independent random loss.
- Full production build passes, retaining the existing Three.js chunk-size advisory. The pinned net/WebRTC revisions are unchanged. Verification used arm64 macOS 14.5, Rust 1.94 and the compatible bundled Node 24/npm 10 runtime.

Real timer scheduling and datagram counts vary even with fixed loss RNG streams. These runs establish local transport correctness and recovery, not a 16-player throughput, browser frame-time, reference-hardware or public-hosting result. Net-crate's opaque internal queues and numeric peer-event generation limitation remain documented in document 04. **T20 — Capacity and performance** is next.
