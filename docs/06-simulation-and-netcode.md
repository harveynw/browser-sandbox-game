# 06 — Simulation and netcode

[Index](../README.md) · Shapes: [Protocol](05-protocol.md)

## Clock model

The server simulates one fixed step of `DT = 1/30` seconds. Tick `T` labels the state **after** simulating input assigned to T. The initial loaded state is tick 0. Use a monotonic clock for scheduling, never wall-clock time for physics.

Replication runs every third simulation tick, giving 10 Hz. Input frames are generated at 30 Hz; rendering uses requestAnimationFrame. Timing constants live in shared config and its hash.

Run an accumulator with at most 5 catch-up steps per outer iteration. If further behind, report overload, drop excess accumulated wall time, and continue with contiguous simulation tick numbers. Never pass a larger `dt`, skip authoritative tick numbers, or accelerate the simulation to consume an attacker’s input queue. Reject new joins while persistently overloaded.

Tick/session counters do not wrap in a shard. Drain/restart long before `u32` exhaustion. Comparison is ordinary bounded integer comparison; no half-specified wrap arithmetic.

## Input scheduling

The client estimates current server tick from Welcome/Pong and elapsed browser monotonic time, using the lowest-RTT samples among the last 8 pings. `server_time_ms` is server monotonic milliseconds since process start, not Unix time; it is echoed for diagnostics and offset estimation, never interpreted as a wall clock.

On StartPlay or ControlChanged:

1. Install the supplied authoritative state at tick T and current epoch.
2. Choose input lead `L = clamp(ceil(RTT_ms / 2 / 33.333) + 2, 2, 6)`; default 3 until RTT is known.
3. Set next generated tick to `max(T + 1, floor(estimated_server_tick) + L)`.
4. Advance the local prediction through the intervening ticks with neutral input, then generate/predict one new input frame per 1/30 s.
5. Send the newest and previous two frames from this epoch on each 30 Hz send.

The lead is a small input buffer. Visual prediction happens immediately when a frame is created; authority applies it on its scheduled tick. Change lead only after sustained clock drift, by pausing generation for at most one tick when ahead or generating at most one extra frame per render when behind. Never reuse/relabel already sent ticks.

Server admission for a frame received before step S:

- Session must be Playing; control epoch must match.
- `S ≤ frame.tick ≤ S + 8`. Anything already simulated is late and discarded. Further-future frames reject and increment a violation metric.
- Buffer at most 9 distinct ticks per peer. Duplicate tick with identical content is ignored; conflicting payload for the same tick rejects.
- At step S, use that peer’s frame for S or a **neutral input** if missing. Do not hold movement/acceleration across a missing frame. Missing jump/fire events are not reconstructed.
- No more than one player input can affect that player’s actor in one tick. Redundancy recovers packet loss without increasing simulation speed.

Neutral input has zero movement/buttons and retains the actor’s current facing. Input absence is safe but can cause momentary braking under loss; accept that alpha tradeoff rather than inventing client-controlled time. On lost focus the browser sends new neutral frames immediately; if it stops sending entirely, missing frames are also neutral.

If the client prediction drifts > 6 ticks from its desired lead or the replay ring would exceed 128 ticks, request a full resync. At > 500 ms without any authenticated server message show a connection warning; at 2 s stop prediction and input; at 10 s close and return to reconnect UI.

## Shared movement API

`sim-core` owns state and exact movement constants. It uses Rapier queries for collision; the motors are custom arcade behavior. No physics engine handles appear in protocol DTOs.

```rust
pub struct StepContext<'a> {
    pub tick: u32,
    pub collision: &'a dyn CollisionQueries,
    pub config: &'a SimConfig,
}
pub fn step_character(
    state: &CharacterMotor,
    input: &InputFrame,
    context: &StepContext<'_>,
) -> CharacterStep;
pub fn step_vehicle(
    state: &VehicleMotor,
    input: &InputFrame,
    template: &VehicleTemplate,
    context: &StepContext<'_>,
) -> VehicleStep;
// Return a new motor plus contact/impact observations, not damage or rewards.
// Gameplay applies those observations only on the server.
```

Implement `CollisionQueries` with:

```rust
fn cast_character(&self, feet: Vec3, delta: Vec3, filter: QueryFilter) -> MoveResult;
fn cast_vehicle(&self, pose: VehiclePose, delta: Vec3, filter: QueryFilter) -> MoveResult;
fn overlaps_character(&self, feet: Vec3, filter: QueryFilter) -> bool;
fn overlaps_vehicle(&self, pose: VehiclePose, filter: QueryFilter) -> bool;
fn raycast(&self, origin: Vec3, direction: Vec3, max_distance: f32,
           filter: QueryFilter) -> Option<RayHit>;
```

These are application interfaces, not claimed Rapier method names. `MoveResult` includes applied displacement, grounded/contact normal information, and hit entity/static collider ID. Query filters distinguish static solid, car, NPC hit target, player hit target, and vision blocker.

### Character motor

1. Convert axes to a normalized X/Z camera-relative direction; derive basis from `look_yaw`.
2. Accelerate horizontal velocity toward desired velocity at 30 m/s² on ground, 10 m/s² in air. Decelerate at the same ground acceleration when input is neutral.
3. Apply jump only on its edge bit when grounded and cooldown = 0. Set vertical velocity 5.5 m/s and cooldown 6 ticks.
4. Apply gravity −18 m/s²; clamp fall speed to −30 m/s.
5. Capsule move-and-slide with offset 0.02 m, slope limit 45°, step height 0.25 m, ground snap 0.15 m only when descending and not launching a jump.
6. If grounded, zero negative vertical velocity. Decrement jump cooldown once per tick.
7. Face aim yaw while aiming; otherwise face movement direction if moving, else retain facing. Maximum turn speed 12 rad/s.

Map roads are flat, but the character should handle low steps. Test against walls, diagonal corners, and low ceilings. Characters collide with static world and vehicle shapes; ignore other characters for movement. Shots use separate hit queries.

### Vehicle motor

Cars use planar bicycle steering, upright Y = 0, no gravity, rollover, suspension physics, or airborne driving in alpha.

1. Apply throttle acceleration from its template. Opposite throttle while moving first applies brake; change direction only when speed magnitude < 0.25 m/s.
2. With neutral input, approach zero speed using 1.5 m/s² rolling resistance. While held, the handbrake overrides throttle to neutral and adds 8 m/s² deceleration; it holds the stopped car even when W/S remains pressed. Releasing it restores throttle immediately.
3. Steering input positive means right, so target steering angle is **negative** under our yaw convention. Max steering magnitude interpolates from 0.55 rad at rest to 0.20 rad at top speed. Move steering toward target at 2.5 rad/s.
4. Compute `yaw_rate = speed / wheelbase * tan(steering)`. Clamp magnitude to 1.8 rad/s. At zero speed there is no yaw turn.
5. Forward vector is `[-sin(yaw), 0, -cos(yaw)]`; displacement is forward × speed × DT.
6. Substep casts with translation ≤ 0.25 m and yaw change ≤ 0.03 rad, maximum 8 substeps. Validate the proposed oriented box before each rotation; retain old yaw if rotation overlaps. Sweep translation, slide along a wall, and reduce speed to the component of resulting displacement along the final forward vector.
7. Report pre/post impact speed and counterpart ID. Compute authoritative health changes elsewhere. No client collision observation becomes a trusted hit report.

Starting compact box: width 1.8 m, height 1.4 m, length 4.2 m. Do not use visual mirrors or wheels as solid shapes. State velocity is actual applied displacement/DT, not merely commanded forward speed.

Vehicles collide with static world and other cars. Vehicle/pedestrian impacts are swept server gameplay queries, so a pedestrian does not physically stop a car. Player damage from vehicles is disabled.

## Prediction and reconciliation

The browser’s WASM world contains the same static collision and current relevant vehicle proxies. It simulates only the locally controlled character/car. Remote actor motion is interpolated; do not run remote AI or authoritative health rules.

Keep a 128-entry ring of local inputs and predicted motor states keyed by `(control_epoch, tick)`.

On newer OwnerState at tick T:

1. Ignore stale world/session/epoch. Do not let an unreliable packet install an unannounced control epoch.
2. Replace local simulation state with the authoritative state at T.
3. Delete inputs with tick ≤ `consumed_through_tick` (equal to T).
4. Replay ticks T+1 through the current predicted tick using stored inputs or neutral input for gaps. Bound work to 128 steps; otherwise resync.
5. Replace predicted motor state immediately. Apply the old-to-new difference only as a decaying render offset over 100 ms for errors ≤ 2 m.
6. Snap render position immediately for errors > 2 m, teleports, death, respawn, or control changes. Camera may fade/transition, but collision never eases through geometry.

Remote proxy positions during replay are sampled from the stored snapshot timeline when available, otherwise use the most recent sample. Different surroundings mean different collision results; sharing Rust code reduces drift but does not promise deterministic world rollback. Vehicles colliding at high latency will visibly correct. Do not conceal this by accepting client transforms.

Control changes are authoritative, reliable, and epoch-tagged. Entry/exit may play a pending animation immediately, but camera/physics ownership switches only after ControlChanged. Clear prediction inputs on an epoch change. Health/status can update separately without changing the epoch.

## Remote interpolation

Render remote actors at estimated server tick minus 3 ticks (100 ms). Store up to 32 samples per entity. Position interpolates linearly; yaw interpolates over the shortest angle. Do not interpolate between different modes, teleports, or respawns; snap that transition.

If a newer endpoint is missing, extrapolate position using velocity for at most 100 ms, then hold. After 500 ms without a fresh sample, show a subtle stale indicator and stop animation acceleration. Hide only on reliable EntityHide, never solely because one snapshot omitted it.

Driving player visuals attach to the interpolated car seat. Do not independently interpolate the driver character through the car. Keep the player replica for identity/health/status.

## Interest and bandwidth

Spatial hash cell size 64 m. Observer center is the controlled entity. Enter relevance within 160 m, leave beyond 192 m; this hysteresis reduces visibility churn.

Always include self, its car, and active mission target. Rank remaining entities by proximity, motion, and interaction relevance, with entity ID as deterministic tie-break. Cap visible entities at 96. If an entity is evicted by the cap, send EntityHide with `out_of_range`.

Rates: owned actor 10 Hz guaranteed; relevant moving actors 10 Hz; idle unoccupied cars 2 Hz. Stillness is speed < 0.1 m/s for 1 second. Show/hide and state transitions are reliable; snapshots repair continuous state.

Budget target per peer: 60 KiB/s game egress and 30 KiB/s ingress, excluding transport overhead/assets. Use a 2-second token bucket. At 96 entities × an estimated 180 bytes × 10 Hz, naive output would exceed 160 KiB/s, so radius, idle rates, and prioritization are essential. Measure actual encoded sizes.

Per replication interval, send owner state first, then nearby moving actors, then round-robin others until the byte budget is spent. Track starvation; a moving visible actor should refresh at least every 300 ms under normal target load. If this cannot be met, lower relevance/population before adding compression. Do not silently send unlimited packets.

## Netcode acceptance

- Identical recorded inputs and static collision produce native/WASM positions within 1 cm after 60 seconds in collision-free and wall-sliding scenarios.
- Duplicate input batches do not speed movement. Late frames never rewind the server.
- Dropped inputs neutralize movement; no stuck throttle after blur or disconnect.
- A snapshot arriving before EntityShow or after EntityHide cannot create a ghost.
- Same-tick packets update different entities correctly.
- Losing one snapshot packet cannot prevent later packets from applying.
- Entry/exit under reordered RTC updates cannot restore control of the previous actor.
- At 100 ms RTT, 20 ms jitter, 2% loss: movement remains usable and ordinary static-world correction p95 target < 0.25 m. Measure separately for vehicle collisions.

## T05 implemented native API

The native character checkpoint is implemented in `sim-core`, with the existing protocol motor/input DTOs and a static Rapier query world. See [19 — Native character simulation](19-native-character-simulation.md) for exact constructors, fallible step API, stability policies, tests and the recorded 60-second trace. The draft `step_character` above now returns `Result<CharacterStep>`; caller state remains unchanged on error. Vehicle queries/motor and dynamic proxy categories remain later tasks. T06 now verifies native/browser agreement within 1 cm; see [20 — WASM bridge](20-wasm-bridge.md). T07 authoritative character/input admission is described in [21 — Authoritative sessions](21-authoritative-sessions.md).
