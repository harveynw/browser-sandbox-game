# 24 — Vehicles and atomic possession

[Index](../README.md) · [Netcode](23-prediction-and-replication.md) · [Module diagrams](diagrams/README.md)

T10 adds the compact vehicle template already present in the verified slice content. The two authored parking slots become real server entities. This milestone covers walking, driving and possession; courier missions remain T11.

## Ownership and modules

- `crates/sim-core/src/vehicle.rs` owns the upright bicycle motor, template validation, seat/door transforms and a common presentation pose. It uses the same fixed 1/30-second step in native Rust and WASM.
- `crates/sim-core/src/collision.rs` retains the static BVH and adds up to 96 reusable vehicle collider slots. Only changed proxy poses/shapes update their AABBs. The controlled car is excluded from its own collision queries. Characters collide with cars but not other characters; cars collide with the static world and other cars. A newly overlapping moving proxy can displace a character to the closest clear side without crossing static geometry. Static penetrations remain errors.
- `crates/server/src/vehicles.rs` owns vehicle state helpers and safe exit geometry. `world.rs` owns the actual occupancy transactions, epochs, action cache and authoritative stepping. Entity ID order defines deterministic car stepping; each later car observes earlier committed car poses in that tick.
- The WASM bridge adds an optional `vehicle` field to `PredictedFrame`, containing the full vehicle motor and template. `motor` remains a common presentation pose; it must match the vehicle motor when restoring a car. The vehicle motor remains the simulated state. Existing character frames omit `vehicle`. Proxy uploads have a 64 KiB byte limit and 96-vehicle count limit; ordinary control buffers remain 16 KiB.
- `net/session.ts` installs reliable control changes, sends reliable interaction requests, and supplies vehicle proxies sampled from replica timelines before prediction/replay steps. `simulation/prediction.ts` clears its ring and presentation offset on control changes. Unreliable future-epoch owner states can be cached but cannot grant control.
- `world/replica.ts` interpolates both character and car poses. Driving player visuals attach to their car's presentation seat. `render/renderer.ts` instantiates verified car primitives; `render/camera.ts` uses a longer follow distance while driving and preserves the approved walking framing.

## Motor and collision

The compact's dimensions, top/reverse speed, acceleration, braking and wheelbase come from the hashed content template. Rolling resistance, handbrake, steering response and query budgets come from hashed simulation configuration. Positive steering input turns right under the existing yaw convention. Opposite throttle brakes before reversing; neutral input brakes through rolling resistance. Space applies a held handbrake while driving, overriding acceleration in either direction until released. The HUD shows speed in km/h and a Handbrake indicator while Space is held.

Translation sweeps use substeps of at most 0.25 m and rotation increments of at most 0.03 radians, capped at eight per fixed tick. Candidate rotations must be clear before translation. Swept boxes slide along obstacles; actual displacement determines velocity and remaining forward speed. Cars remain upright on the flat Y=0 plane. Flat-floor support surfaces below the car are excluded from horizontal sweeps, vertical controller sliding is discarded, and obstructed planar endpoints are rejected. Contact displacement below 0.05 m/s is treated as stopped to suppress numerical wall creep. Impacts are observations, not client-authoritative damage reports.

## Entry and exit

Press E near the left door of a parked compact. The browser sends an `enter_vehicle` action with only its target entity ID. The server checks current phase/epoch, relevance, on-foot mode, health, occupancy, speed below 0.5 m/s, distance within 2 m of the driver entry anchor, and an unobstructed segment from the player's chest. Requests are applied serially in stable peer order. Exactly one contender can become the driver; the next receives `occupied`.

A successful transaction assigns the driver, updates player mode/vehicle and seat position, increments the control epoch, clears old queued inputs, and emits reliable `ControlChanged`. Affected visible player/car replicas receive reliable refreshes with new visibility IDs, so reordered snapshots cannot restore old occupancy. Duplicate request IDs return the cached result without repeating the transaction.

Press E while driving to exit. Speed must be below 2 m/s. The server tries the authored left, right and rear offsets in order. Each candidate needs floor support, capsule clearance and a clear segment from the door. If none is safe, `exit_blocked` preserves occupancy and epoch. Success restores the player as the controlled actor and advances the epoch again. Disconnect releases the driver and immediately stops the car. Resync retains the selected actor, stops motion and requires a new baseline/readiness handshake.

Owner state stays at 10 Hz. Moving or occupied cars replicate at 10 Hz; unoccupied cars below 0.1 m/s for a second replicate at 2 Hz. Existing byte budgets, message packing and bounded queues still apply.

## Verification and reproduction

Run `npm run check` and `npm run build`. Native vehicle tests cover acceleration/braking/reversing, steering sign, handbrake, a 30 m/s thin-wall sweep, proxy collision/exclusion/removal/reuse, character displacement and the frozen driving trace. Server tests cover simultaneous contenders, cached actions, too-fast exits, blocked exits, successful exit epochs, stale input rejection and disconnect release. Browser-side tests cover held/edge controls, vehicle interpolation, reliable occupancy refreshes and retired owner snapshots.

`tests/fixtures/simulation/vehicle-trace.json` records 1,800 native ticks and 60 checkpoints. Native tests retain those checkpoints and real release-WASM tests compare against them within 1 cm. Explicit regeneration, only after reviewing a deliberate motor change:

```sh
cargo run -p sim-core --example vehicle-trace --locked > tests/fixtures/simulation/vehicle-trace.json
```

For a live pair, run the Rust server and Vite, build the production client, then preview on `127.0.0.1:4173`. Use `localhost:5173` for the first guest and the preview for the second, giving independent hostname-scoped cookies. Walk east from the spawn plaza to the parked cars, approach the left door and press E. WASD drives, Space brakes, E exits once stopped, and Esc clears held controls while the world continues. Diagnostics show control epoch, correction, actor positions and traffic rates.

Only the compact and the two authored parking slots are active. Vehicle damage/disable/respawn gameplay, NPC traffic, passengers, missions, durable profiles and public hosting are outside this milestone. The motor reports impacts for subsequent authoritative gameplay; it does not yet apply damage. Dynamic vehicle collisions under latency may visibly correct; the trace is a shared-motor check, not a claim of full network rollback or a 16-player soak result.
