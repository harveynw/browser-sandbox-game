# World content and asset pipeline

[Documentation index](../README.md#documentation) · [Asset provenance](../content/ASSET-LICENSES.md)

## Authored and generated data

Edit **`content/source/city.json`** for the live city. **`content/source/slice.json`** is a compact deterministic regression package for tests/traces. It is not a separate gameplay mode selected by persistence configuration. The release server and browser select `city.json`; `config/memory.toml` only changes storage.

Rust `content` owns content/config DTOs, validation, canonical hashing, baking and graph data. `xtask` generates TS/Schemas for `packages/content`, bakes immutable packages into `apps/client/public/content/`, and checks drift. Do not edit generated packages or duplicate the baker's geometry generation in the browser.

| Command | Purpose |
|---|---|
| `npm run content:bake` | Generate content contracts, validate sources and bake live/regression packages |
| `npm run content:generate` | Generate content declarations/schemas |
| `npm run content:check` | Detect contract/source/output/extra-file drift without rewriting |
| `npm run content:audit` | Measure compressed downloads and managed-buffer budgets after building |

The source defines road/building recipes, reservations, access spurs, gameplay zones, parking, spawns, vehicle templates, mission templates, landmarks, districts and simulation settings. Clear rectangles preserve access to spawns, car doors, terminals, destinations and garages. Ordinary IDs such as `compact_greybox` are stable content identifiers; presentation derives from original procedural geometry.

## Package and hash contract

The live index is `apps/client/public/content/city.json`. The regression index is `slice.json`. Each references `packages/<content_hash>/manifest.json` with an exact manifest byte hash. The immutable directory contains collision, simulation settings, visual chunks, vehicle visuals, backdrop, minimap and debug data. References remain inside the package directory; there are no cross-package or arbitrary URL lookups.

The package hash covers canonical manifest data with its own `content_hash` omitted. Referenced asset byte hashes make settings/geometry/visual/debug changes affect identity. The simulation hash covers canonical config bytes. Assets are written before an atomic index replacement; existing hash-addressed files are compared rather than overwritten.

Canonical JSON v1 is a project contract, not RFC 8785: object keys sort lexicographically and use printable ASCII; arrays retain order; strings use JSON escaping. Numbers are finite, within ±1,000,000, on a six-decimal grid, serialized without exponents/trailing fractional zeroes and with negative zero normalized. Do not put timestamps or runtime entity IDs into authored data. Hashes detect mismatches/corruption; they are not signatures/authentication.

## Geometry and navigation

The live city contains **385 static colliders** and **36 visual chunks**, each **128 m** square, across a **768 m** world. Building placement is a deterministic seeded grid recipe respecting streets, setbacks and reservations. Solid visuals derive from collider geometry; chunk clipping retains source collider IDs and does not duplicate physics. Baked map geometry is axis-aligned; native collision/vehicle queries support yaw-oriented boxes.

Car routes use directed right-hand lanes, intersection connections and bidirectional driveway access. Pedestrian routes use bidirectional sidewalks and explicit crossings. Access points split graph edges; both graphs are checked for strong connectivity and swept-width geometric clearance. `findRoute` uses shortest travel time, not a straight line through buildings. The regression package preserves its bounded geometry and frozen traces independently.

Validation checks DTO versions/ranges, references/IDs, reservation/solid overlaps, spawn/parking/exit clearance, graph connectivity/edge clearance, asset correspondence and package hash integrity. Content collision uses movement/vision flags; rendered geometry never substitutes for query data.

## Verified loading and streaming

The TS loader validates the index/path, exact manifest bytes, canonical package identity, generated content schemas and each asset's SHA-256 hash. Index size is capped at **4 KiB** and each asset at **2 MiB**, with at most **four concurrent** requests. Unsafe paths, missing/mismatched assets, invalid versions and debug/collision disagreement fail loading. Cancellation aborts dependent work.

The game preloads complete static collision, simulation config and bootstrap/debug/vehicle/backdrop/minimap data before readiness. It loads spawn detail initially; the standalone inspector loads full detail. Physics never waits for a visual chunk.

`ChunkStream` requests detail within **192 m** of chunk bounds, retains it through **256 m**, and caps detailed residents at **24**. Requests are distance ordered, have eight-second timeouts and bounded retry/backoff; obsolete work aborts. Failed detail keeps a coarse backdrop and can retry on re-entry. Disposal/cancellation prevent late callbacks from resurrecting removed graphics.

The renderer instances primitives by layer/palette. Eviction disposes instance buffers, restores coarse backdrop coverage and retains shared primitive/material ownership until renderer teardown. It never removes authoritative actors/collision. The baked map supplies the HUD and city map. Track resident chunks, pending/fallback counts, matrix bytes and geometries in Diagnostics; managed buffers are not a total browser/GPU memory measurement.

## Inspector and asset ownership

`/map` is a transport-free content inspector with orbit/pan/zoom, framing, collision, reservations, graphs, route and label layers. `/simulation` is the independent shared-motor diagnostic. Neither tool is an authoritative game client.

All models, facade patterns, character rigs, landmarks, procedural effects, UI/map graphics and synthesized sounds are authored in this repository. No external recorded audio, music, models, texture packs or font files are required. [ASSET-LICENSES.md](../content/ASSET-LICENSES.md) records provenance without assigning a new public license to the repository.

## Mission variants

Each mission template may author a `variants` array of up to 16 named contracts. A variant supplies `name`, `duration_ticks`, `reward_credits` and a complete `objective` of the template's existing kind. Courier objectives reference validated delivery zones; trial variants author closed, connected checkpoint routes starting at the terminal. Empty or omitted variants preserve the base objective. Baking validates every option, including destination references, names, payout/duration bounds and route connectivity.

`MissionSystem` selects once from the random mission instance ID, stores the selected objective and payout, and publishes `variant_index` in the private mission view. This index refers to the hash-pinned content package, allowing the HUD to display the selected contract without choosing gameplay rules itself. Recovery independently ranks all eligible parking slots, retaining capacity, ownership and clearance checks.
