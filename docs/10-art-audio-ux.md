# 10 — Art, audio, and UX

[Index](../README.md)

## Visual direction

Stylized coastal city with warm concrete, terracotta brick, teal water, blue industrial steel, and orange accents. Use simple silhouettes, readable contrast, slightly exaggerated cars, and clear daylight. The goal is a consistent small world, not photorealism.

Set one fixed late-afternoon sun and a light sky gradient. No day/night simulation or weather in alpha. Roads are neutral charcoal, sidewalks warm grey, navigational markers mint/amber, police red/blue. Dangerous/available states also use icons and text so color is not the only signal.

Build the whole slice with primitives first. Replace one asset category at a time while preserving anchors and collision. Do not spend the first implementation tasks collecting decorative models.

## Art inventory

| Category | Slice | Complete alpha |
|---|---|---|
| Character | One capsule/placeholder | One common rig, 4 clothing palettes |
| Cars | One compact greybox | Compact, van, sport; police variant reuses compact |
| Buildings | 4 box footprints | 8 modular facade sets, 6 landmark silhouettes |
| Roads | Straight, intersection, curb/decal | Add T-junction, parking, lane markings, ring corners |
| Props | Terminal, delivery marker | Fence, container, bench, lamp, repair sign |
| Effects | Dust/tracer placeholder | Muzzle flash, impact puff, tire dust, disabled-car smoke |
| UI | Plain readable DOM | Coherent panels/icons with settings and map |

Do not use GTA logos, maps, characters, audio, car branding, or extracted assets. Every imported file gets a source URL, license, author, modification notes, and attribution requirements in `content/licenses/ASSETS.md`. Prefer a small consistent original kit over many incompatible assets.

## Asset and render budgets

These are starting budgets to measure:

| Item | Target |
|---|---:|
| Character LOD0 / LOD1 | ≤ 8k / 2k triangles |
| Car LOD0 / LOD1 | ≤ 8k / 2k triangles |
| Typical building | ≤ 2k triangles |
| Visible total | ≤ 500k triangles, ≤ 200 draw calls on medium |
| Visible skinned characters | ≤ 24; use simple distant impostor/LOD beyond that |
| Textures | 512–1024 px atlases; a few 2048 px landmark atlases |
| Estimated resident GPU resources | ≤ 256 MiB medium target |
| First-play compressed download | ≤ 12 MiB including JS/WASM/spawn assets |
| Full map compressed assets | ≤ 60 MiB fetched progressively |
| Pixel ratio | cap 1.5 medium; 1.0 low |

Use shared materials and instancing for repeated props/buildings where practical. Keep collision meshes independent of render detail. Measure draw calls through renderer diagnostics. No per-entity shadow maps.

Shadow setup: one directional map around the camera, 1024 on low or disabled, 2048 medium. Ambient light fills shadowed streets. Alpha does not require SSAO, SSR, depth of field, volumetric fog, or bloom. A subtle distance fog can hide chunk LOD transitions.

Three.js WebGLRenderer requires WebGL2; feature-detect and explain unsupported hardware at boot. See [sources](15-decisions-and-sources.md).

## Animation contract

Character clips: idle, walk, run, jump_start, fall, land, aim_idle, aim_walk, fire, reload, driving, incapacitated. Use in-place animation; server motion is never driven by root animation.

`AnimationStateMachine` reads rendered velocity, grounded state, mode, local aiming, reload state, and confirmed cosmetic events. Blend idle/walk/run by horizontal speed over ~150 ms. Jump and landing are edge transitions derived from sampled grounded changes; packet reordering must not repeatedly restart the clip.

Remote aim/reload detail is intentionally minimal because the v1 entity state does not replicate aim pitch or weapon animation state. Remote actors may show shot events and generic movement/idle; richer aiming requires an explicit protocol addition later. Do not infer detailed remote weapon state from private PlayerStatus.

Cars: wheels rotate from distance traveled, front wheels steer from motor steering, body lean is cosmetic and clamped ±4°. Handbrake dust is cosmetic. No visual transform may feed back into collision state.

Fallbacks: missing clip → nearest idle/walk clip, missing texture → palette material, missing distant model → simple silhouette. Missing player/car model must still preserve recognizable dimensions.

## HUD layout

```text
┌─────────────────────────────────────────────────────────┐
│ Job title / next step                       Credits 350 │
│ Time remaining / distance                   Heat ●●○    │
│                                                         │
│                      aim reticle                        │
│                                                         │
│               [E] Enter compact                         │
│                                                         │
│ Minimap      Health ▰▰▰▰▰          72 km/h or Ammo 8/12  │
│              connection warning only when needed        │
└─────────────────────────────────────────────────────────┘
```

Use DOM/CSS for HUD, menus, settings, error screens, and map panels. Canvas/Three.js is for world rendering. HUD values come from a read-only model and update at ≤ 10 Hz unless a visible transition requires immediate feedback.

Minimap: bake an SVG/PNG overview from road/building footprints; project X/Z to map coordinates with north up. Player rotates on map; map remains fixed. Show own location, current job, and nearby public vehicles; do not reveal unseen police positions globally. Full map supports pan/zoom locally, legend, and active destination. No fast travel.

World prompts show server-relevant limits: “Slow down to exit,” “Vehicle occupied,” “Exit blocked,” “[F] Repair — 50 credits,” “Connecting…”. Do not show implementation jargon or raw protocol errors to normal players. Debug mode may show tick/RTT/bytes/correction.

## Audio

Browser audio starts only after the Play gesture. One master bus with music/effects sliders; music can remain absent rather than ship unlicensed content.

Required effects: footsteps, compact engine loop, tire/skid cue, door entry/exit cue, pistol, impact, police siren, job success/failure. Use original or licensed short assets. Engine pitch maps to normalized speed; it does not attempt full gear simulation.

Spatialize nearby effects with a maximum audible distance and pooled sources. Cap 32 simultaneous voices, prioritize local feedback and nearby police, and fade culled loops. Avoid a sound per networking packet. Muting and hidden-tab pause affect audio presentation, never authority.

## Accessibility and first-run flow

- Readable text at 1280 × 720 and 1920 × 1080; minimum 16 CSS px for main HUD.
- Keyboard-accessible menu/settings, visible focus, text labels for icon buttons.
- Remappable controls, mouse inversion/sensitivity, reduced motion, independent volume, quality preset.
- Pause overlay releases mouse and neutralizes controls; say the online world continues.
- Three short contextual hints: move/look, enter car, reach job terminal. Dismiss after successful action; no long forced tutorial.
- Do not require flashing police lights to understand pursuit; heat/status text remains readable.

## Presentation acceptance

From a new browser profile, join and complete the first job using only in-game prompts. Record at least one driving loop and inspect for clipping, missing chunk seams, unreadable markers, invalid animation transitions, and repeating audio. Repeat on low quality, muted audio, reduced motion, and resized window.
