# 11 — Testing and performance

[Index](../README.md)

## What constitutes done

A module is done when its public contract, representative failure behavior, and integration with the preceding milestone are verified. Placeholder implementations, fake “multiplayer” sharing local browser storage, or screenshots without a real server are not accepted.

This document defines the acceptance targets. [T19 coverage and evidence](33-impairment-and-lifecycle.md) maps the implemented regression suite and actual UDP test; performance/capacity targets remain unverified until T20.

## Test layers

| Layer | Tool | Required evidence |
|---|---|---|
| Protocol | Rust tests + Vitest + fixture runner | Both encode/decode directions, every variant, negative payloads |
| Shared movement | Native tests + WASM/browser trace runner | Same traces, tolerances, bounds, collision invariants |
| Gameplay | In-memory server scenario tests | Seat contention, shots, jobs, police, transaction retries |
| Replication | Deterministic fake network | Loss/reorder/duplicate/lifecycle/epochs/budget behavior |
| Actual transport | Rust server + browser | WS signalling and binary + real RTC channel |
| End-to-end | Playwright two browser contexts | Join, move, drive, job, leave/reconnect |
| Capacity | Synthetic actual RTC peers + browser observer | 16 players, populated world, bytes and tick timing |
| Manual | Hardware/browser matrix | Feel, camera, art, controls, UI, accessibility |

Unit tests use injected clocks and seeded RNG. Avoid sleeps except where testing real transport/browser timing. Network tests use bounded deadlines and report captured lifecycle diagnostics.

## Protocol suite

- Every enum variant, empty action payload, tuple, nullable field, maximum collection, and numerical bound.
- Reject wrong versions, missing/unknown fields, duplicate map keys, incorrect envelope IDs, invalid UTF-8, extensions, trailing values, huge declared lengths, depth overflow, NaN/infinity.
- Rust map serialization matches generated TS shape; ensure accidental `rmp_serde::to_vec` does not silently change structs to arrays.
- Generation is deterministic; CI compares generated output from a temporary directory with tracked artifacts.
- Maximum legal input batch and owner state fit the RTC application limit.
- Decoding arbitrary fuzzed bytes never panics, allocates outside configured caps, or partially mutates world state.

## Simulation and gameplay invariants

- Axis normalization prevents diagonal speed gain.
- Movement is independent of render FPS (30/60/144 render schedules).
- Character fits under correct ceilings, cannot pass through walls, lands once, cannot double jump.
- Vehicle cannot tunnel through a wall at 30 m/s, rotate through a corner, or accelerate faster from duplicate input.
- Global entity counts never exceed configured caps.
- At most one driver per car and one controlled actor per player.
- Every driving player points to a car whose driver points back to that player.
- Health/cash do not underflow; positions/velocities remain finite and in supported world bounds.
- Repeated action ID gives the same result without a second state change.
- Mission reward and repair debit/refund are idempotent across retry/restart.
- Leaving or failing a mission releases all reservations.

Use property tests for arbitrary bounded controls and randomized action interleavings where they test real invariants. Do not add tests that simply restate a constant without exercising behavior.

## Network impairment matrix

Implement a seeded test scheduler around application packet delivery for deterministic protocol tests. A separate real-network test must verify actual RTC loss/backpressure behavior; browser HTTP throttling alone does not simulate RTC impairment.

| Profile | RTT | Jitter | Loss | Reorder/duplicate | Expected |
|---|---:|---:|---:|---|---|
| Loopback | < 10 ms | none | 0 | none | baseline correctness |
| Good broadband | 50 ms | 10 ms | 1% RTC | occasional | smooth play |
| Supported target | 100 ms | 20 ms | 2% RTC | 5% reorder, 1% duplicate | playable; bounded correction |
| Stressed | 200 ms | 50 ms | 5% RTC | 10% reorder | visible degradation, no desync/crash |
| Severe | 500 ms | 100 ms | 20% RTC | bursts | clear stale/connection feedback |
| Disconnect | unbounded | — | 100% | — | neutral controls and cleanup |

Tests must cover:

1. Snapshot before spawn; hide followed by old snapshot; show again with new visibility ID.
2. Same tick split across packets; packet lost between good packets.
3. Old-epoch input and owner state arriving after vehicle entry/exit.
4. Reliable action duplicated; reconnect sends stale previous-session envelope.
5. Pause tab 10 seconds; resume; no uncontrolled movement or giant catch-up.
6. Slow receiver and burst sender; no unbounded application queue.
7. Pre-open binary input, missing ClientHello, invalid ticket, load timeout.
8. Kill plus rapid peer-ID reuse; old close/data must not affect a new session.

## End-to-end acceptance script

Use separate browser contexts with distinct guest cookies:

1. Start actual server with fixed seed and test content; readiness succeeds only after transport probe.
2. A and B join; each sees the other exactly once and hashes match.
3. A moves 10 m; B sees movement within the interpolation/latency budget.
4. A and B request the same compact; one drives, the other sees occupied.
5. Driver follows courier route, parks in destination, receives exactly 150 credits.
6. Driver exits; other player enters; both replicas agree.
7. In full alpha, shoot near police, gain heat, escape into occlusion, lose heat.
8. Disconnect one browser; survivor sees player removed and car released.
9. Rejoin; new entity/session, saved balance restored, no old job/heat/vehicle control.
10. Stop server; clients show disconnected and release controls/resources.

Use a test-only server interface behind a test build flag to inspect state/assert outcomes. It must not exist on the public release listener. Do not bypass transport or authoritative rules for the tested gameplay action.

## Performance targets and measurement

Choose and record a real reference laptop and browser before claiming a pass. Initial target class: 4-core laptop, 8 GiB RAM, integrated GPU, 1280 × 720 medium. Exact hardware remains a validation decision.

| Metric | Target |
|---|---|
| Client frame time | p95 ≤ 22 ms; sustained ≥ 45 FPS on reference laptop; 60 FPS desirable |
| Client prediction/reconciliation work | p95 ≤ 2 ms/frame under target network profile |
| Server tick work at full population | p95 ≤ 20 ms, p99 < 33.3 ms on a documented 2-vCPU host |
| Game network per peer | average ≤ 60 KiB/s down, ≤ 30 KiB/s up, excluding transport overhead |
| 16-player server application egress | ≤ 960 KiB/s nominal budget sum |
| Connect to playing, cached content | ≤ 5 s on reachable network |
| First playable download | ≤ 12 MiB compressed; report timing at a stated network speed |
| Browser memory | no sustained growth across 10-minute traversal and 10 joins |
| Server memory | no sustained growth over 60-minute soak at population cap |

Collect p50/p95/p99 rather than average alone. Include asset downloads separately from gameplay bytes. Measure encoded bytes before transport and real socket traffic separately; overhead can be significant.

Debug overlay: FPS/frame time, sim tick, estimated server tick, input lead, RTT, input loss/late counts, correction distance, visible entities, draw calls, tri count, RTC/WS buffered amounts, bytes/s.

Server metrics: tick histogram, per-system timing, queue depth/drops, peers by phase, malformed/late/future inputs, outgoing bytes, AOI starvation, NPC/path budget, economic worker latency/failure, transport failures, join success/failure reason.

## Browser and deployment matrix

Minimum test matrix at release: current stable Chrome/Edge and Firefox on desktop, plus Safari on a supported Mac if its full actual RTC/WASM/WebGL flow passes. Browser names alone do not constitute support; record versions and results. If one fails, document it in the supported-browser UI rather than claim untested compatibility.

Localhost is the required deployment test. LAN, public home-network access, a second network, and TLS are deferred to the optional hosting appendix. Local success does not claim those paths work.

## Milestone gates

- **G0 Contracts:** native build, WASM build, generated types/schemas, all wire fixtures.
- **G1 Transport:** real reliable/unreliable browser round trips, failure cleanup, recorded net revision.
- **G2 Slice:** two players, movement/prediction, car possession, complete courier.
- **G3 Alpha gameplay:** all documented mechanics, mission/idempotency scenarios, full-city traversal.
- **G4 Capacity:** 16 clients and population caps, target impairment, budgets measured.
- **G5 Local handoff:** one-command launch, locally served production build, persistence recovery, clean shutdown, complete runbook.
- **Future only:** transport hardening, public UDP/TLS tests, cross-network operation. This is not a local completion gate.

Do not compensate for a failed gate by claiming the next milestone is complete. Report the failing command/scenario, evidence, and the smallest blocking fix.
