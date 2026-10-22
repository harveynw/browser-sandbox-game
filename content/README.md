# Greybox source and assets

Edit `source/city.json` for the live 768 m city and `source/slice.json` for the preserved regression fixture; never edit generated packages directly. Rust `content` owns the DTOs and validation. `npm run content:bake` emits contracts and both immutable packages; `npm run content:check` detects contract, source/output and extra-file drift without writing files. `npm run build` bakes before building Vite; `npm run check` requires committed generated output to match.

Preview with `npm run dev`, then open http://localhost:5173/map. This page needs only the Vite/static server. Its car models are parking previews, not physical entities. Use the inspector controls for collision, reservations, directed vehicle lanes, sidewalks/crossings and the courier route.

The source contains the exact gameplay anchors, clear rectangles and driveways. Road and building recipes produce all geometry deterministically in Rust. Heights use an explicit wrapping integer generator seeded by the source; runtime/browser code never independently randomizes buildings. Primitive visuals and collision boxes share geometry. Chunks split solid visuals at their bounds while retaining original collider IDs, so no duplicate collision objects are needed.

The live package is indexed by `apps/client/public/content/city.json`; `slice.json` independently indexes the regression package. Each points to `packages/<content_hash>/manifest.json`; all referenced paths stay inside that immutable directory. Publishing writes assets first, then atomically replaces the index. Existing hash-addressed files are compared, not overwritten. Older immutable packages may remain during development; checks inspect the current packages and generated contracts. Do not mix assets across package hashes.

See [the implementation guide](../docs/18-content-pipeline.md) for APIs, hashing and validation coverage, and [the full-city guide](../docs/26-full-city.md) for T12 streaming, budgets and evidence. Asset provenance is recorded in [ASSET-LICENSES.md](ASSET-LICENSES.md).
