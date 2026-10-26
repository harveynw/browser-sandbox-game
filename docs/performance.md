# Performance targets and measured limits

[Documentation index](../README.md#documentation) · [Testing](testing.md)

The local functional game is verified. **Target-network performance, reference-hardware budgets and sustained memory stability are not qualified.** Excessive RTC delay and server memory growth remain open work for transport review. Independent full snapshots, one browser thread and the pinned transport remain deliberate defaults; no delta-compression/worker rewrite is assumed.

## Targets

| Metric | Target / scope |
|---|---|
| Frame time | p95 ≤22 ms and sustained ≥45 FPS on a documented reference laptop, 1280×720 Medium |
| Prediction/reconciliation CPU | p95 ≤2 ms/frame under target impairment |
| Full-population server tick | p95 ≤20 ms; p99 <33.3 ms on a documented 2-vCPU host |
| Encoded per-peer game traffic | Average ≤60 KiB/s down, ≤30 KiB/s up |
| Sixteen-player encoded egress | ≤960 KiB/s nominal sum |
| Cached join | ≤5 s on a reachable network |
| First playable package | ≤12 MiB compressed; download timing must state network speed |
| Browser memory | No sustained growth through ten-minute traversal and ten settled joins |
| Server memory | No sustained growth through a sixty-minute capped-population soak |

## Measurement surfaces

`GET /api/metrics` is a cached read-only loopback snapshot refreshed every thirty simulation ticks. It publishes bounded tick/system histograms, per-session encoded bytes, input counters, population/path work and application queue/close-reason statistics, omitting guest cookies/tickets/profile identifiers.

`tick_work` includes world work, control-mutex wait, queue sampling and metrics/status publication. `populated_tick_work` records completed ticks with sixteen allocated players; publication trails a completed tick. System histograms include all ticks since startup. `scheduler_poll_gap` is the main-loop poll gap, not the fixed simulation interval. Histograms retain 4,096 bins at 25 µs resolution; the overflow bin uses the observed maximum. Stage quantiles cannot be added together. Application queues do not expose the dependency's complete buffering.

Diagnostics → Performance capture records visible playing frames, retaining stalls and separately counting hidden/non-playing callbacks. Prediction CPU combines WASM steps, proxy preparation, reset/replay and input sampling between animation callbacks. Browser histograms retain 4,096 bins at 0.05 ms resolution with an overflow maximum. Memory samples are every ten seconds, capped at 720. JS heap is null if unavailable; WASM bytes, geometries, chunks and matrices are additional indicators, not total process/GPU memory. Stop to copy JSON.

## Capacity harness

The ignored `crates/server/tests/capacity.rs` test uses fifteen native peers plus one ordinary browser guest against a separately launched production server. Peers perform actual HTTP admission, WS signalling/reliable messages, RTC input, baseline checks and normal movement/fire/reload actions. They do not inject world state. Ambient/police directors run normally; four parked cars are authored and recovery slots remain reservations until jobs allocate them.

```sh
npm run preview:local -- --temporary
# Use a separate/private browser context for this database.
PM_CAPACITY_PROFILE=target PM_CAPACITY_SECONDS=3600 \
PM_CAPACITY_OUT=var/capacity.json \
cargo test -p server --release --test capacity --locked -- --ignored --nocapture
node tools/summarize-capacity.mjs var/capacity.json var/capacity-summary.json
```

The profile accepts `loopback` or `target`, duration 10–7,200 seconds (default 3,600). Optional `PM_CAPACITY_SERVER_PID` samples the supplied server's RSS, excluding the separate load process. The harness waits for sixteen players and checks that count at ten-second samples; it cannot exclude between-sample dips. Successful harness completion is not itself a performance/playability pass.

Target UDP impairment begins after StartPlay: nominal **50±10 ms per direction**, independent **2% loss**, **5% additional 40 ms delay** and **1% duplication**. Additional delay is a reorder opportunity, not guaranteed inversion. Directional loss seeds are `0x19c0ff01` / `0x19c0ff02`, with separate jitter/reorder/duplicate scheduling. Each relay caps queued datagrams at 512. WS and the browser observer remain loopback. Join times precede impairment; native peers do not implement the browser's complete predicted/resync loop. This is not a browser target-network prediction test.

Reports separate server-admitted encoded bytes, native application receipts and real forwarded UDP payload including DTLS/SCTP/STUN. IP/UDP headers and WS framing are excluded. The summary uses sampled elapsed intervals; RTT/owner-lag distributions use latest values sampled every ten seconds, not every ping. Bot displacement includes respawns and does not prove full-city traversal.

## Recorded local measurements

Recorded 25 October 2026 on **Apple M1 Pro, MacBookPro18,3, eight logical CPUs, 16 GiB, macOS 14.5 arm64**, release Rust 1.94 server. Server, browser, load generator and verification/build activity shared the host; this is not an isolated 2-vCPU/reference-laptop result. [Environment](evidence/capacity-environment.json).

| Measurement | Reliable-clock target dataset, 120.007 s | Owner-anchored stress dataset, 600.014 s |
|---|---:|---:|
| Sampled players | 16 | 16 |
| Tick p50 / p95 / p99 ms | 1.625 / 5.750 / 6.900 | 1.625 / 5.725 / 7.300 |
| Simulation ticks/s | 29.998 | 29.999 |
| Encoded egress KiB/s | 925.677 | 909.641 |
| Sampled RTC RTT p50 / p95 / p99 ms | 696 / 3,310 / 4,493 | 3,860 / 15,915 / 19,597 |
| Sampled owner-lag p95 ms | 3,200 | 15,400 |
| Server RSS first → last KiB | 23,584 → 28,832 | 24,720 → 42,224 |

[Target samples](evidence/capacity-target.json), [target summary](evidence/capacity-target-summary.json), [stress samples](evidence/capacity-stress.json), [stress summary](evidence/capacity-stress-summary.json). The stress dataset anchored input time to delayed owner samples, exaggerating stale input. The checked-in harness uses reliable Pong clock replies; the shorter target dataset reflects that policy. Duration/observer differences prevent direct tail comparisons. Both show excessive delay, so neither qualifies the target network.

The stress run observed 22–24 traffic, 32 pedestrians, eight police and four authored parked cars. Its per-peer encoded averages were 9.186–10.702 KiB/s up and 26.834–60.000 KiB/s down; maximum snapshot was 1,094 bytes. Forwarded native UDP payload totalled 129,855,996 up / 553,115,795 down bytes. Bounded application queues and low tick cost did not guarantee fresh socket delivery.

A [574.973-second browser capture](evidence/browser-capture.json) at 549×776 CSS pixels/DPR2 recorded frame p50/p95/p99 8.30/9.05/9.25 ms and prediction CPU 0.10/0.55/0.75 ms, with loopback networking and mixed movement/menu time. Heap varied about 18.8–48.3 MB and returned to 24.3 MB; WASM stayed at 1,507,328 bytes, with 38 geometries and 14–18 detailed chunks. Quality was not recorded. These numbers do not establish Medium reference performance, target-network prediction or continuous traversal memory stability.

[Ten rapid cached browser joins](evidence/cached-joins.json) took 1,791–1,871 ms on local content, at Low quality. They establish repeated admission, not settled post-GC memory stability. The [asset audit](evidence/asset-budget.json) is a per-file gzip estimate, not a timed network download.

## Unqualified work

Resolve growing RTC delivery delay and memory under impairment, then repeat a sixty-minute populated soak, continuous ten-minute browser traversal and settled join comparisons. Reference laptop/2-vCPU tests and first-play timing at a stated network speed remain unverified. Current findings remain available for net-crate review; they are not hidden by local functional acceptance.
