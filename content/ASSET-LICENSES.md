# Asset provenance

| Asset | Authoring source | Provenance |
|---|---|---|
| City, regression map, roads, markings, district palettes, backdrop and minimap | `source/city.json`, `source/slice.json`, `crates/content/src/bake.rs` | Original procedural geometry and data |
| Water tower, market canopy, garage beacon, Civic clock, Ironworks chimney, dock cranes and Heights pavilion | Authored map dimensions and colours | Original procedural landmark silhouettes |
| Compact, van and sport bodywork, framed glass, trim, wheels, lamps and police equipment | Authored vehicle templates and runtime renderer | Original procedural primitives |
| Articulated characters and four clothing palettes | `apps/client/src/render/character.ts` | Original box rig and animation |
| Eight facade patterns | `apps/client/src/render/facades.ts` | Original runtime patterns |
| Tires, rims, spokes, lights, cosmetic lean, signs, sky, pistol, tracers, impacts, smoke, muzzle flash and tire dust | `apps/client/src/render/renderer.ts` | Original procedural runtime geometry and effects |
| Garage/repair rings, poles, map/UI drawings, collision overlays, diagnostic graphs and styling | Renderer, `ui/`, `style.css` | Original primitives, drawings and system-font styling |
| Footsteps, engine, skid, doors, shots, impacts, reload, siren and mission cues | `apps/client/src/audio/sound.ts` | Original oscillator/noise synthesis |

All authored media is project-controlled. No external models, textures, font files, recorded sound or music are imported. This record does not assign a new public license to the repository. Three.js and other software dependencies retain their own package licenses. Add a provenance entry before introducing externally sourced media.
