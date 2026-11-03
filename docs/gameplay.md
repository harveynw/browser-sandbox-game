# Game features and rules

[Documentation index](../README.md#documentation)

Port Meridian is a local multiplayer driving and exploration game with an authoritative Rust world, a procedural coastal city, ambient traffic and pedestrians, jobs, a pistol and police pursuit. One world supports up to 64 guests. Players can share the city, see each other move and drive, and take turns using cars. Missions, heat and money belong to individual guests. On-foot players can take weapon damage, become downed and respawn.

## City and presentation

The city spans **768 × 768 m**, with world bounds X/Z **−384 to 384 m**. North Heights, Civic Center and Ironworks form the northern districts; Old Quay, Market Grid and East Docks form the southern districts. Main road centerlines are at −256, −128, 0, 128 and 256 m; the outer ring is at ±352 m. Roads use directed lanes, intersections and driveway access. Pedestrians have a separate sidewalk/crossing graph.

Original landmarks include the Quay water tower, Market canopy, garage beacon, Civic clock, Ironworks chimney, dock cranes and Heights pavilion, with a distant hill/ocean backdrop. The renderer uses procedural low-poly geometry, articulated characters with four clothing palettes, vehicle detail and cosmetic motion, facade patterns, signs, pooled effects and synthesized audio. Medium adds detail/shadows; Low reduces detail. Assets and licenses are listed in [asset provenance](../content/ASSET-LICENSES.md).

## Controls and interface

| Control | On foot | Driving |
|---|---|---|
| WASD / arrows | Walk | Accelerate, reverse/brake, steer |
| Shift | Forward sprint | — |
| Space | Jump edge | Handbrake; overrides throttle |
| E | Enter a nearby available car; quick terminal acceptance when no car takes priority | Exit when stopped |
| F | Open nearby job choices | Jobs or repair, where eligible |
| RMB / Q | Hold / toggle aim | — |
| LMB / X | Fire while aiming | — |
| R | Reload | — |
| M | City map | City map |
| Esc | Open menu, neutralize controls | Open menu, neutralize controls |

Play joins a guest; Resume captures the mouse. Drag-look is the fallback when pointer lock is declined. Menus do not pause the world, job deadlines or police search. Blur, hidden tabs, menus and ownership changes clear held/edge input. Twelve bindings can be remapped; duplicate assignments swap, Escape stays reserved, and reset restores defaults.

The HUD separates objective/distance, credits/heat, health, ammunition or vehicle speed/health, and interaction guidance. The minimap and north-up city map identify jobs, garages, repair, nearby cars and the owner's private destination. Map controls support drag/arrows, zoom, whole-city framing and centering on player/destination. Preferences include sensitivity, inverted Y, reduced motion, quality, master/effects volumes and mute. Audio unlocks through Play/Resume gestures and pauses with menus/hidden tabs. There is no music track.

Diagnostics show transport receipts, authoritative/predicted ticks, input lead, RTT, correction/replay history, encoded bandwidth, actor heading/speed, camera distance, entities, graphics/streaming resources and WASM memory. Resync requests a fresh baseline; performance capture produces bounded copyable JSON. See [testing](testing.md) and [performance](performance.md).

## Walking and cars

Walking speed is 4 m/s; forward sprint is 7 m/s. Jumping, gravity, ceilings, sliding and stepping use the shared collision-aware motor. There is no second JavaScript physics implementation. Three car templates—compact, van and sport—use arcade, upright kinematic driving on flat roads. Cars have real collision sweeps, steering/braking/reverse, a single driver and template-specific dimensions/health.

Four ordinary parked cars are authored: two Quay compacts, a Civic van and an Ironworks sport. The second Quay compact is secured. Police and actively AI-driven cars cannot be taken. An ambient car becomes available after parking. Recovery vehicles are private to their job owner.

Entry requires a live on-foot owner, a relevant available car, speed below 0.5 m/s, proximity within 2 m of the driver's entry anchor and a clear approach. Stable server ordering grants one driver. Exit requires speed below 2 m/s and a supported clear left/right/rear candidate. A blocked exit preserves ownership. Enter/exit emits reliable control changes and advances the epoch. Disconnect releases the car and stops it; reconnect starts on foot. Shared motor/query details are in [simulation](simulation.md).

## Jobs and repair

| Activity | Location / prerequisites | Objective | Reward / cost |
|---|---|---|---|
| Courier | Quay cyan terminal **[-172,0,180]** | Random contract: Market express (180 s / 150 CR), Civic dispatch (240 s / 220 CR), or Quay garage drop (120 s / 120 CR); stop inside the selected ring below 2 m/s for 30 continuous ticks | **120–220 credits** |
| Time trial | North ring **[0,0,−256]**, driving | Random coastal circuit or reverse city run; cross ordered gates within 120 s, including a distinct return-to-start finish | **200 credits** |
| Vehicle recovery | Quay or Civic terminal | Collect the owner's reserved East Docks van and deliver it to Quay garage **[−220,0,48]**, within 180 s | **250 credits** |
| Repair | Ironworks green zone **[220,0,−180]**, damaged live driven car, no heat/pursuit, sufficient funds | Remain below 0.5 m/s for 60 observed ticks after requesting repair | **50-credit debit**, full template health on valid application |

F offers the jobs actually available at that terminal. Esc offers job cancellation. Courier may also complete on foot. The trial consumes gate zero at driving acceptance, then advances at most one gate per tick using swept car motion; walking or visiting a later gate cannot skip the order. Its marker shows only the next gate. The courier road route follows the selected destination; other job types show their next target. The contract HUD shows the selected name, payout, countdown and objective progress, highlights the final 30 seconds, and distinguishes confirmed payment from a pending reward.

Recovery randomly selects one of the safe, unreserved secured van slots at East Docks after clearance checks. Only the owner may enter that car; other vehicles do not satisfy the job. The collection target changes to the garage when driving the reserved van. Garage success requires the correct live van within 5 m below 2 m/s for 30 ticks, then a safe actual exit. Blocked doors defer success. The garage consumes/releases the van and reservation after completion. Cancellation, death, arrest, destruction and disconnect release reservations; an occupied blocked exit freezes and retries safely. Timeout and failure take precedence over success.

Only committed rewards appear as spendable credits. Repair eligibility is checked throughout dwell and again against the original session/player/epoch/car after debit commits. Losing eligibility queues an idempotent refund rather than repairing a different car. See [persistence](persistence.md) for transactions, compensation and recovery.

## Ambient city

| Pool | Bound / behavior |
|---|---|
| Traffic | At most **24** cars |
| Pedestrians | At most **32** |
| Parked pool | **24 slots**, including **8 recovery reservations**; four ordinary cars are authored |
| Police | Separate cap of **8**, with two base patrols |

The director runs at 1 Hz. New ambient actors must be 40–160 m from guests, outside every guest's 40 m radius, hidden by static vision blockers and collision-clear. Cars need 12 m separation; pedestrians need 3 m. Unavailable candidates leave slots empty. IDs increase monotonically; visibility generations handle hide/re-entry.

Traffic uses directed routes and shared motors, decisions at 5 Hz, conservative braking and intersection reservations. The ambient FIFO handles at most four path jobs per tick, at most two requests/second per car. Turns slow to 4 m/s; traffic targets at most 12 m/s. Crossing pedestrians and guests cause yielding. Reservation ties use entity order; reservations expire after four seconds. Eligible traffic may park at a free authored spot after thirty seconds; it remains in its traffic pool when available or possessed.

Empty traffic cars can retire after thirty seconds beyond 220 m from all guests and only while hidden. Occupied cars stay pinned. Hidden stuck actors can recycle; visible actors are not teleported. Pedestrians walk at 1.4 m/s, use explicit crossings, wait occasionally and flee on graph for five seconds at 4 m/s after authoritative danger observations. Visual chunk eviction never removes server actors or collision.

## Pistol, damage and recovery

The pistol starts with **12 loaded rounds and 72 reserve rounds**, a **9-tick** fire cooldown and **45-tick** reload. Reloading transfers available reserve rounds only when it completes. Server action IDs prevent duplicate spending/hits. Use 1/2/3 (rebindable) to select owned weapons or scroll the mouse wheel while controlling the game. The HUD shows loaded/reserve ammunition and reload/equip progress. The rifle fires once per press; the machine gun fires while aiming and holding the trigger. Normal sessions equip the pistol and own all three weapons with full ammunition. Automatic fire stops when control is interrupted and requires a fresh press after switching or reloading. Only live, eligible on-foot actors can fire. Entering a car or becoming downed cancels reload. Initial readiness/respawn grants three seconds of protection, ending on firing, car entry or job acceptance.

The client selects the nearest static surface, vehicle box or living character capsule under the centre camera ray, then aims from the local chest toward that point. This corrects shoulder-camera parallax without aim attraction; empty-space aim uses a 120 m camera ray. Fire originates at authoritative feet +1.45 m, follows bounded requested yaw/pitch and tests the chest-to-muzzle path and a weapon-specific ray (**80/120/70 m** for pistol/rifle/machine gun). Static blockers/ground stop it; exact vehicle boxes and NPC/player capsules determine the nearest damage target. Hits apply **25/40/15 damage** respectively. Living, on-foot players other than the shooter absorb shots and take weapon damage. At zero health, players become downed, stop moving/firing, cancel reloads/equipping and active jobs, and respawn safely after 150 ticks with full health and all weapons/ammunition. Joining and respawning grant 90 ticks of protection: players still block shots but take no damage. Firing ends protection early. Control epochs advance on death and respawn so old input cannot regain control. Another guest's occupied car blocks the ray but is protected from player-caused damage. Authoritative Shot/Impact events drive hit feedback and pooled visual/audio effects. Confirmed damage highlights the shooter’s crosshair; health loss shows a brief red screen-edge indicator. Each PvP death broadcasts a system chat elimination message naming both players.

Crash damage is `clamp((speed_loss − 6) × 5, 0, 50)`, once per counterpart per fifteen ticks. Pedestrian impacts above 4 m/s apply `min(100, speed × 8)`, with swept relative-motion queries and the same pair cooldown. Vehicle impacts do not target player capsules; indirect PvP vehicle damage is suppressed.

Incapacitated pedestrians are non-controlling for 300 ticks before leaving the pool. Disabled cars stop and can return after 900 ticks at a clear compatible authored home, retrying when blocked. Driver ejection and respawn use supported, actor-aware clearance rather than teleporting into geometry. A downed driver loses control/job, receives a new epoch and respawns after 150 ticks when space is available, with full health/ammunition and retained credits.

## Police and private heat

Shots can be heard within **80 m** through walls. Secured theft requires a witness within **50 m** and static sight; pedestrian harm requires **60 m** and sight. A damaged police car reports its own hit. Causal IDs and a 64-entry per-player history deduplicate observations. Heat rises at most once per thirty ticks, caps at three, and is private to its owner.

Sight checks run at 5 Hz within **70 m**. Five seconds without sight starts Searching. Units navigate to the last observed position, not the hidden player's current location. Twenty unseen search seconds remove one heat level; fresh sight/crime restarts pursuit/timers.

Patrol home assignments balance Quay and Civic when replacing retired units. Assignments give each wanted player one unit before extras, rotating under contention while retaining the eight-unit cap. Reinforcements must be hidden, collision-clear and at least 40 m from guests. Visible units never recycle; no-path units stop. Police use shared car motors and do not shoot or exit into separate officers.

Arrest requires an assigned live unit's front anchor within **3 m**, static sight and target speed below **0.5 m/s** for **90 continuous ticks**. Arrest safely releases vehicle control, fails an active job, clears heat and advances the epoch. After 150 ticks a safe respawn restores health/ammunition. Credits persist. Blocked exits/spawns defer the transition safely.

## Scope

This is a localhost, single-world game with original procedural art and synthesized effects. Rewind shooting, explosions, on-foot police, distributed worlds and realistic suspension dynamics are outside the implemented rules. Browser compatibility and performance qualifications are listed in [testing](testing.md) and [performance](performance.md).

### Player T-shirt colour

The join menu offers an RGB colour picker. The selected colour is committed with the guest display name before admission and stored in the profile (SQLite migration 002; existing guests default to amber). Player entity descriptors distribute that committed colour reliably in baselines and visibility updates, so local and remote shirts use the same RGB value. While connected, menu changes are saved through `/api/appearance` and broadcast on the next tick after the profile commit. Reliable descriptor refreshes update both the owner and observers without changing control epochs; the world continues running while the local menu is open. Skin palettes remain independent.

### Player chat

Press T to type above the minimap; Enter sends, Shift+Enter adds a line break, and Escape closes the composer. Gameplay input stays released until the canvas is clicked again. Messages use `username: message` with safe `*bold*` / `**bold**`, `_italic_`, `~strike~` / `~~strike~~`, and backtick code formatting. Recent messages fade after 12 seconds; focusing chat reveals the bounded 32-message history. ChatSystem authenticates the author from the player entity, limits sends to one per second, and delivers messages/history reliably to connected players. History lives only for the current server process. Server-generated “Name has joined the game” and “Name has left the game” notices are broadcast to every connected chat window and retained in the same bounded history. Joining is announced after initial loading completes; resyncs and players who never finish loading do not produce extra notices.


## Weapon loadout

Every player joins and respawns with all three weapons: pistol (12 loaded, 72 reserve), rifle (20 loaded, 80 reserve), and machine gun (50 loaded, 150 reserve). Select with 1/2/3 or the mouse wheel. Ammunition remains finite and reloads consume reserves. There are no world supply drops.

Weapon models now attach to local and remote character hands. Aiming uses a collision-aware shoulder camera. Carry, recoil, magazine exchange, equip and stow follow authoritative equipment state. Low quality removes shell casings; reduced motion reduces flashes and suppresses recoil/bobbing/tracers. Firing, reloading and dry clicks have distinct synthesized sounds per weapon.
