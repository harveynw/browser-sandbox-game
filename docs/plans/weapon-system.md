# Weapon system implementation specification

Status: Stages 1–4 implemented locally; Stage 5 pending. Written 2026-11-02.

Audience: an implementation model working in this repository. Implement the stages in order. This document specifies intended behavior; names marked “proposed” are not existing APIs. Inspect the current checkout before editing and preserve unrelated work. Do not deploy or push merely because this specification describes deployment checks; obtain that instruction from the user.

## Outcome and scope

Add a pistol, precision rifle and automatic machine gun; a three-slot inventory with finite ammunition; shared world pickups; and readable held models, animation, audio and feedback. Players should explore different neighbourhoods to find equipment. Keep server-authoritative hits and the existing rule that players and other guests' occupied cars cannot receive player-caused damage.

Deliver functional gameplay incrementally. Do not introduce PvP, explosives, scopes, attachments, penetration, player weapon drops, headshot multipliers, inventory persistence, new currency purchases or interiors in this iteration. Trees remain decorative and do not become ballistic blockers through this work.

## Current implementation and integration map

Verify these observations against the checkout:

- `crates/server/src/entities/weapon.rs`: a single pistol magazine, fire/reload deadlines, and respawn/protection state currently share one `Weapon` struct.
- `crates/server/src/systems/combat.rs`: pistol capacity 12, cooldown 9 ticks, reload 45 ticks, range 80m and damage 25 are hard-coded. Reload currently replenishes the magazine without reserve ammunition. Respawn replaces weapon state.
- `crates/server/src/systems/police/mod.rs` and `missions/mod.rs`: also touch weapon timers/protection. Audit every reference when separating lifecycle state.
- `crates/protocol/src/types.rs`: reliable `ActionRequest` carries request ID and control epoch; actions include `Fire` and `Reload`. `PlayerStatus` carries private ammunition/reload state. `Shot` lacks weapon identity.
- `apps/client/src/net/session.ts`: generic action submission has a shared 250ms throttle. Do not route every automatic bullet through this path or simply remove its limits.
- `apps/client/src/input/input.ts`, `game/app.ts`, `game/settings.ts`: fire is currently edge-triggered; key bindings and settings need migration-safe defaults.
- `apps/client/src/render/renderer.ts`: the pistol is a small camera-relative model; pooled shot/impact effects already exist. `render/character.ts` owns procedural character poses.
- `apps/client/src/audio/sound.ts`: existing synthesized effects can be extended without external assets.
- `crates/content/src/{types,validate,bake}.rs`, `content/source/city.json`: authored content, placement reservations and access references. `packages/content/src/loader.ts` validates package identity and asset hashes.
- `apps/client/src/content/inspector.ts`, `tooling.ts`, `tooling.css`: standalone tools use 98.css; gameplay retains its own styling.

Generated TS, schemas and fixtures must be regenerated from Rust. Generated content package directories are ignored by Git; ship the currently referenced city and slice packages explicitly in deployment artifacts.

## Initial tuning contract

The server runs at 30 ticks/second. Store gameplay durations in integer ticks. These are starting balance values, not established performance results.

| Parameter | Pistol | Rifle | Machine gun |
|---|---:|---:|---:|
| Stable ID | `pistol` | `rifle` | `machine_gun` |
| Fire mode | single press | single press | held trigger |
| Magazine capacity | 12 | 20 | 50 |
| Maximum reserve rounds | 72 | 80 | 150 |
| Fire interval, ticks | 9 | 12 | 3 |
| Reload duration, ticks | 45 | 60 | 90 |
| Equip duration, ticks | 9 | 12 | 15 |
| Damage per accepted hit | 25 | 40 | 15 |
| Maximum range, metres | 80 | 120 | 70 |
| Stationary spread half-angle, degrees | 0.6 | 0.2 | 1.2 |
| Moving spread multiplier | 1.6 | 2.5 | 1.8 |
| Bloom added per shot, degrees | 0.1 | 0.05 | 0.25 |
| Maximum additional bloom, degrees | 0.6 | 0.3 | 3.0 |
| Bloom recovery, degrees/second | 2.0 | 1.5 | 2.0 |

Movement means horizontal speed above 0.5m/s. Spread uses a deterministic, server-owned shot sequence and seed; clients cannot choose the result. Apply bloom after calculating the current shot. Recover bloom per server tick. No damage falloff in this iteration. Keep aim-required firing and current on-foot eligibility.

One canonical weapon catalogue must supply authoritative gameplay values and published client display values. Prefer a dedicated shared Rust module, exported through the existing generated contract/content pipeline. Include gameplay tuning in the content compatibility identity; do not duplicate tables in TS.

## Inventory and lifecycle rules

- Exactly three ownership slots; one equipped weapon. Loaded ammunition and reserves are separate per slot. No shared ammunition pool.
- Starter loadout: pistol with 12 loaded and 36 reserve; other slots unowned.
- Selecting an unowned weapon fails without changing state. Selecting the already equipped weapon is a no-op.
- Switching cancels reload with no ammo transfer, stops automatic fire and begins the new weapon's equip delay. Equip requires a fresh trigger press afterward. Switching cannot reset an existing fire deadline to permit extra shots; retain cooldown per weapon.
- Reload is unavailable when full, already reloading, equipping or out of reserves. Transfer `min(capacity - loaded, reserve)` only at completion. Cancellation preserves both counts.
- Empty trigger: bounded dry-fire feedback, no shot event or damage. Do not auto-switch or auto-reload.
- Entering a car, incapacitation, arrest, loss of control, disconnect and epoch change stop firing and cancel reload/equip. Returning on foot requires a fresh trigger press.
- Death or arrest respawn restores the starter loadout. A new connection also starts with it; reconnect is not an inventory persistence mechanism. Same-session resync preserves authoritative ownership/ammo but clears held-trigger intent.
- Pickups never extend spawn protection. Existing actions that end protection must continue doing so.

## Stage 1 — Authoritative weapon state and inventory

### Work

1. Introduce a bounded weapon ID enum and canonical definitions. Extract respawn/protection into a player lifecycle component before adding inventory complexity.
2. Replace the single magazine with fixed-size weapon slots and equipped/reload/equip state. Use absolute server deadlines and explicit cancellation helpers.
3. Parameterize the existing hit query and damage path. Preserve static/ground blockers, exact vehicle tests, pedestrian capsules and protected targets. Keep a single authoritative query per accepted shot.
4. Extend private status with owned slots, per-slot loaded/reserve counts, equipped ID and reload/equip deadlines. Extend public character state with equipped weapon and coarse aim/reload/equip presentation state; never broadcast private reserves unnecessarily.
5. Add reliable equip actions, using existing request deduplication and control epochs. Include intended weapon identity in fire requests so a delayed action cannot fire a newly selected weapon accidentally.
6. Give `Shot` weapon identity and an authoritative per-shooter shot sequence. Treat schema/protocol compatibility changes explicitly; reject incompatible clients cleanly.

### Acceptance

Unit/integration cases cover partial reloads, zero reserves, cancellation on every lifecycle transition, cooldown preservation while switching, unowned selections, duplicate actions and stale epochs. Existing ray occlusion and protected-target tests pass for all three definitions. Inventory remains bounded and counts never underflow or exceed capacity. Baseline/resync reconstructs correct state.

Stage 1 can use fixtures to grant weapons for tests. Do not add a public debug-grant endpoint. Exit with a playable pistol and server-tested inventory; later stages expose acquisition.

## Stage 2 — Controls, automatic fire and gameplay HUD

### Work

1. Add rebindable slot selection (1/2/3) and wheel cycling through owned slots only. Wheel cycles only during active gameplay, never while a menu owns input.
2. Preserve single-click pistol/rifle behavior. Holding either must not repeat. Add held-trigger state for the machine gun.
3. Proposed automatic-fire transport: reliable start/stop/refresh intent messages, with a monotonic trigger sequence and current control epoch. Refresh at most 10Hz while held; expire intent after 15 server ticks without a fresh accepted refresh. Validate aim and sequence, use bounded message budgets, and never refresh a lease from duplicates or out-of-order messages.
4. The server emits shots on its tick schedule, at most one shot per player per tick. Late packets cannot request historical catch-up bullets. Stop messages clear intent immediately; newer stopped state must not be resurrected by older starts. Aim uses the newest accepted aim sample.
5. Clear local held state on mouse release, focus loss, hidden tab, pointer-lock loss, menu/map opening, death, vehicle entry and disconnect. Send stop when possible; lease expiry is the fallback. Test the existing action limiter so equip/reload actions remain responsive while firing.
6. Display weapon name, slot, magazine/reserve, reload progress and equip state. Crosshair reflects spread from shared tuning and reconciled state. Damage/hit confirmation remains server-driven.

The implementer may integrate intent into the existing input stream instead if ordering, expiry, release semantics and budgets are demonstrably equivalent; record the reason in the handoff.

### Acceptance

Simulate held fire, rapid click spam, focus loss, missing release, stale/replayed messages, jitter and reconnect. Machine gun fires exactly at its configured cadence; no ammunition or damage is created client-side. Trigger expires within 15 ticks when refresh stops. An empty weapon does not generate an effect flood. Selection, aiming and reload work with keyboard and mouse without interfering with driving or chat.

## Stage 3 — Authored pickups and world availability

### Content design

Create six permanent supply stands with these intended locations. Select actual coordinates by inspecting the current map; the table is not a claim that a suitable shed/workbench already exists.

| Stable pickup ID | District and landmark treatment | Weapon | Respawn |
|---|---|---|---:|
| `quay_sidearm` | Open garage workbench near the starting district | pistol | 45s |
| `market_sidearm` | Shop service courtyard | pistol | 45s |
| `heights_rifle` | Small groundskeeper stand beside a grove | rifle | 90s |
| `civic_rifle` | Maintenance compound with street access | rifle | 90s |
| `ironworks_machine_gun` | Workshop loading yard | machine gun | 150s |
| `docks_machine_gun` | Container-yard supply crate | machine gun | 150s |

Add three ammunition-only stands: pistol supplies in Market Grid, rifle supplies in North Heights, machine-gun supplies in East Docks. Place each in a different block from its corresponding weapon stand when feasible. Ammo-only cooldowns: 60/90/120 seconds respectively.

### Rules and implementation

- Proposed authored pickup definition: stable ID, position/yaw, pickup kind, weapon/ammo ID, grant quantities, respawn ticks, reservation ID and pedestrian access ID. Bake stands into visuals and necessary collision; keep the interaction approach clear of buildings, trees and traffic.
- First weapon acquisition grants a full magazine plus two magazines of reserve, clamped to caps. Duplicate weapon pickup adds one magazine to reserve. Ammo-only stands add two magazines to reserve and require ownership. Ammo pickups do not directly refill a loaded magazine or cancel a reload.
- If a claim grants nothing, leave the stand available. Partial grants consume the stand. Never auto-equip a newly acquired weapon; show a notification with its slot key.
- Claim requires a living, eligible on-foot actor within 2.5m in 3D and an unobstructed interaction line. Server derives distance/state, never trusts client quantities or timers.
- Shared availability: atomically resolve claims in the simulation loop; one winner per availability revision. A request includes pickup ID and observed revision, so a delayed old claim cannot consume a later respawn. Existing request IDs handle retries.
- Respawn uses absolute server ticks. Static stands persist visually; an unavailable stand hides its item and shows a nearby countdown based on server time.
- Reliable availability updates plus a complete late-join/resync baseline. Use bounded state for the small authored set, rather than per-frame replication of rotating pickup meshes. Validate missing IDs and invalid definitions at bake/startup.
- On-foot F selects the nearest eligible context target across pickups, terminals and repair interactions. Use the same target selection for prompt and request; stable ID breaks distance ties. Preserve E for car entry/exit. Avoid overlapping authored interaction radii where practical.
- Mark weapon stands on the large map with weapon-specific symbols. Mark ammo stands when nearby. Do not expose decorative pickup meshes as damageable gameplay actors.

### Acceptance

Two simultaneous claimants produce exactly one grant and one cooldown. Full inventories do not waste stands. Test out-of-range, through-wall, driving, dead, duplicate and stale-revision claims. A new client sees correct depleted stands and countdowns. Validate that every stand has a clear approach from the pedestrian graph and does not obstruct existing missions, parking or spawns. Keep the slice regression world supported, with an empty/default pickup list unless a test explicitly adds pickups.

## Stage 4 — Models, poses, effects and sound

### Work

- Build original low-poly procedural models for pistol, rifle and machine gun. Share model factories between held weapons, pickup previews and tooling. Share geometries/materials and dispose them correctly.
- Define grip, support-hand and muzzle attachment transforms per weapon. Attach to character hands; remove the existing camera-floating pistol. Local aiming should use a shoulder framing that keeps the weapon visible without clipping the camera. Remote actors must show the same equipped weapon and corresponding pose.
- Procedural states: lowered carry, raise/aim, fire recoil, reload, equip and stow. Pistol reload has a magazine exchange and slide gesture; long guns have a magazine exchange and return to the support grip. Use normalized server deadlines so late state updates do not restart animations.
- The authoritative ray remains the source of damage. Cosmetic muzzle position must not permit firing around walls or redirect the hit result. Near-wall visual corrections must remain cosmetic and agree with the server's obstruction decision.
- Brief weapon-specific flashes, pooled shell casings with short cosmetic lifetimes, occasional tracers, surface-appropriate impact particles and distinct synthesized firing/reload/dry-fire sounds. Add bounded distance attenuation and avoid one new light or allocation per shot.
- Optional immediate local recoil/flash must reconcile by shot sequence and never double-play the confirmed event. Remote effects use authoritative events. No predicted hit markers.
- Reduced-motion mode removes camera kick and reduces flashes. Low quality omits casings and extra particles while retaining clear firing/reload feedback.

### Acceptance

Review all weapons from local aim, third-person and remote-player viewpoints. Weapon silhouette is identifiable without reading a label. Hands meet grips; weapons stow in vehicles; interrupted reload/equip poses recover cleanly. Bound effect pool sizes and queues; overflow discards cosmetic effects without dropping authoritative state or disconnecting players. Test a synthetic 16-player firing load (up to 160 accepted machine-gun shots/second), measure server tick time, network egress, draw calls and client frame time, and report results rather than claiming an unmeasured performance target.

## Stage 5 — Weapon workbench, integration and handoff

Create a standalone `/weapons` preview in the existing 98.css tooling shell and navigation. Include weapon selection, orbit/zoom, held/pickup views, aim/fire/reload/equip preview buttons, animation pause, reduced-motion/quality toggles, and read-only catalogue values. Reuse production model/pose/effect code; changing preview state must not change world tuning or grant live inventory. Ensure all preview resources are cleaned up on navigation.

Complete the gameplay documentation and control help. Add weapon/pickup explanations to the existing map inspector where useful. Ensure the main game keeps its existing visual design.

### Verification commands

Use the supported Node 24 toolchain. Run focused tests after each stage, then final integration checks:

```sh
npm run protocol:generate
npm run content:bake
npm run build
npm run check
```

Run the repository's local connection probe and the multiplayer scenarios described above. `npm run check` includes strict clippy/format checks; distinguish any pre-existing warnings from regressions, fix new failures, and do not report the whole check as passing if a step fails. Preserve unrelated work when formatting.

### Final acceptance checklist

- [ ] All three weapons can be acquired, selected, fired and reloaded through normal gameplay.
- [ ] Inventory, ammo, cadence, damage and pickup availability are authoritative and bounded.
- [ ] Shared pickup races, lifecycle cancellation and automatic-fire timeout are tested.
- [ ] Main game, map inspector and workbench agree on weapon identities and models.
- [ ] Remote players show equipment and animation correctly; late join/resync works.
- [ ] Existing missions, cars, police, persistence and protected-target rules still work.
- [ ] Shared content/protocol contracts are regenerated; package indexes reference complete local artifacts.
- [ ] Screenshots and measured performance evidence are recorded with the actual hardware/settings.
- [ ] Any remaining limitations are explicitly listed.

## Instructions for each stage handoff

Append a short implementation record below, including stage status, commit if any, files/interfaces changed, tests actually run and outcomes, known issues, and the exact next task. Update this spec if a design choice changes; distinguish deliberate changes from incomplete work. Do not leave temporary grants or tuning controls in public gameplay.

For an authorized deployment, build matching frontend/WASM/content, include both active generated packages despite `.gitignore`, back up the remote bundle and saves, stop the game before replacing the served package, build the Linux binary on the host, restart, and verify public HTTPS plus real WSS/RTC receipts. Preserve a rollback bundle. Local success alone is not a public transport acceptance result.

## Implementation record

### Stage 1 — implemented locally (2026-11-02)

- Canonical catalogue lives in `crates/protocol/src/types.rs` and is included in both published world manifests. Runtime loading checks the compiled catalogue matches the package. Protocol is now version 4; deploy client, server and generated content together.
- Fixed three-slot inventory, per-slot magazine/reserve/cooldown/bloom, equip/reload deadlines and shot sequences live in `entities/weapon.rs`. Player respawn/protection lives separately in `Lifecycle`.
- Reload consumes reserves at completion; cancellation preserves rounds. Equip requires ownership, cancels reload, and retains each slot's fire deadline. Death/arrest restores the pistol loadout; same-session resync preserves inventory. Public equipment is projected from inventory at replication time, avoiding stale copies.
- Combat uses the selected definition for range, cadence and damage, with server-seeded spread and bloom. Requests identify the intended weapon; mismatches and equip-window shots are rejected. Shot events identify weapon and sequence. Existing protected-target rules remain.
- Private status carries all three slots. Public character state carries optional equipment (absent for NPCs), including equip/reload deadlines. Aim presentation and held-trigger state are Stage 2 work.
- Client networking sends the equipped identity and exposes an equip action. Gameplay still starts with the pistol; other slots are granted only inside tests. No public grants, pickups, selection keys or automatic-fire loop have been added.
- Validation: Rust workspace tests, focused duplicate/stale/equip/resync integration tests, all-weapon protected-target coverage, ammo/cancellation unit tests, 136 frontend tests, TypeScript, ESLint, generated protocol/content checks, production/WASM and native builds. Local complete-baseline/StartPlay/WS/RTC probe passed. Clippy was run without `-D warnings`; pre-existing warnings remain, so full `npm run check` is not claimed as passing.
- Not committed, pushed or deployed by this stage. The existing remote remains on its previously deployed protocol.
- Next task: Stage 2 controls/HUD and authoritative held-trigger leases. Use `Session.equipWeapon`, private inventory status, `Player.public_state`, and `Weapon.definition`/per-slot timers. Then implement Stage 3 acquisition; finite starter ammunition currently has no world replenishment.


### Stage 2 — implemented locally (2026-11-02)

- Added rebindable 1/2/3 slot selection and focused-canvas wheel cycling through owned slots. Old saved bindings migrate while preserving custom keys, including conflicts with new defaults.
- Pistol/rifle remain edge-triggered. Machine-gun intent uses a separate reliable `Trigger` message with epoch, monotonic sequence, held/pressed state and bounded aim. Refresh is 10Hz; server acceptance is limited to once per three ticks, with a separate 12/s, 24-burst message budget. Leases expire after 15 ticks; duplicates do not extend them, old starts cannot override newer stops, and refresh alone cannot resurrect expired/cancelled firing.
- The server schedules automatic shots in its tick loop, reusing authoritative hit queries, per-weapon cooldowns and ammo. Direct machine-gun Fire actions are rejected. Reload/equip, ineligible actors, resync and control epoch changes stop intent. There is no historical bullet catch-up.
- Menus, focus loss, aim release, vehicle transitions and interruptions clear local firing. Switching/reloading requires releasing the physical trigger before firing again. Trigger messages do not consume the shared client action throttle, and semi-auto fire has its own cadence limiter so reload/equip remains responsive.
- HUD shows weapon, magazine/reserve, owned slots, capacity-correct ammo bar and equip/reload percentages. Crosshair size uses shared tuning and per-weapon bloom estimated from accepted shot events; authoritative hit markers remain unchanged. This is cosmetic estimation rather than additional per-tick spread replication.
- Tests cover server cadence/timeout/release, duplicate and stale sequence handling, epoch/reload/equip cancellation, held input and active-only selection, client refresh throttling and reload responsiveness, and settings migration. Frontend suite: 140 tests. Rust workspace tests and focused automatic-fire tests pass; TypeScript, ESLint and production build pass. Existing build warnings remain.
- No normal-play grants or debug endpoints were added. Rifle/machine-gun acquisition remains Stage 3; server fixtures grant those weapons for tests. No push or remote deployment was performed.
- Next: Stage 3 authored supply stands, atomic pickup claims, shared respawn state, interaction priority and map markers. Use `Weapon.slots`, catalogue reserve caps, `GameSession.selectWeapon/cycleWeapon/updateTrigger`, and `weaponSummary` rather than introducing parallel inventory logic.

### Stage 3 — implemented locally (2026-11-02)

- Authored six weapon stands and three ammunition stands in `content/source/city.json`, each with a clear reservation and an access spur. North Heights rifle was moved west of the park pavilion after the baker correctly rejected the first obstructed approach. All current road and pedestrian graphs validate.
- `SupplyPickup` definitions are baked into the manifest; missing authoring pickups default to an empty list for the slice. IDs, grant bounds, cooldowns, reservation containment and references are validated. The runtime caps stands at 32.
- `systems/pickups.rs` atomically validates revision, availability, on-foot/alive state, 3D range and static line of sight before granting ammunition. Session action handling also enforces control epoch, request deduplication and arrest restrictions. First ownership grants a full magazine plus two reserve magazines; duplicate weapons grant one reserve magazine; ammo stands grant two. Full/invalid requests leave stands available; partial grants consume them. Pickup never changes equipped weapon or protection.
- World pickup state carries availability revision and absolute respawn tick. Successful claims dirty every session's private status; expiry increments revision and republishes. The complete small pickup list rides in `PlayerStatus`, including baseline/resync, rather than introducing a separate baseline stream. Gameplay grants and reliable status remain bounded.
- F chooses an eligible nearby supply using the same target logic as the prompt. Terminal distance wins if closer, with stable IDs for ties; driving repair keeps its existing behavior. E now remains for cars rather than claiming supplies or quick-accepting a job. Depleted stands show a nearby countdown; new ownership shows its selection slot.
- The renderer shows simple weapon/ammo previews above decorative stands, hides depleted items and uses bounded bob/rotation (disabled by reduced motion). Stands are visual-only so they do not block reserved graph access. They are placeholders: Stage 4 should replace these with the shared weapon model factories and revisit physical stand placement/collision together with navigation clearance, rather than adding blockers to existing access endpoints.
- Large-map weapon markers use P/R/MG; ammunition A markers appear within 80m. Stand locations are also labeled in the standalone map inspector.
- Validation: Rust workspace tests (including 89 server tests and 16 content tests), 141 frontend tests, TypeScript, ESLint, generated protocol/content checks, frontend/WASM and native builds. Focused cases cover simultaneous claims, duplicate requests, stale revisions, cooldown expiry, resync, full reserves, partial grants, unowned ammo, 3D range, driving/dead actors and wall-blocked requests. Client test verifies prompt/claim identity and countdown behavior. Local baseline/StartPlay/WS/RTC probe passes. Existing Clippy/build warnings remain; full strict `npm run check` is not claimed.
- No push or remote deployment performed. Next: Stage 4 low-poly models shared by held weapons and pickups, hand attachments, procedural animation, weapon-specific effects/audio and reduced-motion/performance checks. The current camera-held model still uses the existing pistol presentation until that stage.

### Stage 4 — implemented locally (2026-11-02)

- `render/weapons.ts` owns original pistol/rifle/machine-gun model factories with shared geometry/materials, independent magazine/slide transforms and grip-relative support/muzzle attachments. Held and pickup models use the same factory; the factory is reusable by Stage 5 tooling.
- Removed the camera-floating pistol. Local and remote character rigs pose articulated arm segments to weapon grips, with lowered carry, smoothed raise, accepted-shot recoil, deadline-normalized reload/equip, magazine movement, pistol slide gesture and support-grip return. Vehicles/incapacitation stow equipment. Deadline cancellation restores the current pose rather than restarting a local timer.
- Public equipment includes aiming and pitch from accepted movement input. Generated contracts/fixtures were refreshed within unreleased protocol 4; client/server must still be deployed together. NPCs remain unarmed.
- Shoulder camera uses collision shortening and frames the chest aim ray, including nearby static obstructions. Near-wall weapon lowering and muzzle correction are cosmetic. Damage/hit markers remain authoritative; no immediate predicted effects or camera kick were introduced.
- `render/weapon-effects.ts` owns preallocated flash/tracer/casing/particle pools, with no per-shot lights or geometry. Confirmed shots vary flash size by weapon and show occasional tracers; impact colors distinguish ground, static obstacles, vehicles and actors. Car impact feedback remains supported. Low quality omits casings/reduces particles; reduced motion removes rig recoil/bobbing/tracers and reduces flash duration/size.
- Synthesized firing, reload and dry-fire sounds vary by weapon on the existing 32-voice, distance-attenuated bus. Reload cues support remote actors and dry-fire is throttled to two cues per second.
- Validation: 146 frontend tests, Rust workspace tests, TypeScript, ESLint, generated checks and full build pass; local complete-baseline/StartPlay/WS/RTC probe passes after server restart. Existing repository formatting/Clippy failures prevent claiming full `npm run check` success. Detailed hardware, load measurements, screenshots and limitations are in `docs/verification/stage4/results.md`.
- Synthetic 16-player server load accepted 160 shots/s: tick p95 4.496 ms, aggregate JSON egress 1.31 MB/s (no transport overhead). Isolated browser fixture reached approximately 160 shots/s with bounded pools and maximum 52 draw calls. These are measured fixture results, not full-city/network capacity guarantees.
- Supply stands deliberately remain visual-only; no navigation endpoints were blocked. No push or remote deployment. Local game is ready at http://localhost:4173. Next: Stage 5 standalone 98.css `/weapons` workbench using these production factories/poses/effects, followed by final integration and playtesting.

### Loadout revision — supply drops removed (2026-11-02)

User direction supersedes the Stage 3 acquisition design: every new player receives all three weapons with full magazines and reserves (12/72 pistol, 20/80 rifle, 50/150 machine gun), pistol equipped. Respawn restores this same loadout. Ammo is finite and reload still consumes reserves.

Removed authored stands and dedicated access/reservation spurs; pickup content/protocol types; server claim/availability/tick/status code; pickup meshes, prompts and client actions; map/inspector markers; and pickup-only tests. Regenerated content, schemas and fixtures. Stage 4 models, poses, effects and sounds remain reusable for held weapons and the planned workbench.
