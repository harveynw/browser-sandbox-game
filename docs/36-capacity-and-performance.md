# 36 — Capacity and performance

T20 now has a reproducible populated benchmark and timing/byte/memory instrumentation. **G4 remains incomplete:** the target impairment run exposes excessive RTC latency, and reference-hardware and soak evidence is still missing. On 25 October 2026 the user explicitly deferred network-performance work to revisit their own net crate and authorized proceeding to T21. Network v3, build `protocol-simple-local` and the pinned net/WebRTC dependencies are unchanged.

## Measurement surface

`GET http://127.0.0.1:3001/api/metrics` returns a cached, read-only snapshot once per 30 simulation ticks. It includes bounded timing histograms, six world stages, encoded game-byte counters per session, input rejection/drop counters, population/path work and application queue depths. It omits guest cookies, tickets and profile identifiers. The listener remains loopback-only.

`tick_work` includes world work, waiting for the control mutex, queue sampling and metrics/status publication. `populated_tick_work` counts only completed ticks with 16 players; reporting necessarily trails by one completed tick. Per-system histograms include all ticks since startup. `scheduler_poll_gap` measures main-loop polling gaps, not the simulation interval. Timing quantiles are conservative 25 µs bucket bounds; values above 102.375 ms use the observed maximum. Histograms keep 4,096 bins rather than raw samples. Dependency-internal queues remain opaque.

In **Diagnostics → Performance capture**, start a capture, play, then stop to obtain copyable JSON. Visible playing frames include stalls; hidden/non-playing callbacks are counted separately. Prediction CPU combines WASM stepping, proxy preparation, reset/replay and input sampling between successive animation callbacks. Browser histograms have 4,096 bins at 0.05 ms resolution, with an overflow maximum. Memory sampling is every ten seconds, capped at 720 samples; JS heap is null where the browser does not provide it. WASM, geometries, resident chunks and instance matrices are additional indicators, not total browser/GPU memory. Captures can span leave/rejoin cycles on the same page.

## Real populated load

`crates/server/tests/capacity.rs` connects fifteen native clients through actual HTTP admission, WebSocket signalling/reliable messages and the pinned native WebRTC implementation. They join the production net-crate server, validate baselines, send 30 Hz redundant input, walk authored pedestrian routes and issue legitimate fire/reload actions. They never inject positions or bypass gameplay authority. Add one ordinary browser guest to reach sixteen players. The authored population director runs normally: up to 24 traffic cars, 32 pedestrians and 8 police. Four ordinary parked cars are authored; eight recovery slots are reservations, not pre-spawned cars. Damage/recycling can temporarily reduce ambient counts.

The relay activates impairment after StartPlay. `target` applies nominal 50 ±10 ms delay in each UDP direction, independent one-in-50 datagram loss, one-in-20 extra 40 ms delay and one-in-100 duplication. Extra delay is an injected reorder opportunity, not a guarantee that two received packets reverse order. Loss RNG seeds are `0x19c0ff01` / `0x19c0ff02`; a separate generator supplies jitter/reorder/duplication. Scheduling is reproducible statistically, not bit-for-bit in wall time. Queued delayed datagrams are capped at 512 × 65,536 bytes per relay. The browser observer and WebSocket path remain loopback; this does **not** establish browser prediction under target impairment.

The harness requires sixteen players at admission and every ten-second sample, records early failure as incomplete and stops peers on measurement exit. Between-sample population dips are not excluded by polling. Reports distinguish server-admitted encoded bytes, native received application bytes, and real UDP payload bytes including DTLS/SCTP/STUN. UDP/IP and WebSocket framing overhead is not included. Optional server RSS sampling uses the explicitly supplied PID; it does not include the separate load-generator process.

Use a separate SQLite database, preserving the normal `var/profiles.sqlite`. Stop the normal server gracefully before binding its ports. With the production preview already on localhost:4173:

```sh
cargo build -p server --release --locked
run_dir=$(mktemp -d /tmp/pm-capacity.XXXXXX)
cat > "$run_dir/server.toml" <<EOF_CONFIG
[persistence]
mode = "sqlite"
path = "$run_dir/profiles.sqlite"
EOF_CONFIG
./target/release/server --config "$run_dir/server.toml"
```

In another terminal, run the load test, then join with one browser:

```sh
PM_CAPACITY_PROFILE=target PM_CAPACITY_SECONDS=3600 \
PM_CAPACITY_OUT=var/capacity.json \
cargo test -p server --release --test capacity --locked -- \
  --ignored --nocapture
node tools/summarize-capacity.mjs var/capacity.json var/capacity-summary.json
```

`PM_CAPACITY_PROFILE` accepts `loopback` or `target`; duration accepts 10–7,200 seconds and defaults to 3,600. Set `PM_CAPACITY_SERVER_PID` to the server PID to collect RSS. Baseline/join traffic occurs before impairment. Cached native join times therefore measure local signalling/admission, not impaired asset loading. After testing, leave the browser, stop this server with Ctrl-C and restart the normal configuration.

## Failure found and correction

Two early target-profile runs lost a peer after roughly twenty seconds. Disconnect counters identified **input rate exceeded**, rather than capacity exhaustion. RTC congestion released more than sixty legitimately generated 30 Hz batches within one server rolling second.

The server still accepts at most sixty batches per rolling second. Excess batches now increment `rate_limited_inputs` and are discarded without retiring the session. Scope, epoch, tick-window, input-ring, transport ingress and byte-budget limits remain enforced. A delayed-burst regression verifies the discarded future frames never enter simulation and fresh controls resume. A separate sustained-byte-flood regression still requires actor cleanup and a bounded input ring. The original test assuming the 61st batch must disconnect was updated to this explicit policy. This correction prevents a false-positive disconnect; it does not cure RTC congestion.

## Evidence and remaining gates

Measurements and verification results are recorded below after the completed run. Do not interpret a successful harness exit as G4: the harness checks population and lifecycle, while the acceptance budgets and observed latency require separate assessment.

### 25 October 2026 local measurements

Hardware: **Apple M1 Pro, MacBookPro18,3, eight logical CPUs, 16 GiB RAM, macOS 14.5 arm64**. The browser observer was the in-app Chromium browser reporting **Chrome/156.0.0.0**, viewport **549 × 776 CSS pixels**, DPR **2**. Server used the Rust 1.94 release profile; city seed was **424242**. Server, browser, native load generator and verification/build work shared the Mac. These measurements are not a 2-vCPU-host result or the 1280 × 720 reference-laptop test. [Environment and source fingerprint](evidence/t20-environment.json).

The initial 30-second loopback preflight admitted sixteen players and ran without failure. Two early impairment runs reproduced the burst-triggered disconnect; [initial failure](evidence/t20-target-before-fix.txt), [diagnostic rerun](evidence/t20-burst-diagnosis.txt). The subsequent **600.014-second** target-profile stress measurement retained sixteen allocated players at every sample, all native clients remained connected, and the browser visibly played throughout its capture. Traffic varied **22–24**, pedestrians **32**, police **8**, authored parked cars **4**. [Full samples](evidence/t20-target-600s.json), [summary](evidence/t20-target-600s-summary.json), [run log](evidence/t20-target-600s.txt).

| Measurement | Observed result | Acceptance interpretation |
|---|---|---|
| Populated server tick p50 / p95 / p99 | **1.625 / 5.725 / 7.300 ms**, max 91.797 ms | Local CPU budget looks viable; reference host unverified |
| Actual simulation cadence | **29.999 ticks/s** | No sustained tick-rate collapse in this interval |
| Encoded aggregate egress | **909.641 KiB/s** | Below nominal 960 KiB/s in this interval |
| Per-peer encoded up / down averages | **9.186–10.702 / 26.834–60.000 KiB/s** | Within 30 / 60 KiB/s average budgets |
| Maximum encoded snapshot | **1,094 bytes** | Below 1,100-byte application packet limit |
| Sampled native RTC RTT p50 / p95 / p99 | **3,860 / 15,915 / 19,597 ms**; max 20,641 ms | **Unacceptable delay under nominal 100 ms impairment** |
| Sampled native owner lag p50 / p95 / p99 | **3,400 / 15,400 / 19,000 ms** | Application queue freshness does not guarantee fresh socket delivery |
| Server RSS | **24,720 → 42,224 KiB**, peak 42,704 KiB | Growth observed; 60-minute stability not established |
| Application queue failure / conflicting input / future input | **false / 0 / 0** | No observed application-boundary failure |
| Rate-limited / late input counters | **216 / 621,327** | Confirms degraded input delivery; this is not a playability pass |

Per-system p50/p95/p99 since startup: admission/ingress **0.075/0.125/0.275 ms**, planning **0.375/1.100/1.550**, motors **0.725/0.950/1.575**, gameplay **0.050/0.075/0.100**, replication **0.200/3.025/3.825**, output flush **0.025/0.875/1.550**. Quantiles across stages are not additive. Replication is the largest measured p95 CPU component, but RTC freshness is the immediate blocker, so no speculative delta-compression/worker rewrite was introduced.

Native relay forwarding over the measured interval totalled **129,855,996 upstream / 553,115,795 downstream UDP payload bytes** across fifteen peers. These are real socket datagrams with transport payload overhead, excluding IP/UDP headers and the observer/WS path. Latest native RTT/owner-lag values are polled at ten-second intervals; the table is a distribution of those sampled values, not every ping. The ten-minute stress harness still anchored input generation to delayed owner samples, exaggerating stale input. The checked-in harness now anchors input time from reliable Pong replies, like the browser's use of its best clock path; a separate follow-up is recorded below. The stress run remains useful socket/capacity evidence, not a representative predicted-client pass. Bots attempt route following; displacement includes respawns and does not prove every bot traversed the city. Neither native run implements the browser's complete prediction/reconciliation/resync loop.

The **574.973-second** browser capture included movement from Quay through Market Grid into Civic Center/north ring, with menu pauses between movement segments. It recorded **68,997 frames**, frame p50/p95/p99 **8.30/9.05/9.25 ms**, and prediction/reconciliation CPU **0.10/0.55/0.75 ms** per frame. Mean cadence was **120 FPS**. JS heap samples ranged about **18.8–48.3 MB**, returning to **24.3 MB** at the final sample; WASM stayed at **1,507,328 bytes**, geometries **38**, detailed resident chunks **14–18**. This is a mixed movement/menu capture shorter than ten minutes, at a narrow viewport with loopback observer networking; it does not pass the reference frame, target-network prediction or ten-minute continuous-traversal memory gate. [Browser JSON](evidence/t20-browser.json), [capture UI](evidence/t20-browser.png).

The static package audit reports a conservative first play of **707,125 bytes gzip**, counting all production code plus initial content. This meets the 12 MiB size target; it is a per-file compression audit, not a timed download at a stated network speed. [Asset audit](evidence/t20-assets.json).

Verification: `npm run check` passes **111 TypeScript / 137 Rust tests**, generation checks, release WASM, typing, lint, format and Clippy with warnings denied. Four explicit socket/soak tests are ignored by the default runner. The separate existing actual-transport suite passes **three profiles**, including loss, blackout recovery, reconnect and bounded send pressure. The full production build passes with the existing Three.js chunk-size advisory. [Full check](evidence/t20-check.txt), [build](evidence/t20-build.txt), [actual transport](evidence/t20-actual-transport.txt).

Still incomplete: investigate growing RTC delivery delay and server memory under target impairment; establish representative browser prediction under that profile; rerun a full 60-minute capped-population soak after the transport issue is resolved; run continuous ten-minute traversal and ten settled joins with heap/resource comparisons; validate reference laptop/2-vCPU budgets, and time first play at a stated network speed. These are T20/G4 work. The user subsequently authorized proceeding to T21 while deferring network-performance work. This is an explicit sequencing decision, not a G4 pass.

### Corrected clock and cached-join follow-up

A fresh server and the corrected reliable-Pong input clock completed **120.007 seconds** at sixteen sampled players. Populated tick p50/p95/p99 was **1.625/5.750/6.900 ms**, cadence **29.998 ticks/s**, encoded egress **925.677 KiB/s**. Sampled RTC RTT p50/p95/p99 was **696/3,310/4,493 ms**, max **5,479 ms**; sampled owner lag p95 was **3,200 ms**. RSS rose **23,584 → 28,832 KiB**. Late-input count was **16,027**, rate-limited batches **116**, future inputs **0**. The short duration and changed observer distribution prevent direct tail comparisons with the ten-minute run. It does confirm excessive RTC delay remains with the corrected clock. [Full samples](evidence/t20-target-clock-120s.json), [summary](evidence/t20-target-clock-120s-summary.json), [log](evidence/t20-target-clock-120s.txt).

Ten rapid same-page browser joins through normal HTTP/WS/RTC admission completed in **1,791–1,871 ms** each on cached loopback content. This checks repeated admission/resource recreation, not ten settled post-GC heap observations. [Join diagnostics](evidence/t20-cached-joins.json). The join follow-up explicitly used the existing Low-quality preference; quality was not recorded in the earlier 574.973-second capture, so that capture cannot establish the Medium-quality gate. [Final compact capture controls](evidence/t20-capture-controls.png).

The original normal save database was preserved. After measurements the isolated benchmark server was stopped and the normal `config/dev.toml` server restored. The temporary benchmark database remains `/tmp/pm-t20-capacity.sqlite` for inspection; no test saves were merged into the normal profile database.
