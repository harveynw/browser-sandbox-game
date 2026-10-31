# Neighbourhood architecture experiment

## Plan

1. Compare two contrasting districts: Old Quay (shop houses / tenements) and Ironworks (depots / factories). Keep the remaining four districts as a control with their existing facade patterns.
2. Add authored district recipes for width, depth, floor count and floor height. Bake deterministic dimensions into the same shell used by collision, streaming, minimap and backdrop.
3. Apply a small split grammar on the client: shell → floors → bays → windows, with a street-facing entrance and style-specific rooftop / frontage parts. Use existing instanced cubes and a bounded material palette.
4. Verify deterministic content, safe plot envelopes, road and mission clearance, facade bounds, street-facing entrances, type checking, tests and production build. Inspect both districts in the browser.
5. Review the visual result before expanding the vocabulary to Civic Center, North Heights, Market Grid and East Docks.

## First implementation

District `architecture` in `content/source/city.json` owns style and dimension ranges. Width/depth use half-metre steps and fit within the existing 20m plot envelopes. Floor count is integral; height is count × floor height. Rust validation rejects oversized footprints, invalid ranges and heights outside world bounds. Coordinate-derived entropy makes dimensions stable when other plots are removed by reservations.

Old Quay: 10–16m width, 12–20m depth, 2–4 floors at 3.2m; warm masonry, separate window bays, floor bands, shop or residential frontage, awnings, parapets and chimneys.

Ironworks: 16–20m width/depth, 1–2 levels at 5m; muted industrial finishes, ribs, clerestory windows, loading shutters, service doors, canopy, vents and optional paired stacks.

Other districts use the `modern` style with the existing generic facades. No new runtime dependencies, per-building lights, physics changes or network fields. Rendering expands recipes only as chunks load, not every frame. Low quality continues to omit facade detail.

## Deliberate limits / next experiments

This is a plot-preserving experiment, not a new parcel generator: broad industrial plots above 20m, attached terraces and courtyards need a later layout pass. Roofs use flat parapets and equipment rather than pitched/sawtooth meshes. Small relief and roof equipment are visual-only; the rectangular shell remains collision-bearing. No interiors or rooftop traversal are promised.

Compare street-level readability, silhouette at driving distance, draw calls and frame time before increasing detail. The first pass targets fewer than 260 decorative cube instances per building; material batching avoids a draw call per piece. The existing content audit does not count client-generated facades, so use live renderer metrics for their cost. Test performance on target hardware before claiming a frame-rate budget.

Sources: [Instant Architecture (Wonka et al., 2003)](https://www.cg.tuwien.ac.at/research/publications/2003/Wonka-2003-Ins/) and [Procedural Modeling of Buildings (Müller et al., 2006)](https://www.peterwonka.net/Publications/pdfs/2006.SG.Mueller.ProceduralModelingOfBuildings.final.pdf). This implementation borrows split-grammar structure; it is not a full CGA interpreter.

## Review and verification

Open `/architecture` on the local preview server. Select Old Quay or Ironworks, adjust width/depth/floors and use Another building to compare variants. The preview shares gameplay recipes and facade generation, with brighter review lighting. Orbit and zoom are available; sliders affect only the preview, not authored world content.

Verified on 2026-10-31: production build, TypeScript checks, ESLint, 133 frontend unit tests, Rust workspace tests, and generated-content drift checks passed. Existing Rust unused-variable warnings and the Vite large-chunk warning remain. Browser checks covered both styles, floor/width changes, a live local game join and successful WS/RTC diagnostics. At the Old Quay spawn, the local browser reported about 60 FPS / 16.8ms, 188 draw calls, 132,658 triangles and 14 resident chunks. This is a single-machine smoke test, not a target-device benchmark.

The local game server was rebuilt and restarted with the new content. The VPS deployment was not updated by this experiment.

## Expansion to all six neighbourhoods

The initial two-district experiment was approved and expanded on 2026-10-31. All six districts now use dedicated recipes; `modern` remains only as a compatibility fallback, rather than an active city style. The preview selector is populated directly from the manifest and frames tall buildings automatically.

| District | Footprint ranges | Floors | Character |
| --- | --- | --- | --- |
| North Heights | 12–18 × 14–20m | 5–9 at 3m | Taller apartment blocks, alternating balcony stacks, glazed lobby, lift housing and optional aerial |
| Civic Center | 16–20 × 14–20m | 3–6 at 4m | Pale masonry, tall windows, pilasters, entrance columns and a layered roof crown |
| Market Grid | 10–16 × 12–18m | 3–5 at 3.2m | Warm mixed-use blocks, multiple shopfronts, coloured awnings/signboards and roof equipment |
| East Docks | 14–20 × 18–20m | 1–2 at 4.5m | Low warehouses, horizontal cladding, paired loading bays and a shallow stepped ridge |

Old Quay and Ironworks retain their approved recipes. All widths and depths still fit the existing plots; significant footprint/height differences are baked into collision. Balconies, canopies and rooftop details remain decorative. New facade tests exercise all six recipes at minimum/maximum dimensions and two variants, limiting relief to 0.75m, roof additions to 3m and decoration to fewer than 420 pieces per building. Rust content tests now check dimensional variety and visual/collision agreement in all six districts.

Expansion validation: 135 frontend tests and the Rust workspace tests pass, alongside TypeScript, ESLint and content drift checks. Native server, WASM and frontend production builds completed. All four new styles were inspected in the browser; the North Heights floor control was checked at 5 and 9 floors. The rebuilt local server passed the baseline/StartPlay/WebSocket/RTC probe and returned HTTP 200 readiness. The existing build warnings remain; this expansion has not been deployed to the VPS.
