# 32 — Presentation and UX

T18 completed on 24 October 2026. Matching browser/server build IDs are `t18-local`; network version 2, content, local WASM bridge and SQLite schema remain unchanged.

## Playing and preferences

Play joins the local authoritative world. Resume captures the mouse; dragging the scene is the fallback if pointer lock is declined. Esc opens the menu and neutralizes controls; the online world continues. The HUD separates objective/distance, credits/heat, health, ammunition or car speed/health, and the current interaction. Ordinary players receive actionable connection errors; raw diagnostics remain expandable.

Default keys are WASD/arrows to move, Shift to sprint, Space to jump/handbrake, E to enter/exit, F for jobs/repair, Q or RMB to aim, LMB or X to fire while aiming, R to reload, and M for the city map. Settings remap twelve actions. Duplicate assignments swap; Escape remains reserved. Reset keys restores defaults. Arrow/alternate-Shift fallbacks apply only while their corresponding default bindings remain unchanged. Menu/map transitions cancel unfinished remaps.

Name, sensitivity, invert Y, reduced motion, Medium/Low quality, master/effects volume, mute and bindings persist locally under `port-meridian.settings.v1`. Invalid values fall back to defaults; unavailable browser storage does not prevent play. Names use 1–24 ASCII letters, digits, spaces, underscore or hyphen and are committed through normal profile admission. These browser preferences are separate from SQLite credits/jobs.

Contextual hints introduce movement, nearby cars and the cyan job terminal; accepting a job dismisses the tutorial. Profiles with completed jobs skip it. Approach a cyan circle within 3 m, press F and choose a job. The objective, gold destination and courier road route guide delivery; authoritative completion commits the reward. Other terminals expose their own available choices. Repair/payment feedback and cancellation remain in the menu.

## World presentation

The original low-poly kit uses warm concrete, terracotta, teal, a fixed sun and generated sky gradient. A common articulated box rig has four clothing palettes and in-place idle/walk/run, jump/fall/land, aim/fire/reload, driving and incapacitated poses. Blend timing is approximately 150 ms. Remote movement and confirmed fire are visible; detailed remote aim/reload is unavailable in the entity contract. Distant actors use cheaper silhouettes, and remote pieces share bounded instanced batches.

Eight deterministic facade patterns add shared windows, trim, roofs and canopies without changing verified collision footprints. The existing six landmark silhouettes remain; authored job/garage/repair zones receive readable signs. Compact/van/sport share procedural tires, rims, spokes and lights. Wheel travel/steering and body lean (maximum four degrees) are cosmetic. The seated pose fits below the authored roofs. Confirmed shots use bounded muzzle/tracer/impact effects; driving handbrake dust and disabled smoke use bounded pools.

The camera eases between walking, driving and aiming FOVs while retaining collision queries. Reduced motion snaps FOV changes and removes extra bob, recoil and squash. Low caps pixel ratio at 1, hides facade detail and dust, simplifies remote characters and disables/disposes the directional shadow map; Medium caps at 1.5 and uses one 2048 directional shadow map. Switching quality invalidates material programs so cached shadows do not remain after selecting Low.

## Maps and sound

The north-up minimap and full map rasterize verified content footprints locally. They show the local heading, authored public locations, current private objective and only nearby, non-stale public vehicles. They do not reveal unseen police globally. M opens the modal map, releases mouse input and pauses local controls/sound. Drag or arrow keys pan; +/- zoom; Whole city, Find me, Destination and named locations focus useful areas. Zoom is bounded 1–8×, focus stays within the dialog, and Escape closes it. No fast travel is added.

WebAudio is created/unlocked only by Play/Resume gestures. Original oscillator/noise effects cover footsteps, engine, skid, doors, gunfire, impacts, reload, siren and job results. A 32-voice priority pool bounds sources; nearby cues spatialize within 70 m, engine pitch follows normalized vehicle speed, and confirmed events trigger combat sounds. There is no music track or recorded media. Master/effects gains and mute affect all sources. Menus, maps, blur and hidden tabs stop/suspend presentation audio without changing server authority. Unmute can resume an already unlocked active context.

## Verification

`npm run check` passes **221 TypeScript and 134 Rust tests**, including strict typing, ESLint/formatting, contract/content/bridge drift, release WASM, Clippy with warnings denied and native gameplay/persistence regressions. Added tests cover validated settings/remap swaps, gesture and mute buses/voice bounds, map transforms, tutorial/error text, animation transitions/seat roof bounds, facade bounds and camera transitions. `npm run build` passes; Vite retains its existing large Three.js chunk advisory.

Live production-browser checks used a separate `localhost:4173` guest with the actual SQLite-backed WS/RTC server, ordinary keyboard/menu controls and no live actor/economic/DB injection:

- A fresh Harbour Test profile walked to Quay, chose courier, followed the map route and completed delivery for **150 credits / one completed job**. Rejoining retained that balance. [Delivery wallet](evidence/t18-first-delivery.png).
- The full map showed the gold objective/road route; focus/zoom and 640×720 layout worked. [City map](evidence/t18-city-map.png), [narrow map](evidence/t18-map-narrow.png).
- Entered a compact, accelerated, steered both ways, used handbrake/reverse, stopped and exited. A collision reduced vehicle health through the existing authoritative system. Final visual inspection confirmed the seated head stays below the roof; native bounds tests also cover van/sport. [Seated driver](evidence/t18-driving.png).
- Selected Low, reduced motion and mute; remapped M to N, verified M no longer opened the map and N did, then reloaded and confirmed preferences persisted. Narrow gameplay remained readable. Defaults were restored for keys after the test. [Low/narrow HUD](evidence/t18-low-narrow.png). Temporary viewport overrides were reset.
- A Medium local sample recorded **60 FPS / 16.7 ms, 152 draw calls, 34,418 triangles, 19 resident chunks, zero pending/fallback chunks, 983 KiB instance matrices and 0.000 m correction p95**. This is a functional scene sample, not a 16-peer capacity result.

Audio lifecycle/gesture/mute behavior was checked through unit tests and visible settings state; a subjective audible mix assessment was not recorded. The art is an original procedural low-poly kit rather than imported production models. Detailed remote weapon animation remains excluded. Full real-RTC impairment, 16-player capacity and soak measurements remain T19/T20; Chrome itself was not controlled in this verification.

## Quick review

Refresh the browser after updating both builds. Play, walk to the cyan Quay circle and press F. Complete courier using the HUD and M map. Enter a parked compact from its left door with E, drive and stop before exiting. Open Esc → Settings & controls, try Low/reduced motion/mute and remap M; refresh to check persistence. Resize the window, open the map and try keyboard pan/zoom. Unlock audio with Play/Resume before listening, then verify mute and menu/hidden-tab silence on your device.
