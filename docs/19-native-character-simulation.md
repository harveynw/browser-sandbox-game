# T05 — Native character simulation

This checkpoint implements character movement in shared Rust. It does not add browser input, WASM bindings, player entities, vehicles or network authority. The browser remains the T04 map inspector/T03 transport diagnostic.

## API and ownership

`sim-core` has no networking, clock or renderer dependency. It uses existing `protocol::CharacterMotor` and `InputFrame`, not duplicate actor DTOs. `CharacterMotor` remains complete state: feet position, velocity, yaw, grounded flag and jump cooldown. Tick/epoch/session scheduling lives outside the motor, so T07/T09 must admit each input once and supply the matching tick.

```rust
let config = sim_core::SimConfig::from_content(&baked_sim_config)?;
let world = sim_core::CollisionWorld::from_content(&baked_collision, &config)?;
let state = sim_core::spawn_character([-180.0, 0.0, 180.0], 0.0)?;
let input = sim_core::neutral_input(1, state.yaw);
let result = sim_core::step_character(&state, &input, &sim_core::StepContext {
    tick: 1,
    collision: &world,
    config: &config,
})?;
let next_state = result.motor;
```

File loading/integrity checks are the caller's job; these constructors accept decoded content DTOs and validate runtime geometry/constants. The world builds its immutable static query BVH once, sorting authored collider IDs before insertion. No query/motor call rebuilds it. No engine handle appears in returned results.

`SimConfig` converts the T04 config into validated f32 `CharacterSettings`. External callers cannot mutate those settings afterward. A step rejects a collision world built with different character settings. Frequency is always 30 Hz / `DT_SECONDS = 1/30`; the API accepts no variable dt. There is no call to the dynamic rigid-body simulation pipeline.

`step_character` is deliberately fallible (`Result<CharacterStep>`), refining the draft interface in document 06. It rejects mismatched/reserved ticks, invalid input bits/axes/angles, non-finite/unreasonable restore state, starts inside geometry and unsafe query outcomes. It never mutates the input motor or exposes a partially updated state on error. Restore data must come from the authoritative server; finite-value validation does not authenticate it.

`CharacterStep` returns the tick/new motor, bounded contact observations (static collider IDs and world normals), and `jumped`, `hit_ceiling`, `endpoint_clamped` diagnostics. Health, rewards, entity ownership and networking are excluded.

## Collision boundary

`CollisionQueries` currently exposes:

- `character_settings()` — ensures geometry and motor use matching dimensions/config.
- `cast_character(feet, delta, options)` — swept capsule movement with grounded/snap policy; returns applied displacement and contacts.
- `overlaps_character(feet, filter)` — penetration query, ignoring exact surface touch with a 0.1 mm radius reduction.
- `raycast(origin, direction, distance, filter)` — validates/normalizes the direction and returns static ID, distance, point and normal.

`QueryFilter::MOVEMENT` uses content flag 1 and `VISION` flag 2; an optional static ID can be excluded. Cars/NPC/player query categories, moving proxies and vehicle queries are explicitly future extensions (T10/T14), not fake implementations. The native loader accepts valid yaw-rotated boxes, although the current T04 bake emits yaw zero. Pitch/roll ramps are outside the flat alpha map.

Pinned engine: **rapier3d 0.36.1**, with `std`, `dim3`, `f32`, `enhanced-determinism`; parallel and explicit SIMD features are not enabled. The underlying Parry version is locked at 0.31.1. Source/API references: [versioned controller API](https://docs.rs/rapier3d/0.36.1/rapier3d/control/struct.KinematicCharacterController.html), [query pipeline API](https://docs.rs/rapier3d/0.36.1/rapier3d/pipeline/struct.QueryPipeline.html). Enhanced determinism is not a claim that native/WASM agreement has already been measured.

The capsule has total height 1.8 m, radius 0.35 m, centered half the height above its feet. Movement uses Rapier's kinematic controller, with a 0.02 m absolute offset, 45° slope threshold, 0.25 m autostep and minimum landing width equal to the capsule diameter. No other actor's character collider exists in this static world.

Three explicit stability policies are part of the shared implementation:

1. Sweep horizontal and vertical phases separately. Rising motion resolves vertical first; descending motion resolves horizontal first. Autostep applies only to grounded horizontal movement. Every phase uses a shape sweep, rather than discrete endpoint motion.
2. A supported foot within the skin margin can be lifted to the configured resting gap if headroom is clear. The same small support-gap correction runs after downward movement. This keeps authored Y=0 spawns valid and avoids ambiguous exact-touch ground normals. Larger upward corrections and depenetration of invalid restores are not permitted.
3. A strict final capsule-overlap check catches grazing box-corner cast error. On a penetrating endpoint, a bounded 14-iteration search clips motion to a clear endpoint and reports `endpoint_clamped`. It does not increase overlap tolerance. Motor post-validation still rejects any unsafe returned state.

Ground snap is a separate downward capsule cast, only when the previous state was grounded, the step is descending, and no jump launched. A character already airborne does not snap merely because it passes within 0.15 m of ground. Contact output is capped at 16 records, but ceiling detection observes all cast callbacks.

## Motor behavior

Input axes are normalized when their combined length exceeds one. Forward at yaw zero is −Z; camera-right is +X; positive yaw rotates forward toward −X. Forward sprint raises only the forward-axis limit from 4 to 7 m/s; backward/strafe retain 4 m/s limits. Thus a walking diagonal is 4 m/s, and a sprint diagonal stays below 7 m/s without gaining full sprint speed from pure strafing. Partial analog input stays proportional.

Horizontal velocity approaches the desired vector by at most 30 m/s² on ground or 10 m/s² in air, including neutral braking. Persisted horizontal velocity comes from actual applied displacement/DT, preventing continued velocity into a wall. Vertical velocity is the gravity/jump integrator (not autostep or skin correction displacement).

Jump requires the input's jump-edge bit, grounded state and zero cooldown. It sets takeoff velocity 5.5 m/s, then applies this tick's 18 m/s² gravity; the first resulting vertical velocity is 4.9 m/s. Cooldown starts at six ticks and decrements in that same tick to five. Repeated airborne jump edges cannot produce another launch. Landing/ceiling collision zero blocked vertical velocity; fall speed caps at 30 m/s. Input sampling must supply an edge, not a held jump button; no hidden previous-button state is added to the motor.

Facing uses aim yaw while aiming, otherwise actual horizontal movement direction, retaining yaw while stopped. It follows the shortest wrapped arc at at most 12 rad/s. There is no render smoothing here.

## Tests and recorded trace

Run:

```sh
cargo test -p sim-core --locked
npm run sim:trace
npm run check
```

The 16 native simulation tests cover speeds/normalization, acceleration/braking, jump/cooldown/gravity/landing, air control, ceilings, low/tall steps, ground snap, wall/corner sliding, angle conventions/turn limits, filters/ray identity, invalid inputs/config/geometry, the actual baked spawn, seeded bounded inputs and the recorded trace. The grazing-corner test preserves the exact seed-17/tick-205 reproduction, asserting that the endpoint guard activates and returns a clear capsule.

`tests/fixtures/simulation/character-trace.json` stores a 60-second, 1,800-tick run with run-length encoded inputs, complete initial motor/config/collision, their hashes and 13 frozen complete motor checkpoints. It exercises walking into a wall, diagonal wall slide into a corner, reverse movement, a jump edge, sprint, a rotated camera basis and neutral braking. Replay checks capsule clearance every tick. Native verification requires position/velocity error ≤0.0001 and yaw error ≤0.001, with exact grounded/cooldown values. Replaying twice also compares motor states exactly.

`sim_core::trace::{replay, verify}` is pure Rust without filesystem access; xtask owns loading/reporting. The fixture can be replayed in the later WASM bridge. `cargo run -p sim-core --locked --example record_trace` prints candidate regenerated checkpoints; it never overwrites the saved fixture. Review and explain any checkpoint changes instead of automatically blessing new values.

T06 must run this trace through actual browser WASM and compare native/WASM checkpoints within 1 cm. Compile success alone is not that gate. T07 must supply authoritative input admission/ticks and actor lifecycle; T08 supplies actual browser controls/camera/rendering. No browser playability or dynamic-world collision claim is made by this native checkpoint.

T06 follow-up: the byte bridge and real browser/production agreement checks are now complete. See [20 — WASM bridge](20-wasm-bridge.md); the native checkpoint details above remain the T05 record.
