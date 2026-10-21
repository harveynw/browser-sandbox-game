# 23 — Prediction, reconciliation and replication

[Index](../README.md) · [Wire contract](05-protocol.md) · [Timing rules](06-simulation-and-netcode.md) · Implemented in T09, 21 October 2026.

## Run and demonstrate

Use Node 24.19.0 and the pinned Rust toolchain. Run `npm run check`, `npm run build`, then `npm run dev` and `cargo run -p server --locked` in separate terminals. At localhost:5173 choose Play and Resume / mouse look. Drag-look works in the in-app browser without desktop permissions; successful native pointer lock remains unverified there.

For two independent guests in the same browser, also run:

```sh
npm run preview --workspace @port-meridian/client -- --host 127.0.0.1
```

Open localhost:5173 and 127.0.0.1:4173. The first is the development client; the second is the production bundle. Hostname cookie scopes create separate genuine guests, each with its own admission ticket, socket/RTC connection, actor and session. Same-hostname tabs still replace one another. Separate private browser contexts are another supported setup; the live evidence for this checkpoint uses the two hostname scopes in the existing in-app browser.

Orange is the local player, teal is a remote player, and grey indicates a stale remote sample. WASD/arrows, Shift, Space and drag/mouse look retain the T08 controls. The camera remains horizontally centred; the head sits just below the fixed green centre cross. Driving, vehicles and jobs are T10–T11.

## Client modules and bounds

- `net/clock.ts`: Welcome/owner fallback anchors, then the lowest RTT among the last eight matching pongs, estimating the current tick with half-RTT compensation. Input lead defaults to 3 and becomes `clamp(ceil(RTT/2/DT)+2,2,6)`.
- `simulation/prediction.ts`: one actual WASM motor, at most 128 epoch-scoped input/predicted-state entries. StartPlay/ControlChanged installs authority, clears prior inputs and steps neutral gaps before the first scheduled tick. The rAF accumulator generates 30 Hz inputs with at most five steps per frame. Sustained lead drift pauses one tick or adds at most one extra step within that cap; >6 ticks of drift or a >250 ms frame gap requests resync.
- `net/session.ts`: real scoped admission/baselines, newest-three ascending input redundancy, per-owner tick comparison, one future-epoch owner cache, reliable control installation, status revision checks, heartbeat/readiness and cumulative encoded-byte counters. A reliable resync request is limited to one per five seconds and can repeat while waiting for its baseline acknowledgement. No old-session envelope updates authenticated liveness.
- `world/replica.ts`: at most 96 visible replicas, 32 samples each, and 256 bounded tombstones. Independent per-record ticks allow same-tick packets for different actors. Unknown, retired or old-visibility snapshots cannot spawn actors. BaselineBegin preserves inert render poses while clearing authoritative replicas/tombstones; BaselineEnd replaces them.
- `render/renderer.ts`: shared primitive assets/materials for local and remote capsules. Replica removal removes the corresponding render group; simulation/health are never derived from meshes.

Newer OwnerState resets the shared WASM motor at the authoritative tick, retires all consumed input and replays remaining ticks with stored or neutral input. Replay work is capped at 128 steps. Small errors affect only a 100 ms decaying render offset; >2 m errors, mode/death/control changes snap. Local rendering interpolates consecutive predicted fixed-step poses independently of render FPS. Successful reliable control changes clear all old input identity; unreliable future epochs never install ownership.

Remote rendering samples estimated server tick minus three ticks, with linear position and shortest-arc yaw interpolation. Mode/death/teleport discontinuities snap. Missing endpoints extrapolate velocity for no more than three ticks, then hold; after 500 ms without a fresh sample the remote is grey. Only reliable EntityHide removes it. Snapshot omission alone does not despawn anything. T09 creates only player characters; car-seat attachments and vehicle proxy replay belong to T10.

At 500 ms without an authenticated message, the HUD warns that the connection is delayed. At 2 s prediction/input stop; renewed traffic requires a fresh baseline. At 10 s the session closes into retry UI. Blur, menus and hidden-tab handling still neutralize input; a long background frame requests resync rather than replaying an unbounded backlog.

## Server modules and bounds

`server/replication.rs` builds a 64 m spatial index. Players enter relevance at 160 m and leave beyond 192 m. Self is retained, nearest actors win deterministic ties by entity ID, and the general visible cap is 96 (the current shard has at most 16 players). Baselines use one frozen relevant set; subsequent visibility changes append on the reliable FIFO. Re-entry receives a new visibility ID. Relevant characters replicate at 10 Hz; owner state is independent of ordinary snapshot culling.

Snapshot envelopes contain complete independent records, split by actual `encode_server` MessagePack size including scope/header overhead. Each packet is ≤1,100 bytes; one record is never split. Refresh priority includes starvation age and distance, and native metrics record packet count, maximum packet bytes and maximum queued refresh age. Idle-car rates, mission-target relevance and population priorities become applicable when those entities exist.

Per-session game ingress is a 30 KiB/s token bucket with a two-second capacity. Input batches also have a strict rolling 60-per-second cap; duplicate ticks still cannot produce extra simulation steps. Actions retain their separate 10/s, burst-20 admission and remain unavailable gameplay at this checkpoint. Egress admission uses 60 KiB/s with two-second capacity; snapshots retain a 1,100-byte owner reserve. Budget-full unreliable output drops safely; reliable exhaustion disconnects instead of accumulating unlimited world output.

The pump additionally meters the actual encoded bytes it submits to net-crate, after coalescing/repacking. Its reliable FIFO remains bounded. Owner state is drained before snapshots; pong and cosmetic streams remain separate. SnapshotBuffer retains the latest record per entity/visibility (at most 96), groups records by their source tick and re-packs independent envelopes. This preserves same-tick splits and unrelated entities from older packets. Baseline/control messages clear pending old snapshot/owner state; EntityHide retires matching pending records. Dependency-internal queues/health/shutdown limitations remain as documented in 04; these are application bounds, not a new claim about net-crate internals.

## Diagnostics and verification

The Diagnostics panel shows owner and predicted ticks, lead, lowest recent RTT, latest and rolling-256 correction p95, replay count, ring size, visible remote count/coordinates and approximately one-second encoded application KiB/s. Resync world requests a fresh baseline/epoch. These rates exclude WS/RTC/socket framing and content downloads. Native World metrics count application admission; the pump enforces its separate actual-submission budget.

`npm run check` passes with **178 TS and 64 Rust tests**, including real release-WASM replay and seeded application delivery. `npm run build` passes native/WASM/contracts/content/frontend production bundling. Existing Three.js >500 kB warning remains; no populated frame-time/capacity gate is claimed.

Seeded simulation uses real WASM motors on the client and authority reference, real protocol-sized input/owner encoding, and the same neutral/single-frame admission rules tested independently in the native server. It is a deterministic application-delivery test, not a claim to have physically impaired the actual RTC connection. Both profiles run 1,800 server ticks / 60 seconds with reorder and duplicate delivery. Full-run measured corrections:

| Profile | RTT | Jitter | RTC loss | Correction p95 | Maximum correction | Conflicting inputs / resyncs |
|---|---:|---:|---:|---:|---:|---:|
| Supported | 100 ms | ±20 ms | 2% | 0.000000 m | 0.033351 m | 0 / 0 |
| Stressed | 200 ms | ±50 ms | 5% | 0.033933 m | 0.601377 m | 0 / 0 |

The supported ordinary static-world p95 is below the 0.25 m target. The zero reported percentile does not mean every packet arrived: newest-three redundancy recovers most missing frames, while occasional late/absent frames produce the measured nonzero maximum. Replays at 30/60/144 Hz render schedules also produce identical motors at the same simulation tick. Tests independently cover stale epochs, future-epoch caching, ring overflow, stopped prediction, small correction smoothing, large snaps, spatial hysteresis/re-entry, strict byte/rate limits, split-packet coalescing, stale generations and seeded ghost prevention.

Real final development + production guests on the same Rust server demonstrated walk/jump replication, epoch-2 resync without duplicate remotes, disconnect removal, and a fresh actor on reconnect. The final health response showed exactly two Playing players/two sessions with readiness true. Live loopback rates were around 10–11 KiB/s ingress and 9 KiB/s egress per guest; these are two-player diagnostic samples, not full-population budgets. Screenshots and exact movements are recorded in the implementation log.

No car simulation/proxies, mission logic, durable profiles, populated performance/soak test, physical-network impairment test or cross-network hosting is claimed. T10 is the next milestone. T19/T20 still own the complete impairment/lifecycle matrix and actual-RTC capacity measurements.
