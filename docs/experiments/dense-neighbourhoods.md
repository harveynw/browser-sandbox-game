# Dense neighbourhood generation

The full city uses adjoining 16m/20m frontage parcels, 20m deep, with no required parcel spacing. Footprints align to parcel edges toward the closest street on each axis, so variations create yards and passages and full-size shells can attach. Roads, pavements and authored access reservations remain protected. Collision validation rejects actual overlaps rather than imposing a universal 3m passage between all buildings.

Each district authors three weighted archetypes: primary (65%), smaller local use (20%), and a supporting use (15%). These reuse the six architectural vocabularies with purpose and dimension variations. District influences use separable smoothstep ramps extending 48m either side of each boundary and normalized weighted sampling. A 40m coordinate seed groups district selections locally. Building dimensions and subtype selection retain coordinate-derived seeds.

Baked building metadata carries the selected architecture, purpose and nearby shell bounds. Client facades consume this selection rather than reselecting from district rectangles. Shared-wall details intersecting neighbouring shells are removed, including neighbours in other streaming chunks. Shell colours are baked from the selected architecture for consistent backdrop/minimap appearance. Building IDs derive from shell position rather than collider sequence.

Validation: TypeScript, ESLint, 136 frontend tests, Rust workspace tests, content drift checks and client production build passed. Added tests cover baked recipe selection, shared-wall suppression, mixed purposes/styles, deterministic output and collision/visual agreement.

Current limits: rectangular shells and a maximum 20m footprint; incidental gaps are not a generated pedestrian navigation network. Required gameplay access uses authored reservations. Purpose variants reuse existing facade grammars; this does not add interiors. The multi-building preview, live street-level visual review and target-hardware performance measurement remain outstanding. No deployment or running-server restart was performed.
