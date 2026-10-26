# Testing and verification

[Documentation index](../README.md#documentation) · [Operations](operations.md) · [Performance](performance.md)

## Automated checks

Use Node 24 and the pinned Rust/WASM tools from [setup](operations.md#first-checkout).

```sh
npm run check
npm run build
npm run test:local
cargo test -p server --example local_probe --locked
cargo test -p server --test actual_transport --locked -- --ignored --nocapture
npm run content:audit -- docs/evidence/asset-budget.json
```

`check` verifies bridge/protocol/content generation drift, builds release WASM, checks TypeScript, lint and formatting, runs Vitest, then Clippy and Rust workspace tests. Socket-dependent tests are ignored in the ordinary workspace run and explicitly invoked above. They need permission to bind loopback sockets; production listeners on other ports can remain running. Launcher tests use owned test processes and loopback listeners, including occupied-port, startup failure and descendant cleanup scenarios. They do not reset normal saves.

The recorded ordinary suite contains **111 TypeScript tests and 138 Rust tests**, plus **seven launcher/process tests**, **two startup-probe unit tests** and **three actual-transport tests**. Counts are observations, not fixed acceptance requirements. [Check output](evidence/checks.txt), [build output](evidence/build.txt), [launcher tests](evidence/launcher-tests.txt), [probe tests](evidence/probe-tests.txt), [actual transport](evidence/actual-transport.txt).

## Coverage

| Area | Verification |
|---|---|
| Protocol | Cross-language positive/negative fixtures, maximum packets, malformed/deep/trailing data, scalar/input limits and Serde defaults |
| Content | Deterministic bake/hash, geometry/reference/navigation checks, immutable publication, generated drift and streamed chunk ownership |
| Simulation | Frozen 1,800-tick native trace with 13 checkpoints, character/vehicle rules, collision queries and actual WASM agreement |
| Prediction | Bounded replay/history, possession/reset epochs, loss/reorder/duplicate delivery, stale visibility and controlled resync |
| Authority | Admission/guest replacement, baseline ordering, inputs, vehicles, damage, police, jobs and repairs |
| Persistence | SQLite restart, committed reward idempotence, storage failure, repair acknowledgement/refund and interrupted-debit recovery |
| Transport | Real WS/RTC receipts, seeded UDP loss/blackout recovery, outbound backpressure and reconnect/lifecycle behavior |
| Operations | Tool/port checks, real startup probe, shutdown, sibling failure, process descendants and cookie reuse |

Seeded in-memory impairment tests exercise game semantics independently of sockets. Actual-transport tests additionally exercise the pinned RTC implementation and bounded test relay. Passing these tests establishes their scenarios; it does not certify loaded-network latency or a memory soak. Capacity and browser-capture methodology belong to [performance](performance.md).

## Local play verification

Run `npm run preview:local -- --temporary`, retaining the printed database path. Use separate normal/private browser contexts for two guests. Avoid switching databases in the same context: a new database can replace that context's guest cookie.

1. Join both guests, move/jump, and check remote interpolation. Enter the same car in turn; verify exclusive possession, stopped exit and the other guest's view.
2. At the cyan Quay terminal, choose courier. Drive to Market, stop within the delivery ring for one second and check the single 150-credit reward. Restart against the same database/context and verify credits remain.
3. Try the north-ring driving trial in gate order. For recovery, accept at Quay/Civic, collect the assigned secured van, return to the garage and exit safely. Check cancellation/timeouts and ownership restrictions.
4. Damage a live car, earn enough credits, stop at the green Ironworks repair zone with no heat and request a repair. Verify its 50-credit cost and restored health.
5. Fire near police to gain private heat. Leave sight, observe search, then escape until heat clears. Arrest/downed states should release vehicles, cancel jobs and respawn safely.
6. Open M/map, change quality, remap controls, toggle reduced motion and sound, and inspect diagnostics. Pause/resume sound requires the browser gesture policy. The world continues while the menu is open.
7. Disconnect/rejoin, test a shared-cookie replacement deliberately, and stop/restart the launcher. Normal saved progress must survive.

The [recorded acceptance](evidence/browser-acceptance.json) verifies two guests, car handoff, a driven courier reward, police pursuit/search/escape, map/menu use and saved reward across two restarts. [Persistence evidence](evidence/persistence.json) checks one SQL reward ledger entry. Screenshots: [gameplay](evidence/gameplay.png), [courier](evidence/courier.png), [map](evidence/city-map.png), [pursuit](evidence/police-pursuit.png), [search](evidence/police-search.png), [escape](evidence/police-escape.png). Automated scenarios cover additional game rules; this recorded browser session is not exhaustive manual coverage of every feature.

## Diagnostic pages

- `/diagnostic`: ordinary guest admission, reliable/RTC receipts, movement probe and baseline resync. It consumes a player slot and shares the context's cookie.
- `/map`: content-only orbit inspector with collision, reservations, lanes, sidewalks/crossings and route overlays; no server required.
- `/simulation`: actual WASM replay of the frozen native trace; expect PASS for 1,800 ticks / 13 checkpoints within 1 cm. No server required.
- In-game Diagnostics: compact counters and graphs for frame/prediction timing, RTT, replay/corrections, entities/rendering and encoded traffic, plus performance capture and Resync world.

## Compatibility and limits

The recorded functional environment is **in-app Chromium (Chrome/156 UA), 1280×720 CSS pixels, DPR2, Medium**, on macOS 14.5 arm64. [Environment](evidence/functional-environment.json). Separate browser contexts exercised independent identities. Drag-to-look is verified; native pointer-lock behavior, forced WebGL restoration, standalone Chrome/Edge/Firefox/Safari and subjective audio mixing remain unverified. Sound pause/on/pause state was checked, without claiming a listening-quality review.

Local launch/process tests are verified on macOS. Linux supervision is not exercised; Windows supervision is unsupported. The [performance results](performance.md) retain excessive RTC delay and memory growth as open findings. There is no qualification for target-network browser prediction, reference hardware, continuous traversal memory stability or a sixty-minute server soak.
