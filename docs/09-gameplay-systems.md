# 09 — Gameplay systems

[Index](../README.md) · Numerical tuning: [Game design](01-game-design.md)

All state machines run on the server. Clients can anticipate animations/sounds but cannot commit outcomes.

## Vehicles and possession

Vehicle lifecycle:

```text
Parked/Traffic -> AvailableStopped -> Occupied -> AvailableStopped
any live state -> Disabled -> RespawnPending -> AvailableStopped
```

Traffic cars retain their AI driver while moving; after stopping in a designated parking spot, the AI driver disappears and the car becomes available. There is no visible forcible carjacking.

Entry preconditions:

- Player is on foot, alive, unprotected after this action, and not arrested.
- Vehicle is visible/relevant, health > 0, not disabled, has no driver, and speed < 0.5 m/s.
- Distance from player feet to driver entry anchor ≤ 2 m.
- A static line segment from player chest to entry anchor is unblocked.
- If a car is mission-reserved, only the owning mission player can enter.

Entry transaction: assign driver, set player mode driving and vehicle ID, remove player movement collider, place player at seat anchor, increment owner control epoch, clear queued old inputs, emit ControlChanged. Update vehicle/player replicas in the same tick. A losing simultaneous request gets `occupied`.

Exit requires speed < 2 m/s. Test left door, right door, rear in that order. Use template offsets transformed by car yaw; each candidate requires floor support and an empty capsule plus a clear segment from the door. If all blocked, remain seated and return `exit_blocked`. On success restore capsule and on-foot mode, increment epoch, emit ControlChanged. Driver disconnect stops the car and clears its driver ID.

Vehicle damage: static/car collision damage `clamp((impact_delta_speed - 6) * 5, 0, 50)`, at most once per counterpart per 15 ticks. Hitscan pistol damage is 25. At health 0 force occupant downed at nearest safe exit (or nearest safe spawn if no exit), disable car, cancel relevant recovery job, and schedule respawn after 900 ticks. Do not force a capsule inside a wall.

Respawn uses the car’s home parking slot if clear, otherwise retries every 30 ticks for up to 30 seconds then selects another compatible clear slot. No respawn on top of players. Mission-owned cars are released through mission cleanup instead of ordinary car respawn.

## Combat

Weapon state per player: magazine 0–12, next_fire_tick, reload_end_tick or null. Aim mode is a movement/UI state; a Fire request still includes bounded aim angles so the server has an explicit shot direction independent of unreliable input arrival.

Fire validation: on foot, alive, not reloading, magazine > 0, current tick ≥ next_fire_tick, valid epoch, finite aim angles within protocol limits. Set next fire time to S + 9 ticks and decrement magazine once. No client timestamp or reported victim is trusted.

Origin is server player feet + `[0,1.45,0]`, with a small forward offset checked against geometry. Positive aim pitch looks up. Ray direction is `[-sin(yaw)*cos(pitch), sin(pitch), -cos(yaw)*cos(pitch)]`; max distance 80 m. Query static shot blockers, vehicles, and NPC hit volumes. Ignore shooter and other players for damage; other player hit volumes should not absorb shots when PvP is off. Use earliest valid hit.

One successful shot returns ActionResult and broadcasts a Shot event to relevant observers. Damage comes from authoritative raycast, then replicated health/state. Local muzzle flash can appear immediately; authoritative endpoint/hit marker arrives later. Display hit marker only for server-confirmed damage.

Reload accepted when magazine < 12 and not already reloading; completes after 45 ticks. Fire during reload returns `cooldown`. Switching to driving/downed cancels reload; respawn gives full magazine. No reserve-ammo inventory in alpha.

Downing/arrest disables controls immediately. If driving, clear the car's driver, select a safe exit or safe respawn anchor, restore the player entity as the controlled actor, and send a new reliable control epoch. Respawn also increments epoch and sends ControlChanged. Ordinary nonfatal health changes do not change control epoch.

Pedestrian at health 0 becomes incapacitated for 10 seconds, then despawns and later repopulates elsewhere. Police at health 0 is removed after a short non-graphic effect and frees its unit slot. No loot is spawned.

## Crimes and pursuit

Only server observations create crimes:

| Event | Heat change | Witness condition |
|---|---:|---|
| Enter a secured parked car | +1 | Police within 50 m with clear line of sight |
| Fire pistol | +1 | Police within 80 m can hear; no line of sight required |
| Damage pedestrian or police | +1 | Police within 60 m with line of sight |
| Damage a police vehicle | +1 | Targeted police unit itself is a witness |

Clamp heat at 3. One causal action generates at most one heat increment even if several witnesses report it; deduplicate crime event ID. Repeated crimes can increase heat at most once per 30 ticks per player, but always refresh last crime/known-position time.

Per-player pursuit:

```text
Clear --witnessed crime--> Pursuing
Pursuing --no sight for 5 s--> Searching
Searching --sighted or new crime--> Pursuing
Searching --20 s since search began and no new crime--> heat-1
heat remains >0 -> Searching with timer reset
heat becomes 0 -> Clear
Pursuing --arrest conditions continuously 3 s--> Arrested
Arrested --5 s--> respawn, Clear
```

Search is canceled whenever sight returns. Police line-of-sight check runs at 5 Hz using static blockers, within 70 m, to player/car position. Police remain at/near the last known position while searching; they do not home onto an unseen target using perfect knowledge.

Maintain two patrol cars at heat 0, initially on Civic Center and Old Quay road loops, so crimes can have witnesses. They count toward the global cap and can be reassigned to pursuit. Requested pursuit budget by heat: 1, 2, 3 units, capped globally at 8 including patrols. Assign fairly round-robin across wanted players; never spawn 3 × 16 units. A police car pursues via road graph, updates target path at 2 Hz, and switches to direct steering only within 15 m with a clear drivable path. Police cars do not magically cross building lots.

Arrest conditions: player heat > 0, player/vehicle speed < 0.5 m/s, officer unit within 3 m, unobstructed line of sight, conditions continuous for 90 ticks. A car police unit counts its front interaction anchor. Arrest progress resets on movement or obstruction; show it in-world/HUD as locally inferred feedback but only server status confirms arrest. After 5 seconds respawn, no cash penalty, active mission fails.

If a pursuing unit is stuck (progress < 2 m for 5 seconds), replan. After 15 seconds stuck and outside all players’ clear view, despawn/reassign. Never teleport a visible police car.

## Ambient traffic and pedestrians

Traffic AI states: `Driving`, `Yielding`, `Stopped`, `Parking`, `Available`, `Fleeing` (optional later; do not require for alpha).

Use lane waypoints and a target speed up to 12 m/s. A front sensor detects obstacles/cars within `max(4, speed*1.5)` meters and brakes. Intersection right-of-way uses a deterministic reservation: first requesting car wins, expire reservation after 4 seconds, break same-tick ties by entity ID. Signals are cosmetic unless traffic behavior actually implements them.

Traffic decisions run at 5 Hz; their controls feed the same 30 Hz vehicle motor. Path calculations run at most 2 Hz per car and use a capped shared work queue. If no path exists, stop safely and retry; never move straight through geometry.

Pedestrians: `Walking → Waiting → Walking`, `Walking/Waiting → Fleeing`, `any → Incapacitated`. They move along sidewalk graph at 1.4 m/s and flee at 4 m/s away from nearby shot/impact for 5 seconds. No road-crossing behavior until explicitly authored crossing edges exist. Use the shared character motor with NPC desired direction, no jumping.

A vehicle crossing a pedestrian hit capsule at speed > 4 m/s causes `min(100, speed*8)` damage once per 15 ticks per pair. Player hit capsules are excluded from vehicle damage. Server world queries are swept from previous to current pose to prevent tunneling.

Population director runs once per second. It uses the caps, visibility-safe spawning, reservations, and despawning rules in document 02. Seeded RNG plus stable entity order supports repeatable tests.

## Mission interface and state

```rust
pub enum MissionPhase {
    Active,
    RewardPending { event_id: String },
    Completed,
    Failed { reason: MissionFailure },
}
pub struct MissionInstance {
    pub id: String,
    pub owner_profile_id: String,
    pub owner_player_id: EntityId,
    pub template_id: String,
    pub started_tick: u32,
    pub deadline_tick: u32,
    pub checkpoint_index: u16,
    pub dwell_ticks: u16,
    pub reserved_vehicle_id: Option<EntityId>,
    pub phase: MissionPhase,
}
```

Accept requires player alive, no mission (including pending reward), in terminal zone ≤ 3 m from its center, and template available. A recovery job also requires a free reserved car slot. Create an opaque unique instance ID and record deadline. No reward is precredited.

Courier: destination dwell counter increases each tick inside 5 m at speed < 2 m/s; resets on leaving/speeding. Success at 30 continuous ticks.

Time trial: require driving at acceptance, start terminal beside the start checkpoint, consume the initial start gate once, then advance ordered gates using the controlled vehicle’s swept segment. At most one checkpoint advancement per tick; finish gate is a distinct final index even though its position equals the start. Hitting the final location early does nothing.

Recovery: only the reserved vehicle satisfies delivery; use same 30-tick dwell in garage zone with speed < 2 m/s and health > 0. On success freeze/release it safely after the player exits automatically through validated exit logic; if no safe exit, keep the job active and display the blocked prompt until clear. No teleporting the player to complete.

Terminal conditions resolve in this order: cancel/death/arrest/disconnect/vehicle destruction → timeout → success. Once RewardPending, stop timeout progression and prevent new mission acceptance; retry persistence with the same event ID. On commit publish Completed plus credits. Keep terminal mission view for 5 seconds, then clear to null.

In memory-only slice the persistence adapter commits immediately with identical idempotency semantics. A committed memory reward remains through reconnect even if the client missed its success message. Process restart clears memory profiles, rewards and active/pending jobs; durable profile transactions are introduced in T17.

## Repairs

Require player driving the target car, clear heat/pursuit, speed < 0.5 m/s, inside repair zone continuously for 60 ticks, health > 0 and below maximum, and ≥ 50 committed credits. F requests the repair; UI shows dwell progress and the price.

The server validates dwell from observed motion; client cannot send “held for 2 seconds.” Submit one debit transaction, apply repair after successful commit if still eligible, otherwise refund as specified in document 07. Do not accept overlapping repair requests for a vehicle/profile.

## Gameplay scenario acceptance

| Scenario | Expected |
|---|---|
| Two players request the same car in one tick | One driver, one `occupied`, one ownership epoch change |
| Exit against left wall with right side free | Use right exit |
| All exits blocked | Reject and remain driver |
| Duplicate Fire action | One ammo decrement and one damage application |
| Fire through a building | Static hit, no actor damage behind it |
| Same crime with three witnesses | One heat increment |
| Wanted player enters occluded alley | Search begins only after sight timeout |
| 16 wanted players | At most 8 police units, fair reassignment |
| Revisit final checkpoint before earlier gates | No completion |
| Completion and death in same tick | Mission fails, no reward |
| Duplicate completion/DB retry | One credited economic event |
| Driver disconnects mid-job | Car released, player gone, mission reservations released |
