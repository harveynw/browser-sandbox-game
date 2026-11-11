# Self-recovering traffic — implementation handoff

## Objective and scope

Make ambient traffic recover from intersection jams through ordinary vehicle movement: waiting outside junctions, granting fair passage, making room safely, and rerouting. Recovery must work while a player watches, preserving each vehicle's identity and continuous position.

Do not despawn, teleport, reset motors, disable collision, or push vehicles through obstacles to resolve a jam. Keep distance-based population unloading for performance, independent of recovery. Player-driven cars are obstacles, never AI-controlled recovery participants. Preserve existing parking, possession, missions and police behaviour.

This is an implementation plan, not an implemented change. The current working tree contains unrelated gameplay changes; preserve them. Do not deploy or restart servers unless requested.

## Current implementation to inspect

Paths below are relative to the repository root.

- `crates/server/src/systems/population/mod.rs`: `Traffic`, `TrafficPhase`, `Navigation`, `Intersections`, `PopulationSystem::{direct, decide, vehicle_input, release_ai}` and existing tests.
- `crates/server/src/systems/movement/mod.rs`: AI decisions precede authoritative motor integration; `vehicle_input` supplies inputs to the shared motor.
- `crates/sim-core/src/collision.rs`: vehicle shapes, collision/sweep queries and character shapes.
- `crates/sim-core/src/`: shared vehicle motor, including steering, braking and reverse semantics. Verify these before implementing recovery inputs.
- `crates/content/src/` and the loaded manifest: road graph, directed edges, lane geometry and parking destinations.
- `crates/server/src/systems/police/mod.rs`: independently controlled vehicles that must remain physical obstacles.

Observed behaviour:

- AI decisions run every six ticks (5 Hz at 30 Hz simulation); route work is limited to four jobs per tick.
- Intersection keys currently round route-target coordinates to 16 m cells. Reservations expire after 120 ticks, even if their holder is still crossing. Sorted IDs decide simultaneous requests.
- Every car brakes when its three forward rays or actor proximity checks detect an obstacle, including a car with intersection priority.
- After 150 ticks without sufficient displacement, the planner selects another distant goal. This does not necessarily resolve the local blockage.
- After 450 ticks without progress, `direct` can remove a stuck car if hidden from observers.
- `vehicle_input` recomputes throttle from `target_speed` every tick. Recovery steering/throttle must survive this layer; merely setting `b.input.move_z` will not suffice.
- Progress currently means displacement of at least 2 m. Repeated forward/backward motion could incorrectly count as recovery.

These rules allow a reservation holder to be physically blocked by another vehicle that is waiting for the reservation. The specific observed jam has not been reproduced, so begin with diagnostics and a deterministic fixture.

## Behavioural contract

1. A car enters only after acquiring passage and confirming enough space beyond its exit for its full footprint plus a safety gap.
2. A crossing car retains passage until its rear clears the conflict area. A timeout alone never grants conflicting passage through a physically occupied junction.
3. Waiting priority ages; entity ID is only a deterministic tie-breaker. Normal waiting must not trigger unnecessary recovery.
4. A blocked car records what prevents movement. Recovery coordinates those cars rather than independently issuing contradictory manoeuvres.
5. Every manoeuvre uses normal motor inputs and collision checks over the intended swept vehicle footprint. Recheck safety every simulation tick, especially when reversing.
6. Prefer a small safe manoeuvre; reroute when local recovery cannot restore passage. Preserve the original destination when practical.
7. If no safe motion exists, wait and retry when the obstruction changes. A player sealing every exit cannot be guaranteed to clear; report this state rather than forcing unsafe motion.

## Proposed state and ownership

Keep the traffic controller server-owned. Extract a focused traffic/intersection module if it makes `population/mod.rs` easier to maintain; avoid a general-purpose scheduling framework.

Suggested structures (names can follow repository conventions):

- `IntersectionGeometry`: stable ID, approaches/stop lines, conflict area and exit clearance corridors, derived from the road graph and lane widths. Validate junction grouping; do not assume rounded coordinates identify every crossing correctly.
- `PassageRequest`: car, approach, intended exit, first wait tick. Repeated requests preserve wait age.
- `PassageGrant`: holder, entry/exit, approach/crossing/clearing state and last meaningful progress. Start with one crossing at a time; parallel nonconflicting movements are optional later work.
- `Blockage`: cause (`Vehicle`, `Pedestrian`, `Static`, `ExitOccupied`, `WaitingForPassage`, `NoRoute`), blocker IDs, first blocked tick and last observed geometry. Retain all relevant blockers, not just the first ray hit.
- `RecoveryState`: manoeuvre, bounded waypoints, start/deadline, attempt count, clearance requirements, progress along the plan and retry cooldown.
- `RecoveryCoordinator`: temporary owner for a connected blocking group, participants, selected vehicle to clear first and manoeuvre-space claims. Clear state on possession, destruction, removal, route change or recovery completion.

Use stable iteration order and bounded storage based on the active traffic population. Timers use simulation ticks. Separate observed geometry, arbitration and output generation so decisions do not depend on which car happens to be processed first.

## Stage 1 — diagnostics and deterministic failure scenarios

1. Record blockage causes and blocker IDs during existing sensing without changing driving behaviour.
2. Add per-car blocked duration, intersection state and recovery state to existing diagnostics, or a debug-only inspection surface. Add counters for junction waits, detected cycles, recovery attempts/completions, reroutes and unresolved obstruction groups.
3. Build small deterministic road fixtures for crossing traffic, opposing turns and a blocked exit. Use actual vehicle motor integration and collision queries.
4. Reproduce a jam with observers keeping the cars visible. Record vehicle IDs, paths, distances and grants so later stages can demonstrate recovery without removal.

Acceptance: tests can distinguish a normal queue, a pedestrian wait and a circular wait; diagnostics remain bounded. Existing population tests continue to pass.

## Stage 2 — prevent junction blocking

1. Derive explicit junction approaches and exit corridors from the road graph. Place stop lines so the full vehicle stays outside the conflict area.
2. Require exit clearance before granting entry. Include stopped cars, parked cars, player vehicles, police and pedestrians. Claim sufficient receiving space so separate requests cannot both assume the same exit gap is available.
3. Replace fixed expiry with passage states. Release after the vehicle's rear clears. Cancel stale approaching requests safely; retain physical occupancy protection for stalled crossing cars and hand them to recovery.
4. Arbitrate by wait age, then stable ID. Stop cars before the line using braking distance, including different vehicle lengths and speeds.
5. Recheck changed exit occupancy after a grant. Stop safely if necessary and classify the blockage; never continue solely because a grant exists.

Acceptance: queues remain outside junctions; turns finish before conflicting entry; a grant expiring cannot cause a collision; higher-ID cars are not starved by repeated lower-ID arrivals.

## Stage 3 — detect and coordinate deadlocks

1. Build a wait-for graph from the current observation snapshot. Add edges for physical vehicle blockers and passage dependencies. Keep static/pedestrian/external blockers distinct from cooperative AI participants.
2. Find strongly connected components for circular waits. Also detect single-car wedges and chains ending at a persistent external obstacle; these need recovery even without a cycle.
3. Require sustained lack of useful route progress before escalation. Exclude normal queued movement and short pedestrian crossings. Use hysteresis to avoid rapid state changes.
4. Select a deterministic recovery plan using existing passage, safe available manoeuvres and wait age. Assign one coordinator per blocking group; reserve manoeuvre space to avoid independent reversals colliding.
5. Distinguish permission from feasibility: the selected car proceeds only after the other participants have actually made enough room.

Acceptance: a two-car circular wait and a multi-car cycle receive stable plans; an ordinary moving queue does not. Possession/removal of a participant invalidates its plan immediately.

## Stage 4 — safe local manoeuvres and rerouting

1. Implement bounded recovery actions: hold outside conflict area; reverse slowly into a verified gap; reverse with a steering correction; advance along a short clearance path; resume the intended route.
2. Evaluate candidate paths using full vehicle footprint sweeps through intermediate poses. Include rear/side clearance, pedestrians and other participants. Do not rely solely on the existing forward rays or endpoint checks.
3. Drive through the normal motor using explicit signed speed targets and appropriate braking/steering. Check safety each motor tick, not only at 5 Hz planning frequency. Stop if a player/pedestrian enters the manoeuvre path.
4. Measure useful route/clearance progress, not total displacement. Bound attempts and prevent alternating forward/reverse indefinitely. Retry after relevant obstacle changes or a controlled cooldown.
5. When local manoeuvres fail, plan from the actual reachable lane position, temporarily penalizing blocked approaches. Keep directed-road constraints and the four-jobs-per-tick budget. Do not repeatedly select an unreachable route-front node or random distant goal as a substitute for local recovery.
6. End recovery once passage is restored; release claims and resume normal traffic rules. If no safe alternative exists, retain a diagnosed waiting state and retry when conditions change.

Acceptance: opposing cars and turning conflicts clear through continuous safe motion; a temporarily parked player car causes waiting/rerouting and traffic resumes when it moves; a pedestrian appearing behind a reversing car causes a safe stop.

## Stage 5 — remove stuck-car deletion and validate in the city

1. Remove the stuck-duration removal condition from `PopulationSystem::direct` for traffic. Retain independently justified distant/unobserved unloading; do not relabel stuck deletion as another cleanup path. Pedestrian recycling is outside this task.
2. Update population tests that relied on recycling stuck traffic. In recovery scenarios keep observers nearby and assert the same vehicle IDs survive throughout.
3. Exercise parking, possession, disabled vehicles, mission-reserved cars and police interactions. Recovery must not claim or command cars after ownership changes.
4. Add a deterministic city soak with adversarial congestion and a fixed seed. Record unresolved groups and progress, not just population totals. Inspect several seeds after deterministic fixtures pass.
5. Verify visibly in the local game: watch a jam form, clear itself and resume movement without disappearance, teleportation or obvious oscillation.

## Required regression matrix

- Four approaches arrive simultaneously; all eventually cross with bounded waiting in a clear junction.
- A long van occupies the exit; following traffic waits before entry until its full rear clears.
- Two opposing turns and a three-or-more-car circular wait clear without removal.
- A crossing car is held longer than the former four-second reservation; no conflicting entry occurs.
- A pedestrian enters the rear path during reversal; the car stops and later resumes safely.
- A player blocks an exit, then moves; traffic recovers without a reset. Permanently blocked exits produce a stable unresolved state, not unsafe driving.
- A car becomes player-controlled or disabled during recovery; its AI commands and claims are released appropriately.
- Reversing/creeping without route progress cannot reset the stuck timer forever.
- Identical seeds and observations produce identical plans and motor traces.
- Per-tick path work and recovery candidate counts stay bounded at the configured traffic cap.
- Full-footprint collision safety, vehicle identity and continuous position hold throughout every recovery fixture.

Use measured fixture-specific recovery deadlines with a small allowance for vehicle dynamics; define those deadlines before tuning the implementation. Do not promise a global clearance deadline for physically impossible situations.

## Verification and delivery

Run focused traffic tests after each stage, then server tests and relevant sim-core tests if collision or motor code changes. Run frontend checks only if diagnostics/UI changed. Confirm current test target names in Cargo before executing them. Record any pre-existing warnings separately.

Deliver implementation, regression fixtures, updated gameplay/networking documentation where applicable, and a short verification note with seeds, recovery timings, collision checks and any unresolved scenarios. Update this document with completed stages and deviations. Any visual verification not performed must be stated explicitly.

No protocol change should be necessary for the recovery itself: clients already receive authoritative vehicle movement. Keep debug additions out of normal gameplay UI and avoid raising population or connection limits as part of this work.
