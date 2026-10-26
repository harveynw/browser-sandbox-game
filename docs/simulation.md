# Shared simulation, prediction and queries

[Documentation index](../README.md#documentation) · [Gameplay](gameplay.md)

## Motor ownership

`sim-core` supplies fallible, fixed **1/30 s** character and vehicle steps. Inputs, complete motor state, collision/query context and actual vehicle template determine the next state. The caller commits only valid results. Network/session deduplication belongs to server admission and client reconciliation, not hidden motor state. There are no health, money, mission or rendering rules in the motor crate.

The server calls this crate natively. `sim-wasm` exposes the same code to the browser; TypeScript owns scheduling, presentation and replay identity, not a second physics implementation. Rapier 0.36.1 is used with f32 and enhanced determinism, without parallel/explicit SIMD features. Native/WASM trace tests verify agreement within tolerances; shared code alone is not a cross-platform bit-identical-world guarantee.

## Character motor

Feet positions define a capsule **1.8 m** high and **0.35 m** in radius. The Rapier kinematic controller uses a **0.02 m** offset, **45°** slope threshold and **0.25 m** autostep. The authored city is flat; pitch/roll ramp gameplay is not implemented.

Forward at yaw zero is −Z, right is +X and positive yaw turns forward toward −X. Combined axes normalize to prevent diagonal gain. Walk is **4 m/s**, forward sprint **7 m/s**; backward/strafe retain walking limits. Desired horizontal velocity is approached at **30 m/s²** grounded or **10 m/s²** airborne, including neutral braking. Actual displacement determines persisted horizontal velocity so walls do not accumulate hidden movement.

Jump requires a grounded edge and zero cooldown, launches at **5.5 m/s**, uses **18 m/s²** gravity and caps falling at **30 m/s**. The six-tick jump cooldown decrements during the launch tick. Airborne repeated jump edges cannot launch again. Facing follows movement or aim through the shortest arc, capped at **12 rad/s**.

Horizontal/vertical phases sweep separately; rising resolves vertical first and descending resolves horizontal first. Autostep only applies to grounded horizontal movement. Small supported skin-gap corrections require headroom; larger depenetration of invalid restored state is rejected. A bounded fourteen-iteration endpoint search clips grazing corner penetration. Ground snap only applies to a previously grounded descending actor without a jump. Contacts are bounded, and blocked vertical motion cancels vertical velocity.

## Vehicles and collision

Vehicles are upright kinematic oriented boxes with template-specific acceleration/braking, top/reverse speed, wheelbase, dimensions, health and seats/exit anchors. Steering eases toward speed-dependent limits; handbrake overrides throttle. Translation and yaw substeps sweep the full box. Blocked motion cancels numerical wall creep; unintended sliding is discarded. Damage is a server interpretation of impact observations.

`CollisionWorld` loads all static authored colliders once and maintains replaceable vehicle proxies. Queries include character/vehicle shape casts and overlaps, supported exits/floor checks, normalized static rays/sphere casts, weapon rays and relative-motion car/pedestrian sweeps. Movement and vision flags are distinct; entity and static collider IDs have separate namespaces. Proxies exclude the currently controlled car. Remote proxy state for replay comes from bounded interpolated replicas, with shortest-arc yaw.

No render mesh determines collision. Evicting a visual chunk does not remove any physics. Invalid/nonfinite settings, geometry, inputs or unsafe overlaps fail rather than partially advancing caller state. Query/motor public APIs are in [`sim-core`](../crates/sim-core/src/lib.rs).

## Browser WASM boundary

[`WasmPredictionWorld`](../crates/sim-wasm/src/lib.rs) exposes constructor/free, `load_collision`, `set_vehicle_proxies`, `reset`, `step`, `current_state`, `raycast_static` and `spherecast_static`. [`simulation/bridge.ts`](../apps/client/src/simulation/bridge.ts) owns asynchronous module initialization, allocation lifetime, TS validation and byte conversion. Free is idempotent; further use fails. Failed initialization can be explicitly retried.

The local bridge is **version 1**, independently versioned from the game protocol. MessagePack named-map control buffers are capped at **16 KiB**, each collision/config buffer at **2 MiB**, with decode depth **32**, trailing-byte rejection and shared scalar/simulation safety checks. Bridge-owned request structs reject unknown fields. Generated AJV2020 schemas validate the wrapper's local DTOs; network DTO schemas are not a second game-packet validation layer.

`PredictedFrame` contains version, tick, epoch, controlled entity ID, character motor and optional `VehiclePrediction` (actual vehicle motor/template). `StepRequest` carries version/identity/epoch/InputFrame. `ProxySet` supplies actual vehicle motor/template proxies. A step must be exactly current tick +1 for the restored actor/epoch. Reset may replace tick/identity for authorized reconciliation. Only collision loading builds the static query world; replay/reset/query calls reuse it. The browser verifies content hashes before passing content into this local boundary.

## Prediction and reconciliation

The client predicts only its controlled actor. StartPlay and reliable ControlChanged install authority and clear old history. The rAF accumulator generates 30 Hz input with at most five steps per frame and at most **128** epoch-scoped input/predicted-state entries. Each outgoing batch contains the newest three ascending frames. Excess drift (>6 ticks), long frames (>250 ms) and history overflow request resync.

A newer authoritative owner sample resets the shared motor, retires consumed inputs and replays remaining stored/neutral frames, capped at 128 steps. Small correction uses a **100 ms** decaying render offset; errors over **2 m** and mode/death/control discontinuities snap. Local rendering interpolates consecutive predicted poses independently of render FPS. A future unreliable epoch may be cached but never grants control. Arrested/downed owners freeze predicted controls until a valid authoritative transition.

Remote actors use independent interpolation/extrapolation described in [networking](networking.md). Their samples supply bounded moving-car proxies for local replay, without becoming authority over another actor.

## Camera and traces

Camera queries share verified static collision. Walking/driving follow views, template-sensitive height, reduced-motion behavior, obstruction sphere casts and aiming use presentation code; camera state never determines server position or hit origin. Obstruction can shorten the camera rather than clipping through a wall. Aim uses a simple first-person procedural pistol view.

Frozen character and vehicle traces under `tests/fixtures/simulation/` provide 1,800-tick native/WASM comparisons. Character checks use thirteen checkpoints with 1 cm position/velocity and 0.001 rad yaw tolerances plus exact discrete fields; the vehicle trace has sixty checkpoints. Fixtures are deliberate reference data and are not regenerated merely to make a test pass. `/simulation` replays the character trace through the public WASM wrapper; `npm run sim:trace` runs its native counterpart.
