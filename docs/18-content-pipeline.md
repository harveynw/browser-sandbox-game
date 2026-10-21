# T04 — Baked slice and standalone inspection

## Entry points

- `content/source/slice.json`: authoritative JSON authoring input. Coordinates use meters, +Y up and north = −Z. Rectangle arrays are `[X,Z]`; all positions/offsets are `[X,Y,Z]`. Character positions are feet and vehicle positions bottom-center.
- `crates/content/src/types.rs`: Serde + Schemars + ts-rs DTOs. Unknown fields are rejected; nullable chunk/primitive fields require explicit null.
- `content::bake::bake(&Authoring) -> Result<BakedPackage>`: pure deterministic bake, validation and complete file bytes. No filesystem, network or physics dependency.
- `content::validate::authoring`: source bounds, number domain, IDs/references, reservations, driveways, template/config values and anchor placement.
- `content::validate::package`: collider/road/clearance checks, graph connectivity and clear edges, referenced-file integrity, visual/collision agreement and vehicle template geometry. This is the baker's package verification, not a native runtime filesystem loader. T07 must load and verify a complete package before admission.
- `content::canonical::{bytes, sha256, manifest_hash}`: canonical serialization and hash implementation shared through generated fixtures with TS.
- `@port-meridian/content`: generated types plus `loadPackage`, `manifestHash`, `canonicalJson`, `sha256` and `findRoute`.

`cargo run -p xtask --locked -- generate-content` generates nine contract/fixture files under `packages/content/src/generated`. `bake-content` generates those and publishes the package/index. `check-content` regenerates in memory and compares without mutating the repository; it also rejects extra files in the generated contract/current package directories. npm exposes these as `content:generate`, `content:bake` and `content:check`.

## Output and integrity

`apps/client/public/content/slice.json` is a small versioned index with package content hash, relative manifest URL and exact manifest byte hash. The package directory is immutable and contains:

| File | Consumer / purpose |
|---|---|
| `manifest.json` | Bounds, roads, reservations/access links, graphs, anchors, templates, chunk and asset references/hashes |
| `collision.json` | Complete simplified static boxes, with stable numeric IDs, roles and flags; native/WASM consumers in later tasks |
| `sim-config.json` | Character and vehicle motor constants, separately hashed; behavior begins in T05/T10 |
| Four `chunk_<cx>_<cz>.json` files | Renderable primitive boxes in full-city chunk coordinates `(1..2,3..4)` |
| `compact_greybox.json` | Original body/windows primitives relative to a vehicle bottom-center anchor |
| `debug.json` | Identical collider records plus reservations/accesses for inspection |

Hashing rules are **content canonical JSON v1**, not a claim of RFC 8785 implementation. Object keys sort lexicographically and must be printable ASCII. Array order is preserved; strings use JSON escaping. Numbers are finite, within ±1,000,000, and on a six-decimal grid (scaled floating tolerance 0.0001). Serialize with six fractional places, trim trailing fractional zeros/dot, normalize negative zero to zero, and never use exponent notation. Do not insert runtime entity IDs, timestamps or random values into content.

The manifest content hash covers canonical manifest data after removing its own `content_hash` field. Referenced byte hashes make collision, visuals, config and debug changes affect package identity. The sim-config hash covers only canonical config bytes. The outer index additionally checks exact manifest bytes. No file URL contains the package hash internally, avoiding self-referential hashing. Bytes do not include a trailing newline.

The TS loader first validates the index path, verifies exact manifest bytes and canonical package identity, then fetches/validates every referenced file, at most four concurrently. Frames/files are streamed with a 2 MiB per-asset cap (index 4 KiB), aborted together on failure, and parsed with generated 2020-12 schemas. Unsigned integer formats and finite doubles are checked. It rejects unsafe paths, mismatched IDs, missing files, wrong versions, changed hashes and debug/collision disagreement before returning `LoadedPackage`. A failed inspector offers Retry; it never displays an unverified partial world. Runtime retry/streaming policy for actual play belongs to T08/T12.

Hashes detect package mismatch/corruption; they are not authentication or signatures.

## Geometry and navigation

This task bakes only `x=[-256,0], z=[0,256]`, with ground at Y=0 and four visible 4 m boundary walls. All collision boxes have yaw zero in T04; unsupported rotation is rejected rather than tested with incorrect axis-aligned math. Ground supports the complete slice. Building footprints lie on the authored 4 m grid, use 20 m squares with 8 m spacing, and stay outside 12 m setbacks and every reservation. Three colored solid landmarks are intentionally rough greyboxes, not final towers/canopies.

**Slice-only road correction:** outer centerlines move 10 m inward, to X `[-246,-128,-10]` and Z `[10,128,246]`. This fits complete 12 m carriageways and 3 m sidewalks inside temporary barriers. The full-city centerlines in document 02 remain unchanged; T12 removes the temporary boundary and replaces this slice road recipe with the city grid/ring.

Reservations preserve the specified spawn, terminal, parking, delivery and garage anchor coordinates. They include a clear parking apron and driveways at least 5 m wide. Validation conservatively checks solid-box clearance for the spawn, each parked car's dimensions and all three prospective capsule exit anchors. Actual capsule sweeps/floor queries belong to T05; dynamic occupancy belongs to T07/T10.

Vehicle graph edges use offset right-hand lanes, directed intersection connections and bidirectional driveway spurs. Pedestrians have a separate bidirectional sidewalk graph with explicit crossing edges. Access points split existing edges, preserving connectivity instead of creating disconnected markers. Every edge is checked against solid geometry with its width; both graphs must be strongly connected, including every access spur. This proves the authored courier terminal/destination can route through clear edges. `findRoute` uses distance/speed Dijkstra and returns graph positions, not a straight-line target arrow.

Visual solids derive directly from collider records. Chunk clipping retains the source collider ID. Validation checks each clipped face/extent, rejects duplicate collider visuals in a chunk, and verifies complete solid coverage. The debug overlay uses those same collision records; its rendering wire offset is 1 cm. Flat road/sidewalk tops remain within 5 cm of Y=0 collision, with draw offsets to prevent flickering. No physics shape is inferred from a rendered mesh.

## Inspector and handoff

Run `npm run dev`; visit [localhost:5173/map](http://localhost:5173/map). It creates no game transport/session. Orbit/pan/zoom inspect the package; isometric/top presets reset the view. Expand **Layers and inspection controls** for collision, reservations, vehicle graph, sidewalks, courier route and labels. Controls collapse initially in narrow panels; framing keeps the full slice visible. Resources, animation, resize/context listeners and outstanding requests are disposed on teardown/HMR. The original transport diagnostic remains at `/` and is lazy loaded independently.

T05 should consume `CollisionPackage` and `SimConfig` in native Rust, implement actual collision queries/motor behavior, and retain complete motor state. Do not copy the TS inspector's visual geometry into physics, independently regenerate content, or implement movement in TS. T06 exposes that motor through WASM. T07 replaces T03's placeholder admission/hash values with real package compatibility checks. T08 builds the actual third-person game scene; this inspector is not that scene.

No walking, driving, entity replication, jobs or persistence has been implemented in this task.
