# Asset provenance

| Asset | Source | Provenance / licensing |
|---|---|---|
| Four slice primitive chunks | `source/slice.json`, `crates/content/src/bake.rs` | Original procedural boxes, roads and markings created in this repository; project-controlled content |
| Quay water tower, market canopy, garage beacon | Landmark dimensions/colors in `source/slice.json` | Original colored greybox primitives; project-controlled content |
| `compact_greybox` | Compact dimensions in `source/slice.json`, generated body/windows | Original procedural primitives; project-controlled content |
| Collision, debug overlay and graphs | Same authored source and baker | Original project data; no third-party models/textures involved |
| 36 city chunks, district palettes, backdrop and minimap | `source/city.json`, the same Rust baker | Original procedural geometry/data; no imported media |
| Civic clock, Ironworks chimney, dock cranes, Heights pavilion, distant hill and ocean | City source and baker | Original colored greybox boxes; project-controlled content |
| Van and sport greyboxes | City vehicle templates and baker | Original procedural bodies/windows; project-controlled content |

No downloaded models, textures, fonts or audio have been added. These records do not assign a new public license to the repository. Three.js and other software dependencies retain their own package licenses; their source is not copied into authored asset files. Add a provenance entry before introducing externally sourced media.

T14 pistol boxes, tracer lines, impact markers and disabled-car smoke blocks are original procedural runtime geometry in `apps/client/src/render/renderer.ts`; no external asset files were added.

T15 police cars reuse the original procedural compact and add an original blue roof beacon. The second Quay compact is marked secured in city authoring; the visual assets retain their original provenance.

T16 adds original procedural blue garage and green repair rings/poles, using the existing shared primitives. No third-party assets were added.
