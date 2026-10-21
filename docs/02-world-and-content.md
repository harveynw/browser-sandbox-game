# 02 — City and content

[Index](../README.md)

## Coordinate contract

One unit is one meter. Right-handed coordinates: +X east, +Y up, −Z north. Gameplay yaw 0 faces −Z; positive yaw turns toward −X using the standard positive rotation about Y. Camera-relative “right” at yaw 0 is +X. Use radians and seconds. Convert art assets at import, not in network code.

City bounds: `x,z ∈ [-384,384]`, ground `y = 0`, ocean visual plane `y = -2`. Solid perimeter barriers sit at the city bounds. Players cannot enter the ocean in alpha. Buildings and props have positive Y; all drivable road surfaces are flat at Y = 0 in alpha. Ramps, bridges, multi-level roads, and physics jumps for cars are later.

Characters store feet position; cars store bottom-center position. Render anchors/collider centers are derived offsets. Content must never guess which anchor a position means.

## District plan

The map has six 256 × 384 m districts, with 6 × 6 visual chunks of 128 m each. A chunk key is `(cx,cz)` with each axis 0–5 and origin `(-384 + 128*cx, 0, -384 + 128*cz)`. District borders do not have to match every chunk border.

```text
                              NORTH (−Z)
                  X −384       −128         128          384
Z −384               ┌────────────┬────────────┬────────────┐
                     │ NORTH     │ CIVIC      │ IRONWORKS  │
                     │ HEIGHTS   │ CENTER     │            │
                     │ park /    │ square /   │ warehouses │
                     │ villas    │ station    │ / factory  │
Z    0               ├────────────┼────────────┼────────────┤
                     │ OLD QUAY  │ MARKET     │ EAST DOCKS │
                     │ spawn /   │ GRID       │ cranes /   │
                     │ alleys    │ shops      │ containers │
Z  384               └────────────┴────────────┴────────────┘
                              SOUTH (+Z)
```

| District | Identity | Gameplay use | Landmark |
|---|---|---|---|
| Old Quay | Brick warehouses, faded painted signs, narrow pedestrian cuts | Spawn, first car, tutorial delivery | Orange water tower |
| Market Grid | Bright awnings, dense low-rise streets | Courier routes, close turning | Green market canopy |
| East Docks | Long sightlines, containers, fences | Vehicle recovery and high-speed roads | Two blue cranes |
| Ironworks | Factory yards and right-angle service roads | Pursuit loops, repair bay | Red chimney |
| Civic Center | Open plaza, clean concrete, station | Respawn, police staging, job terminal | White clock tower |
| North Heights | Parks and larger lots, flat roads despite name | Time trial and quieter driving | Hill silhouette outside playable bounds |

Use simple low-rise blocks with two or three distant silhouette buildings. Do not construct every room. Streets should provide alternating openness and enclosure, not a uniform maze.

## Roads and parcel generation

Primary north/south and east/west road centerlines: `-256, -128, 0, 128, 256`. Road carriageway width 12 m, two 6 m lanes. Sidewalks are 3 m on each side. Reserve a corridor extending 9 m from each centerline; buildings start at least another 3 m away. Each intersection is a level square with crosswalk markings. Curbs are visual only in alpha.

Outer ring centerline is the rectangle at `x = ±352`, `z = ±352`, connected to grid roads. Its same road/sidewalk envelope stays inside the map. No random buildings in these corridors.

Lot fill algorithm:

1. Enumerate rectangles between reserved road corridors.
2. Subtract explicit landmark, plaza, parking, mission, and spawn reservations.
3. Place building footprints on a 4 m grid with at least 3 m separation.
4. Buildings use seeded choices for height/color only. Geometry and collider footprints are baked, never independently randomized at runtime.
5. Provide at least one 3 m-wide pedestrian passage per interior block and a connected sidewalk path around every block.

Do not place collision props in the center of an intersection or a mission destination. Decorative poles/bins are non-colliding initially. Physics collision is reserved for buildings, barriers, key fences, and vehicles.

## Initial slice

The first playable area is `x ∈ [-256,0], z ∈ [0,256]`. It spans parts of Old Quay and Market Grid so it includes useful intersections. During slice development, temporary visible barriers bound this rectangle. Remove them when expanding the map.

Explicit reservations and anchors (positions are `[x,y,z]`):

| ID | Position | Reservation/use |
|---|---|---|
| `spawn_quay` | `[-180,0,180]` | 24 × 24 m plaza; player spawn |
| `terminal_quay` | `[-172,0,180]` | Courier terminal; 3 m interaction range |
| `parking_quay_01` | `[-152,0,176]` | 6 × 10 m clear parking space, yaw 0 |
| `parking_quay_02` | `[-152,0,192]` | Same |
| `delivery_market` | `[-44,0,52]` | 16 × 16 m clear destination lot with driveway to z = 0 road |
| `garage_quay` | `[-220,0,48]` | 20 × 20 m garage forecourt with driveway to x = −256 |
| `spawn_civic` | `[40,0,-180]` | 24 × 24 m safe plaza |
| `terminal_civic` | `[48,0,-180]` | Courier/recovery terminal |
| `terminal_trial` | `[0,0,-256]` | Drive-through time-trial start zone; physical sign placed on sidewalk |
| `recovery_docks` | `[300,0,180]` | 24 × 24 m reserved secured-vehicle lot |
| `repair_ironworks` | `[220,0,-180]` | 20 × 20 m forecourt |

A reservation is a world-space rectangle recorded in content, with a minimum 5 m driveway to the nearest road. Bake-time validation must catch overlap with building colliders. Adjust art around these anchors rather than silently moving gameplay markers.

Initial courier route: `terminal_quay → delivery_market`. Suggested driving route is to the x = −128 road, then north to z = 0, then east and into the destination driveway. Waypoint guidance uses the road graph; do not draw a straight arrow through buildings.

The 24 × 24 m `recovery_docks` reservation contains eight 6 × 12 m parking cells: centers `x = 291, 297, 303, 309`, `z = 174, 186`, all Y = 0 and yaw 0. Give each a unique parking ID, `recovery_docks_01` through `_08`. A recovery template selects a free slot; it does not spawn eight cars at the same anchor. Reserve an additional clear approach apron/driveway around the lot when generating buildings.

Required template IDs: `courier_quay_market` (Quay terminal to market), `trial_city_ring` (drive-through trial terminal and gates below), and `recovery_docks_quay` (Quay/Civic terminals to a free dock slot, then Quay garage). Terminals list allowed template IDs explicitly. Additional courier routes can reuse the same rules with new data, not new message kinds.

Full-city time-trial checkpoint centers:

```text
start [0,0,-256]
  -> [256,0,-256]
  -> [256,0,128]
  -> [0,0,256]
  -> [-256,0,128]
  -> [-256,0,-256]
  -> [0,0,-256]
```

These checkpoints define gates, not mandatory straight-line segments. A navigation route must traverse graph edges around corners. Gates have a radius of 8 m, ordered progression, and swept crossing detection to avoid missing a gate between ticks.

## Content data interfaces

Rust `content` owns these structures, derives Serialize/Deserialize/JsonSchema/TS, and exports their browser counterparts. Content is JSON at authoring time and a versioned JSON manifest at runtime; network messages remain MessagePack.

```ts
type Vec3 = [number, number, number];
type WorldManifest = {
  schema_version: number;
  world_asset_id: string;       // "port_meridian_v1"
  content_hash: string;         // SHA-256 hex; see hash rule below
  sim_config_hash: string;
  bounds: { min: Vec3; max: Vec3 };
  collision_url: string;
  collision_sha256: string;
  chunks: ChunkAsset[];
  road_graph: RoadGraph;
  pedestrian_graph: RoadGraph;
  spawns: SpawnPoint[];
  parking: ParkingSpawn[];
  zones: Zone[];
  mission_templates: MissionTemplate[];
  vehicle_templates: VehicleTemplate[];
};
type ChunkAsset = {
  id: string; cx: number; cz: number;
  visual_url: string; sha256: string;
  lod_url: string | null;
  bounds: { min: Vec3; max: Vec3 };
};
type StaticCollider = {
  id: number;
  shape: "box";
  center: Vec3;
  half_extents: Vec3;
  yaw: number;
  flags: number;               // 1=movement solid, 2=vision/shot blocker
};
type RoadGraph = {
  nodes: { id: number; position: Vec3 }[];
  edges: {
    from: number; to: number; width: number;
    speed_limit: number;      // m/s
  }[];
};
type SpawnPoint = { id: string; position: Vec3; yaw: number; safe_radius: number };
type ParkingSpawn = { id: string; position: Vec3; yaw: number; template_id: string; secured: boolean };
type Zone = { id: string; center: Vec3; radius: number; kind: "delivery" | "garage" | "repair" | "terminal" };
```

`MissionTemplate` has `id`, tagged `kind`, `terminal_zone_id`, `duration_ticks`, `reward_credits`, plus kind-specific destination/checkpoint/vehicle-spawn references. `VehicleTemplate` includes dimensions and the values from document 01, visual asset ID, seat and exit offsets. Template IDs are strings capped at 64 ASCII characters. Entity IDs are runtime numbers; never put them in authored templates.

Hashes use canonical JSON (sorted object keys, stable array order, finite numbers) excluding the manifest’s own `content_hash` field. The manifest includes hashes of referenced content and assets, so changing collision or an asset changes the package hash. `sim_config_hash` covers movement constants separately. Serve a whole immutable package; never mix files from two builds.

## Loading and navigation

- Server loads all collision, graph, template, and zone data before becoming ready.
- Browser loads the complete simplified collision package before `ClientReady`; target ≤ 2 MiB decoded JSON. No runtime collision streaming in alpha.
- Browser loads visuals for the current chunk and neighbors within 192 m; keep them until farther than 256 m, subject to memory budget. Always keep the spawn chunk until play starts.
- Load with bounded concurrency (4 requests), retry twice with backoff, and use a neutral visible placeholder on failed distant art. A missing collision package blocks play with a retry screen.
- Abort obsolete asset requests; reference-count shared textures/materials and dispose only when no chunks/entities use them.
- Keep a low-detail city silhouette and ground visible while chunks arrive. Do not send world meshes over the game socket.

Use directed lane graphs for vehicles and a separate sidewalk graph for pedestrians. Dijkstra/A* with distance/speed costs is sufficient. A content validation pass must prove connectivity between each mission terminal/destination and the road/sidewalk graph. No runtime navmesh generation is required.

## Population budgets

Full shard caps: 16 players, 24 parked cars, 24 traffic cars, 32 pedestrians, and 8 police units total. Police units are either cars or on-foot officers, never two independently counted actors for one unit. Recovery cars consume the parked-car budget; reserve 8 of those slots for jobs and queue/reject additional recovery jobs with a clear `unavailable` result. A car keeps its original population-pool tag when parked or taken by a player; a former traffic car still occupies one of the 24 traffic slots. This prevents ownership changes from exceeding the total cap.

Only spawn ambient NPCs within 160 m of at least one player, and outside 40 m/clear camera sight where possible. Despawn unowned ambient actors after 30 seconds beyond 220 m from all players. Job-reserved, occupied, or pursuing actors are pinned. Visual unloading never despawns the server entity.

## Asset pipeline and map acceptance

Use modular buildings, roads, cars, characters, and props with explicit license records. Art and collision are separate outputs of the same authoring source. Authoring can begin entirely in JSON with primitive greyboxes; Blender/glTF is added later.

Required checks:

- Every spawn and vehicle exit anchor can fit its capsule/box without overlap.
- Every job has a reachable start and finish; checkpoint graph connectivity passes.
- No road corridor is blocked by a generated building; no duplicate collider IDs.
- World rendering and collision debug overlay agree within 5 cm at walls.
- Driving the ring road and initial courier loop encounters no holes or invisible walls.
- At least three distinguishable landmarks are visible from major intersections.

## T04 implemented slice contract

The standalone slice now has an authored source, Rust bake/validation, generated TS/schemas and a verified browser inspector. See [18 — Content pipeline](18-content-pipeline.md) for exact APIs/output. The full-city plan above remains the T12 target. For the temporary slice only, edge roads move 10 m inward to keep both lanes and sidewalks inside its solid boundary; anchors retain their documented positions. Primitive JSON chunks are the implemented greybox asset format. The runtime manifest additionally records reservations/access links, config/debug references and vehicle visual assets. Content hashing uses the restricted six-decimal canonical JSON v1 contract documented in 18.
