# 01 — Product and game design

[Index](../README.md) · Next: [City and content](02-world-and-content.md)

## Experience

Working title: **Port Meridian**. A sunlit coastal industrial city where players take short driving jobs, explore alleys and shortcuts, steal unoccupied vehicles, and escape police pursuit. The tone is playful, readable, and stylized. Multiplayer makes the city social: another player can follow your route, race alongside you, or cause a nearby police response.

The city should be enjoyable within one minute of entering the browser: choose a display name, join, walk to a visibly available car, get in, and drive toward a job marker. No account registration, tutorial wall, or loading screen after every street. The required deployment is localhost: a locally launched Rust server and browser app, tested with multiple browser contexts. Public hosting is outside this implementation scope.

This design assumes a small independent project and staged implementation by a less capable coding agent. Build a convincing short loop before increasing content. The initial audience is desktop browser players on laptops and desktops.

## Release boundaries

| Release | Required experience | Deliberate limits |
|---|---|---|
| Technical proof | Two browser tabs exchange typed data through both crate transports | No city or game UI required |
| Vertical slice | Two players share Old Quay, walk, drive one car type, finish a delivery | Greybox, no pedestrians/combat/police/persistence |
| Complete alpha | Up to 16 players in the full city; traffic, jobs, police, pistol, basic guest saves | One seat per car, PvP off, modest NPC counts |
| Later | Passenger seats, parties, PvP rules, interiors, expanded progression, mobile/gamepad | Each requires its own design/change request |

No aircraft, boats, swimming, destructible buildings, physics debris, vehicle flipping, vehicle customization, voice/text chat, user-generated content, monetization, gangs, housing, or seamless shard migration in alpha.

## Core loops

1. **Explore:** spawn at a safe plaza → find a vehicle → learn roads and shortcuts → discover job terminals.
2. **Earn:** accept one job → follow world marker and minimap → complete server-verified checkpoints → receive session credits.
3. **Risk:** steal a marked vehicle or attack an NPC → build heat → evade sight → survive the search timer → resume work.
4. **Recover:** get downed/arrested → respawn at a safe point after a brief delay → retain accumulated money → choose another route.

Typical jobs last 90–180 seconds. A useful initial session is 10 minutes. Money is a score and repair resource; no progression grind gates access to movement or cars.

## Controls

| Input | On foot | Driving |
|---|---|---|
| W/S | Forward/back relative to camera yaw | Throttle/reverse or brake before reversing |
| A/D | Strafe relative to camera yaw | Steer |
| Mouse | Orbit camera | Orbit camera; smoothly recenter after 1.5 s inactivity |
| Shift | Sprint | No boost |
| Space | Jump on press | Handbrake while held |
| E | Enter nearest eligible car within 2 m; otherwise interact with terminal | Exit if speed < 2 m/s and a safe exit exists |
| F | Use nearby job terminal | Use nearby job terminal or request repair while stopped |
| Right mouse | Aim pistol | No action |
| Left mouse | Fire pistol if aiming | No action |
| R | Reload | No action |
| M | Toggle full map | Toggle full map |
| Escape | Release pointer lock and open pause/settings overlay | Same; multiplayer world continues |

On pointer-lock loss or hidden tab, send neutral controls and stop creating gameplay actions. Background-tab throttling must never leave acceleration or firing latched. Server input expiry independently enforces this.

Controls should be remappable locally. Default camera sensitivity, inversion, volume, reduced motion, and rendering quality belong in local settings. Touch/gamepad support is later.

## Player rules

- Capsule height 1.8 m, radius 0.35 m; feet are the gameplay position reference.
- Walk 4 m/s, sprint 7 m/s, backward/strafe 4 m/s; normalize diagonal movement.
- Jump takeoff speed 5.5 m/s; gravity 18 m/s²; no double jump, crouch, climbing, or stamina.
- Health 100; armor absent. Downed players cannot control, collide with, or damage actors.
- Respawn after 5 seconds at the nearest eligible safe spawn. Reset health, heat, pistol ammo, and active job. No cash penalty in alpha.
- Spawn protection lasts 3 seconds and ends on firing, taking a car, or accepting a job. Show it clearly.
- Player/player collision and player-caused damage to other players are disabled in alpha. Player cars still collide with cars and world objects; car impacts cannot damage other players.
- Display names: 3–20 Unicode scalar values after trim; reject control characters; render as plain text.

## Vehicle rules

Cars use arcade kinematics: responsive steering, collision sliding, simplified braking, cosmetic body lean and wheel suspension. The car stays upright; wheels do not participate in a full rigid-body suspension simulation. This is an intentional alpha feature boundary.

| Class | Top forward speed | Reverse | Acceleration | Brake | Wheelbase | Health |
|---|---:|---:|---:|---:|---:|---:|
| Compact | 22 m/s | 7 m/s | 7 m/s² | 12 m/s² | 2.5 m | 100 |
| Van | 18 m/s | 5 m/s | 5 m/s² | 10 m/s² | 3.2 m | 160 |
| Sport | 30 m/s | 8 m/s | 9 m/s² | 14 m/s² | 2.7 m | 90 |

Only the driver seat exists. Entry is an atomic server operation; first valid request wins. Traffic cars must be stopped and unoccupied to become enterable; there is no animated carjacking. Destroyed cars stop, smoke, become non-enterable, and respawn after 30 seconds at a clear parking spawn. Explosion damage is later.

Unoccupied parked public cars are available freely. A subset marked “secured” adds one heat level on theft if a police witness has line of sight. A player’s current vehicle is never despawned by distance culling.

## Jobs and economy

| Job | Action | Success | Failure | Reward |
|---|---|---|---|---:|
| Courier | Accept at terminal, drive/walk parcel to a fixed destination | Stay within 5 m for 1 second at < 2 m/s, on foot or in car | 180 s elapsed, death, arrest, cancel | 150 credits |
| Time trial | Accept at start and drive ordered checkpoints | Finish all checkpoints within 120 s | Wrong-way is allowed; timeout/death/arrest/cancel fails | 200 credits |
| Vehicle recovery | Accept, collect a designated secured car, return it | Correct vehicle enters garage zone at < 2 m/s and health > 0 | Vehicle destroyed, 180 s elapsed, death/arrest/cancel | 250 credits |

Only one active job per player. Jobs are individual; anyone may watch or accompany. Shared rewards and cooperative ownership are later. Recovery spawns are reserved per active mission and released on every terminal path. A mission reward is credited once, keyed by mission instance ID.

Repair bays cost 50 credits, require no active pursuit, vehicle speed < 0.5 m/s, and 2 seconds in a 5 m zone. A repair restores health only, not money/ammunition. No trading or paid items.

Complete alpha persists only guest profile ID, display name, credits, and completed-job count to local SQLite. World position, vehicle possession, heat, weapons, and active jobs reset on reconnect. The slice uses memory only. See [server persistence](07-server-interfaces.md).

## Police and combat

Heat 0–3 is private player state, represented by three clear HUD pips. Detection requires a police unit’s distance and line of sight, except shooting, which also produces an audible stimulus. Police use the same locomotion rules as other cars/characters.

Alpha police pursue and arrest; they do not shoot. One pistol provides a basic combat loop against NPCs and vehicles. No gore or civilian death animation is required; pedestrians flee and can be temporarily incapacitated. One hitscan ray per shot, 80 m range, 25 damage, 300 ms cooldown, 12-round magazine, unlimited reserve with a 1.5 s reload. Server raycasts determine hits.

PvP and lag-compensated competitive shooting are explicitly later. Alpha shooting uses current authoritative positions; its responsiveness limitations under latency must be visible in tests.

The exact state transitions and damage rules live in [Gameplay systems](09-gameplay-systems.md).

## Completion checks

- A new player can find and enter a car without reading a separate manual.
- Two players see the same car occupancy and job-related world changes.
- A 10-minute playtest includes walking, driving, a completed job, a pursuit, and recovery.
- No UI claims that guest saves, offline play, mobile controls, or reconnect continuity exist before their tasks are implemented.
- A disabled or later feature has no misleading active button.
