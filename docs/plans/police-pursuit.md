# Police pursuit upgrade — staged implementation plan

Status: Stage 1 and the Stage 2 driving and recovery pass are implemented. Stage 3 pursuit roles and low-speed contact are implemented. Stage 4 foot pursuit and combat are implemented. Stage 5 presentation and diagnostics are implemented. Browser qualification and multiplayer performance measurements remain pending.

## Goal

Make police a readable, escalating threat: competent vehicle pursuit, coordinated interception, officers who can pursue and shoot on foot, and wanted levels that change tactics as well as numbers. Preserve fair escape, server authority, physical movement and bounded multiplayer cost.

Keep three stars for this iteration. Helicopters, five-star escalation, drive-by shooting, destructible cover and a general navigation mesh are outside scope.

## Current implementation

- `crates/server/src/systems/police.rs` owns wanted state, patrol allocation, driving and arrest. There are at most eight police cars globally, with demand of one car per star and at least two patrols. Allocation rotates across wanted players.
- Witnessed crimes add one star, at most once per second, capped at three. Crime categories affect witnessing but not severity. Sight loss starts searching after five seconds; searching removes one star every twenty seconds. A stationary suspect near a car's front can be arrested after three seconds.
- All levels use the same driver. Steering decisions run at 5 Hz, replanning at most 2 Hz per unit. Cars generally target 4 or 12 m/s, slow to at most 8 m/s on the short direct approach, and stop for sensed obstacles. There is no interception or reversing recovery. Hidden cars stuck for fifteen seconds can be removed.
- `Navigation` in `systems/population.rs` already finds travel-time routes over directed edges. Problems include nearest-node attachment, restarting from an existing route-front node, simplistic waypoint tracking and no temporary obstruction costs. Replacing the search algorithm alone will not fix driving.
- Police have vehicles but no officer entities. `systems/combat.rs` contains player-fired hitscan, weapon timing and damage handling. Pedestrians have character motors but no combat controller.
- `Heat` in `crates/protocol/src/types.rs` and the client HUD support three stars. Replication distinguishes police vehicles; officers will need their own replicated identity and presentation.

## Intended escalation

Initial tuning values below are proposals, to be adjusted against deterministic scenarios. Unit counts are requested allocations, not guarantees under global demand.

| Stars | Response | Driving | Officers | Time without contact to lose a star |
| --- | --- | --- | --- | --- |
| 1 | One patrol; stop and arrest | Close pursuit, safe overtaking, no deliberate ramming | One pistol officer; arrest first, fire only against an active lethal threat | 20 seconds |
| 2 | Two units; pursuer plus interceptor | Predict junction exits; controlled vehicle contact when safe | Up to two pistol officers per car; reposition and use cover | 35 seconds |
| 3 | Three units; chase, intercept and contain | Coordinated boxing-in and controlled ramming against vehicles | Up to two officers per car; one rifle-equipped officer per response group | 50 seconds |

Use severity points with configurable thresholds, separate from displayed stars. Theft is minor, harming civilians is serious, attacking officers is severe. A single shot can produce both a noise report and damage: merge reports by causal event and use the highest applicable severity, rather than counting twice. Burst gunfire raises alarm without instantly reaching maximum wanted level from three frames of input. Keep bounded event deduplication.

Separate wanted severity, pursuit phase and permission to fire. Stars alone do not make every officer shoot immediately: two/three-star officers can engage a visibly armed, resisting suspect after a readable warning; an actual attack permits immediate defensive fire. Define resistance through explicit events (firing, aiming at an officer, or fleeing after warning), not simply owning a gun. Stop firing when surrender/arrest begins. Add a surrender action that lowers the weapon and holds the player stationary; movement or firing cancels it. A nearby officer with sight must still complete the arrest timer. Preserve safe vehicle exit, mission failure and respawn handling.

The escape loop remains pursuit → lost contact → search → gradual reduction → clear. Search uses the last observed position, heading and speed, with an expanding bounded search area. Hearing supplies an approximate investigation location, not exact visual tracking. New confirmed contact resets the relevant search timer; no live player position is available to planning while unseen. Show flashing stars during search and a clear arrest/surrender indicator.

## Ownership and boundaries

Split `police.rs` into focused modules as the stages require; retain `PoliceSystem` as the coordinator rather than introducing a generic AI framework.

- `WantedState`: severity, stars, pursuit phase, observed contact, warning/threat state, decay and arrest timers, causal crime history.
- `ResponseProfile`: per-level unit demand, speed/spacing limits, tactics, officer loadouts and search parameters.
- `ResponseAssignment`: target, unit role, assignment age and reassignment cooldown. Preserve assignments unless invalid or demand requires a fair change.
- `PoliceUnit`: vehicle, roster, route, manoeuvre state, progress along route, obstruction and recovery state.
- `PoliceOfficer`: stable entity ID, character/health state, assigned vehicle and target, behaviour state, weapon state and perception memory. Own officers in police state; reuse character primitives without making them ambient civilians or fake player sessions.
- `ShotRequest` / damage source: shared authoritative combat input for players and NPCs, with shooter identity and faction. Keep player network validation separate from shot resolution.

Tick flow: collect observations and crime outcomes → update wanted/assignments → plan unit/officer intent on bounded schedules → integrate normal motors → resolve validated shots and damage → apply lifecycle transitions → replicate. Document one-tick observation latency explicitly and ensure a dead/arrested/disconnected target cannot receive later queued attacks.

## Stage 1 — meaningful wanted levels and observable decisions

1. Extract configurable response profiles and severity rules, retaining the existing three-star protocol range.
2. Preserve crime kind/severity through the queued crime record; merge duplicate reports before applying severity. Separate noise, confirmed sight and attack reports.
3. Implement level-dependent search decay and threat/warning state. Initially expose officer-specific policies as data until officers exist.
4. Stabilize dispatch assignments with minimum tenure and fair rotation under contention. Keep the global eight-car cap initially; do not allocate eight per player. Search uses assigned units, with no free replacement wave on each replan.
5. Add bounded diagnostics for target, role, phase, last contact, route cost, blockage, path work and assignment changes. Establish scripted baseline chase scenarios before tuning driving.

Acceptance: a theft, civilian attack and officer attack produce distinct outcomes; duplicate causal reports do not inflate severity; unseen suspects cannot be tracked through walls; death/disconnect clears assignments; competing wanted players receive fair service. Verify existing arrest and mission regressions.

Stage 1 tuning uses thresholds of 1, 5 and 10 points. Witnessed theft and heard shots add one point, witnessed harm adds five and damage to a police car adds ten. Noise alone caps at one star and contributes at most once per second. Civilian crimes cap at two stars. Damage to police is required to reach three. When sight is lost, search starts after five seconds and reduces the current tier after 20, 35 or 50 seconds. The existing metrics endpoint reports active wanted phases, unit targets, blocked units, path jobs and assignment changes. Officer warning policy and attacks on police are recorded for the officer stage.

Local playtest: fire a shot near a patrol, then get out of sight and watch the pursuit switch to searching. Compare with a witnessed attack on a civilian or a hit on a patrol car. Refresh contact mid-search to verify that the decay timer resets.

## Stage 2 — better routes and aggressive, controlled driving

1. Extract reusable directed-road navigation without changing civilian driving policy. Attach cars to reachable lane segments using heading and footprint clearance, including an off-road re-entry path; do not blindly choose the nearest node.
2. Add turn, U-turn and temporary blockage costs. Use bounded A* with an admissible travel-time heuristic, or retain the existing search if profiling supports it. Bound node expansions as well as job count; stagger jobs across ticks. Cache/reuse valid routes and replan on meaningful changes.
3. Follow a speed-dependent point ahead along the route rather than steering at each node. Calculate target speed from curvature, braking distance, visibility and response profile. Police may exceed normal traffic speed within vehicle limits and use checked overtaking corridors; walls, actors and occupied exits remain constraints.
4. Implement observe → brake → checked reverse/turn → rejoin/reroute recovery using signed speed targets. `vehicle_input` currently rewrites throttle, so recovery must survive this layer. Check full swept footprints and actors each motor tick, including behind the car. Measure route progress, not mere back-and-forth displacement.
5. Remove stuck-duration removal as a police recovery mechanism once recovery scenarios pass. Keep ordinary distant retirement and wreck lifecycle separate. If every exit is physically blocked, wait and retry on changed geometry instead of teleporting or forcing motion.

Acceptance: actual motor/collision fixtures cover sharp turns, a missed waypoint, wrong-facing starts, blocked lanes, a watched intersection jam and a player stepping behind a reversing car. Preserve vehicle IDs and continuous movement. Capture baseline and improved chase times on the same routes.

Coordinate shared intersection/clearance interfaces with `docs/plans/traffic-recovery.md`. That document is a proposal, not an implemented dependency. Do not create competing reservation systems: if shared passage arbitration is needed here, implement the narrow common primitive and document the remaining civilian work.

The Stage 2 pass uses reachable lane entry points, cached directed routes and at most two route searches per tick. Pursuit cruise targets up to 29 m/s, subject to the vehicle and braking limits. After 1.5 seconds of sustained blockage or poor alignment, patrols can back up at up to 2 m/s, check clearance every tick and retry from a new lane entry. Recently blocked approaches receive a temporary routing penalty. Stuck patrols keep their vehicle identity. Same-lane overtaking and measured before/after chase times remain open.

## Stage 3 — coordinated vehicle tactics

1. Assign distinct lead pursuit, interception and containment roles. Estimate reachable junction arrivals from recent observed velocity and road routes; choose an intercept the police car can actually reach in time.
2. Bound prediction by observation age. On contact loss, distribute units across plausible exits around the last-known location rather than converging on the suspect's live position.
3. Add overtaking and boxing-in manoeuvres with space claims, separation and abort conditions. Higher stars unlock controlled contact only against the target vehicle, with relative-speed and angle limits. Never deliberately ram an on-foot suspect. Use normal collision/damage rules; do not bypass collision by ignoring the target.
4. Emergency intersection priority cannot revoke a crossing vehicle's physical occupancy. Police wait for clearance or choose another approach. Fixed roadblocks are deferred unless the placement/traffic interaction is proven safe.

Acceptance: two cars take different useful routes; three cars do not merely queue behind the same obstacle; a sudden player turn invalidates an intercept cleanly; loss of sight ends privileged tracking; contact tactics abort near civilians or unsuitable geometry.

The Stage 3 pass assigns a lead car and up to two support cars. Support cars project separate exits from a vehicle sighting no older than 0.6 seconds, reject routes outside their estimated travel budget, and split search directions after sight loss. The lead can close to low-speed contact with the confirmed target vehicle at two or three stars if approach angle, closing speed and bystander clearance allow it. Coordinated side-by-side boxing, same-lane passing and emergency junction priority remain open. Use a two- or three-star vehicle chase to playtest the roles and route choices.

## Stage 4 — officers on foot and shared combat

1. Extract reusable shot resolution from player fire handling, preserving range, spread, muzzle obstruction, nearest-hit ordering, reload/cadence, spawn protection and death transitions. NPC fire is generated on the server and does not need a network peer. Distinguish police damage attribution in elimination messages; no NPC name lookup through player sessions.
2. Add officer entities with mounted → safe exit → approach → warn/arrest or engage → search → return/board states, plus incapacitation. Start with one officer per car, then enable profile rosters. Reserve safe exit/boarding positions; never instantiate an officer in a wall. Mounted officers are not separate exposed character hitboxes; vehicle damage remains authoritative.
3. Navigate using the pedestrian graph plus collision-checked local approach segments. Use simple reachable cover candidates from existing geometry and the patrol car, verified by sight and movement queries. No navigation-mesh dependency. Handle blocked approach and reboarding through bounded retries and alternate routes.
4. Give officers finite magazines, reloads, reaction delay, visible aim wind-up, movement accuracy penalties and burst pauses. Reuse pistol/rifle definitions; tune NPC aim separately from player weapon balance. Only fire with fresh sight and a clear shot; revalidate at fire time. Vehicles block shots, so attack a fleeing occupied vehicle rather than shooting its driver through the shell.
5. Allies and bystanders block fire: officers withhold unsafe shots; any accidental hit follows explicit faction/damage rules and is never attributed as a player crime. Officers cannot attack protected, downed, arrested or disconnected players. Player hits on officers enter the same authoritative hit ordering and severity pipeline.
6. Implement surrender/arrest against nearby officers and release weapons/targets immediately on lifecycle changes. Define roster persistence: disembarking/reboarding never creates a replacement officer; incapacitated officers remain dead until ordinary unit retirement/reinforcement rules allow replacement.

Acceptance: officers can exit, walk, acquire a target, fire/reload, take damage, arrest and reboard. Walls/cars block bullets, nearby allies inhibit fire, player protection works, and NPC damage produces correct death/respawn without requiring a session. Test vehicle destruction while mounted and while the roster is outside.

Stage 4 uses the existing police descriptor with character state for officers, so no wire types changed. Officers use the sidewalk graph, safe vehicle exits, finite ammunition, reloads, and the same hit ordering as player weapons. Mounted officers are excluded from replication and shots. Hold G while stationary to surrender. Warning is currently shown through the officer raising their weapon and the HUD surrender hint. Richer warning and animation cues belong to Stage 5.

## Stage 5 — replication, visuals and playable tuning

1. Ship the minimal officer descriptor/state replication alongside Stage 4 so each stage remains testable. Extend protocol source and regenerate derived TypeScript/contracts; inspect exhaustive entity-kind handling, interest admission, baseline/delta updates, removal and reconnect paths. Do not hand-edit generated files.
2. Render distinct uniforms and equipped weapons using existing character/weapon assets. Reuse muzzle flashes, tracers, impact effects and spatial audio with server-approved shot events. Include aim/reload/incapacitation cues and visible surrender. Keep officers anonymous rather than giving them player nametags or online-player map dots.
3. Add star/search feedback, short escalation/warning notices and arrest progress. Keep detailed AI diagnostics in the 98.css tools collection, not the game HUD. Add deterministic tool scenarios for chase, interception, blocked-road recovery, officer firefight and escape.
4. Set a hard officer cap (initial ceiling: sixteen globally with eight cars), concurrent-shooter limits per target, staggered perception and route budgets. A full roster is a capacity ceiling, not a requirement that all sixteen shoot one player. Evaluate 64 connected players, including many simultaneously wanted; raise caps only after measurement.
5. Tune reaction time, accuracy, pursuit speeds and search durations using both on-foot and vehicle escapes. Record pursuit contact time, successful escapes, time-to-incapacitation, unresolved blockages, assignment fairness, server tick cost and replication volume.

Acceptance: Chrome and Firefox show consistent officer state and effects; late join/reconnect reconstructs active pursuits; search is escapable; stationary surrender works; one star is manageable and three stars demands active evasion. Under a fixed stress scenario, bounded work remains within the 30 Hz simulation's tick budget with documented headroom and no sustained backlog.

## Validation and delivery

Each stage delivers focused deterministic regressions and a short local playtest recipe. Reuse existing police, combat, population, protocol and simulation fixtures; add motor-integrated scenarios rather than only testing chosen steering values. Test multiple wanted players, officer/car destruction, possession, disconnect, respawn, safe exits and ID cleanup throughout.

Run relevant Rust tests per stage; regenerate/check protocol when wire types change, and run frontend tests/typechecking for rendering/HUD changes. Finish with the repository's full `npm run check` and production build. Record unperformed visual or load checks explicitly. Restart the local game after major implemented changes so the user can test; remote deployment remains a separate requested action. Update this document with completed stages, tuning values and deviations.

Recommended delivery order: Stage 1 establishes escalation and diagnostics; Stage 2 improves the existing chase immediately; Stage 3 adds coordination; Stage 4 adds armed officers with minimal rendering; Stage 5 finishes presentation and multiplayer balance.

## Combined Stage 4–5 verification

The client adds police caps, badges, belts, search-star feedback, escalation notices, a nearby armed-officer warning and a local surrender pose. The /police tool uses the shared 98.css shell and displays live bounded officer, patrol, timing and network diagnostics with manual scenario recipes. Shared firing opportunities are capped at one per target every twelve ticks.

Server regressions cover deployment, surrender, shooting, search, collision recovery and dispatch fairness at the configured player capacity. This checkout currently sets MAX_PLAYERS to 16. The attempted 64-player admission test exposed that limit, so 64-player performance remains unverified. The monitor scenarios are manual recipes, not automated replay controls. Chrome/Firefox visual checks, remote surrender animation, and production tick-budget measurements remain open. Earlier deferred overtaking/boxing work is still listed in its respective stage.
