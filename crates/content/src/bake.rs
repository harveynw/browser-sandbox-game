use crate::{Result, canonical, types::*, validate};
use std::collections::BTreeMap;

#[derive(Debug)]
pub struct BakedPackage {
    pub manifest: WorldManifest,
    pub files: BTreeMap<String, Vec<u8>>,
}
fn collider(
    colliders: &mut Vec<StaticCollider>,
    role: ColliderRole,
    center: Vec3,
    half_extents: Vec3,
) {
    colliders.push(StaticCollider {
        id: colliders.len() as u32 + 1,
        shape: BoxShape::Box,
        role,
        center,
        half_extents,
        yaw: 0.0,
        flags: if role == ColliderRole::Ground { 1 } else { 3 },
    });
}
fn primitive(
    id: String,
    role: VisualRole,
    center: Vec3,
    half_extents: Vec3,
    color: &str,
    collider_id: Option<u32>,
) -> Primitive {
    Primitive {
        id,
        role,
        center,
        half_extents,
        color: color.into(),
        collider_id,
        building: None,
    }
}
fn asset<T: serde::Serialize>(
    files: &mut BTreeMap<String, Vec<u8>>,
    name: &str,
    value: &T,
) -> Result<String> {
    let bytes = canonical::bytes(value)?;
    let hash = canonical::sha256(&bytes);
    files.insert(name.into(), bytes);
    Ok(hash)
}
fn coordinate_seed(seed: u32, x: f64, z: f64) -> u32 {
    let v =
        seed ^ (x as i32 as u32).wrapping_mul(73856093) ^ (z as i32 as u32).wrapping_mul(19349663);
    let v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
    let v = (v ^ (v >> 15)).wrapping_mul(0x846ca68b);
    v ^ (v >> 16)
}
fn select_architecture(
    source: &Authoring,
    x: f64,
    z: f64,
    px: f64,
    pz: f64,
) -> Option<(&ArchitectureRecipe, &str)> {
    let weights: Vec<_> = source
        .districts
        .iter()
        .map(|d| {
            let axis = |v: f64, lo: f64, hi: f64| {
                let t = ((v - lo + 48.0) / 96.0).clamp(0.0, 1.0);
                let u = ((hi + 48.0 - v) / 96.0).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t) * u * u * (3.0 - 2.0 * u)
            };
            axis(x, d.rect.min[0], d.rect.max[0]) * axis(z, d.rect.min[1], d.rect.max[1])
        })
        .collect();
    let total: f64 = weights.iter().sum();
    let mut pick = f64::from(coordinate_seed(
        source.buildings.seed,
        (px / 40.0).floor() * 40.0,
        (pz / 40.0).floor() * 40.0,
    )) / (f64::from(u32::MAX) + 1.0)
        * total;
    for (d, w) in source.districts.iter().zip(weights) {
        if pick < w {
            let total: u32 = d.archetypes.iter().map(|a| a.weight).sum();
            if total == 0 {
                return Some((&d.architecture, "housing"));
            }
            let mut choice = coordinate_seed(source.buildings.seed ^ 0xa1b2c3d4, px, pz) % total;
            for a in &d.archetypes {
                if choice < a.weight {
                    return Some((&a.architecture, &a.purpose));
                }
                choice -= a.weight;
            }
            return Some((&d.architecture, "housing"));
        }
        pick -= w;
    }
    None
}
pub fn bake(source: &Authoring) -> Result<BakedPackage> {
    validate::authoring(source)?;
    let b = &source.bounds;
    let center = [
        (b.min[0] + b.max[0]) / 2.0,
        -0.5,
        (b.min[2] + b.max[2]) / 2.0,
    ];
    let mut colliders = Vec::new();
    collider(
        &mut colliders,
        ColliderRole::Ground,
        center,
        [
            (b.max[0] - b.min[0]) / 2.0,
            0.5,
            (b.max[2] - b.min[2]) / 2.0,
        ],
    );
    for x in [b.min[0] + 0.5, b.max[0] - 0.5] {
        collider(
            &mut colliders,
            ColliderRole::Boundary,
            [x, 2.0, center[2]],
            [0.5, 2.0, (b.max[2] - b.min[2]) / 2.0],
        );
    }
    for z in [b.min[2] + 0.5, b.max[2] - 0.5] {
        collider(
            &mut colliders,
            ColliderRole::Boundary,
            [center[0], 2.0, z],
            [(b.max[0] - b.min[0]) / 2.0, 2.0, 0.5],
        );
    }
    let full = source.bounds.min[0] == -384.0;
    let recipe = &source.buildings;
    let mut rng = recipe.seed;
    let mut identities = BTreeMap::new();
    for xpair in source.roads.x.windows(2) {
        for zpair in source.roads.z.windows(2) {
            let mut x = ((xpair[0] + recipe.setback) / recipe.grid).ceil() * recipe.grid;
            while x + recipe.size <= xpair[1] - recipe.setback {
                let parcel_width =
                    if full && coordinate_seed(recipe.seed ^ 17, x, zpair[0]) % 3 == 0 {
                        16.0
                    } else {
                        recipe.size
                    };
                let mut z = ((zpair[0] + recipe.setback) / recipe.grid).ceil() * recipe.grid;
                while z + recipe.size <= zpair[1] - recipe.setback {
                    let rect = Rect {
                        min: [x, z],
                        max: [x + parcel_width, z + recipe.size],
                    };
                    if !source
                        .reservations
                        .iter()
                        .any(|r| validate::overlap(&rect, &r.rect))
                    {
                        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                        let mut dimensions =
                            [recipe.size, 8.0 + 4.0 * f64::from(rng % 4), recipe.size];
                        let mut cx = x + parcel_width / 2.0;
                        let mut cz = z + recipe.size / 2.0;
                        let selected = select_architecture(source, cx, cz, x, z);
                        if let Some((architecture, _)) = selected {
                            // Coordinate-derived entropy keeps a plot stable when another is reserved.
                            let seed = recipe.seed
                                ^ (x as i32 as u32).wrapping_mul(73856093)
                                ^ (z as i32 as u32).wrapping_mul(19349663);
                            let mix = |v: u32| {
                                let v = (v ^ (v >> 16)).wrapping_mul(0x7feb352d);
                                let v = (v ^ (v >> 15)).wrapping_mul(0x846ca68b);
                                v ^ (v >> 16)
                            };
                            let sample = |range: [f64; 2], salt| {
                                // Half-metre increments avoid canonical floating-point rounding issues.
                                range[0]
                                    + f64::from(
                                        mix(seed ^ salt)
                                            % ((2.0 * (range[1] - range[0])) as u32 + 1),
                                    ) / 2.0
                            };
                            let floors = architecture.floors[0]
                                + mix(seed ^ 3)
                                    % (architecture.floors[1] - architecture.floors[0] + 1);
                            dimensions = [
                                sample(
                                    [
                                        architecture.width[0].min(parcel_width),
                                        architecture.width[1].min(parcel_width),
                                    ],
                                    1,
                                ),
                                (f64::from(floors) * architecture.floor_height * 100.0).round()
                                    / 100.0,
                                sample(architecture.depth, 2),
                            ];
                        }
                        // Anchor to the closest street edge; paired rear edges share parcel boundaries.
                        let edge_x = if cx - xpair[0] <= xpair[1] - cx {
                            x
                        } else {
                            x + parcel_width - dimensions[0]
                        };
                        let edge_z = if cz - zpair[0] <= zpair[1] - cz {
                            z
                        } else {
                            z + recipe.size - dimensions[2]
                        };
                        cx = edge_x + dimensions[0] / 2.0;
                        cz = edge_z + dimensions[2] / 2.0;
                        if let Some((architecture, purpose)) = selected {
                            identities.insert(
                                colliders.len() as u32 + 1,
                                BuildingIdentity {
                                    architecture: architecture.clone(),
                                    purpose: purpose.into(),
                                    neighbours: Vec::new(),
                                },
                            );
                        }
                        collider(
                            &mut colliders,
                            ColliderRole::Building,
                            [cx, dimensions[1] / 2.0, cz],
                            dimensions.map(|n| n / 2.0),
                        );
                    }
                    z += recipe.size + recipe.spacing;
                }
                x += parcel_width + recipe.spacing;
            }
        }
    }
    for landmark in &source.landmarks {
        let mut center = landmark.position;
        center[1] += landmark.size[1] / 2.0;
        collider(
            &mut colliders,
            ColliderRole::Landmark,
            center,
            landmark.size.map(|n| n / 2.0),
        );
    }
    let collision = CollisionPackage {
        schema_version: 1,
        colliders,
    };
    for c in &collision.colliders {
        if let Some(identity) = identities.get_mut(&c.id) {
            identity.neighbours = collision
                .colliders
                .iter()
                .filter(|other| {
                    other.role == ColliderRole::Building
                        && other.id != c.id
                        && [0, 2].iter().all(|&axis| {
                            (other.center[axis] - c.center[axis]).abs()
                                <= other.half_extents[axis] + c.half_extents[axis] + 0.8
                        })
                })
                .map(|other| Bounds {
                    min: std::array::from_fn(|axis| other.center[axis] - other.half_extents[axis]),
                    max: std::array::from_fn(|axis| other.center[axis] + other.half_extents[axis]),
                })
                .collect();
        }
    }
    let mut visuals = Vec::new();
    let palette = ["#687f87", "#83958e", "#9c8d80", "#75858d"];
    let mut landmark_index = 0;
    for c in &collision.colliders {
        let (role, color) = match c.role {
            ColliderRole::Ground => (VisualRole::Ground, "#647763"),
            ColliderRole::Boundary => (VisualRole::Boundary, "#e0a348"),
            ColliderRole::Building => (
                VisualRole::Building,
                source
                    .districts
                    .iter()
                    .find(|d| {
                        c.center[0] >= d.rect.min[0]
                            && c.center[0] < d.rect.max[0]
                            && c.center[2] >= d.rect.min[1]
                            && c.center[2] < d.rect.max[1]
                    })
                    .map_or(palette[c.id as usize % palette.len()], |d| d.color.as_str()),
            ),
            ColliderRole::Landmark => {
                let color = &source.landmarks[landmark_index].color;
                landmark_index += 1;
                (VisualRole::Landmark, color.as_str())
            }
        };
        let mut shell = primitive(
            if identities.contains_key(&c.id) {
                format!("building_{}_{}", c.center[0], c.center[2])
            } else {
                format!("solid_{}", c.id)
            },
            role,
            c.center,
            c.half_extents,
            color,
            Some(c.id),
        );
        shell.building = identities.get(&c.id).cloned();
        if let Some(identity) = &shell.building {
            let colors: &[&str] = match identity.architecture.style {
                ArchitectureStyle::Quay => &["#a77861", "#bdac88", "#8d6860", "#b39a7d"],
                ArchitectureStyle::Industrial => &["#827a69", "#756b62", "#8b8070", "#777e79"],
                ArchitectureStyle::Residential => &["#b3b6a0", "#a5b3ad", "#b5b0a6"],
                ArchitectureStyle::Civic => &["#c9c4ae", "#bcbfb5", "#d0c6b4"],
                ArchitectureStyle::Market => &["#c49b72", "#af826c", "#b6ac86", "#91a49b"],
                ArchitectureStyle::Dockside => &["#6d8994", "#81989c", "#89958c"],
                ArchitectureStyle::Modern => &["#687f87", "#83958e"],
            };
            let h = shell.id.bytes().fold(2166136261u32, |h, c| {
                (h ^ u32::from(c)).wrapping_mul(16777619)
            });
            shell.color = colors[h as usize % colors.len()].into();
        }
        visuals.push(shell);
    }
    let road_half = source.roads.carriageway_width / 2.0;
    let pavement_half = road_half + source.roads.sidewalk_width;
    for (axis, lines) in [(0, &source.roads.x), (2, &source.roads.z)] {
        let other = 2 - axis;
        for (i, &line) in lines.iter().enumerate() {
            let mut center = [center[0], 0.01, center[2]];
            center[axis] = line;
            let mut half = [
                (b.max[0] - b.min[0]) / 2.0,
                0.01,
                (b.max[2] - b.min[2]) / 2.0,
            ];
            half[axis] = pavement_half;
            visuals.push(primitive(
                format!("sidewalk_{axis}_{i}"),
                VisualRole::Sidewalk,
                center,
                half,
                "#bec5b6",
                None,
            ));
            center[1] = 0.025;
            half[axis] = road_half;
            half[1] = 0.005;
            visuals.push(primitive(
                format!("road_{axis}_{i}"),
                VisualRole::Road,
                center,
                half,
                "#35464e",
                None,
            ));
            // Dashed center lines, clipped out of cross-street intersections.
            let mut along = b.min[other] + 4.0;
            while along < b.max[other] - 4.0 {
                let crosses = if axis == 0 {
                    &source.roads.z
                } else {
                    &source.roads.x
                };
                if !crosses
                    .iter()
                    .any(|n| (along - n).abs() < pavement_half + 2.0)
                {
                    center[other] = along;
                    center[1] = 0.04;
                    half[axis] = 0.12;
                    half[other] = 2.0;
                    half[1] = 0.005;
                    visuals.push(primitive(
                        format!("mark_{axis}_{i}_{along}"),
                        VisualRole::Marking,
                        center,
                        half,
                        "#e8deae",
                        None,
                    ));
                }
                along += 8.0;
            }
        }
    }
    if full {
        visuals.extend(crate::vegetation::trees(source, &collision));
    }
    let mut files = BTreeMap::new();
    let mut chunks = Vec::new();
    let xs = if full { 0..=5 } else { 1..=2 };
    let zs = if full { 0..=5 } else { 3..=4 };
    for cx in xs {
        for cz in zs.clone() {
            let id = format!("chunk_{cx}_{cz}");
            let min = [
                -384.0 + 128.0 * f64::from(cx),
                b.min[1],
                -384.0 + 128.0 * f64::from(cz),
            ];
            let max = [min[0] + 128.0, b.max[1], min[2] + 128.0];
            let mut clipped = Vec::new();
            for p in &visuals {
                let lo = [0, 1, 2].map(|a| (p.center[a] - p.half_extents[a]).max(min[a]));
                let hi = [0, 1, 2].map(|a| (p.center[a] + p.half_extents[a]).min(max[a]));
                if (0..3).all(|a| hi[a] > lo[a]) {
                    let mut p = p.clone();
                    p.center = [0, 1, 2].map(|a| (lo[a] + hi[a]) / 2.0);
                    p.half_extents = [0, 1, 2].map(|a| (hi[a] - lo[a]) / 2.0);
                    clipped.push(p);
                }
            }
            let visual_url = format!("{id}.json");
            let sha256 = asset(
                &mut files,
                &visual_url,
                &VisualChunk {
                    schema_version: 1,
                    id: id.clone(),
                    primitives: clipped,
                },
            )?;
            chunks.push(ChunkAsset {
                id,
                cx,
                cz,
                visual_url,
                sha256,
                lod_url: None,
                bounds: Bounds { min, max },
            });
        }
    }
    let mut vehicle_visuals = Vec::new();
    for v in &source.vehicle_templates {
        let id = v.visual_asset_id.clone();
        let url = format!("{id}.json");
        let visual = VisualChunk {
            schema_version: 1,
            id: id.clone(),
            primitives: vec![
                primitive(
                    "body".into(),
                    VisualRole::Vehicle,
                    [0.0, v.dimensions[1] / 2.0, 0.0],
                    v.dimensions.map(|n| n / 2.0),
                    "#69bdd4",
                    None,
                ),
                primitive(
                    "windows".into(),
                    VisualRole::Vehicle,
                    [0.0, v.dimensions[1] - 0.25, -0.3],
                    [v.dimensions[0] / 2.0 - 0.12, 0.25, v.dimensions[2] / 4.0],
                    "#193440",
                    None,
                ),
            ],
        };
        let sha256 = asset(&mut files, &url, &visual)?;
        vehicle_visuals.push(VisualAsset { id, url, sha256 });
    }
    let mut backdrop = VisualChunk {
        schema_version: 1,
        id: "backdrop".into(),
        primitives: visuals
            .iter()
            .filter(|p| {
                matches!(
                    p.role,
                    VisualRole::Vegetation
                        | VisualRole::Building
                        | VisualRole::Landmark
                        | VisualRole::Ground
                        | VisualRole::Boundary
                )
            })
            .cloned()
            .collect(),
    };
    if full {
        backdrop.primitives.push(primitive(
            "ocean".into(),
            VisualRole::Ground,
            [0.0, -2.0, 0.0],
            [800.0, 0.1, 800.0],
            "#426d82",
            None,
        ));
        backdrop.primitives.push(primitive(
            "heights_distant_hill".into(),
            VisualRole::Landmark,
            [-216.0, 10.0, -420.0],
            [60.0, 20.0, 24.0],
            "#65825e",
            None,
        ));
    }
    let minimap = VisualChunk {
        schema_version: 1,
        id: "minimap".into(),
        primitives: visuals
            .iter()
            .filter(|p| p.role != VisualRole::Marking)
            .cloned()
            .map(|mut p| {
                p.center[1] = 0.001;
                p.half_extents[1] = 0.001;
                p.collider_id = None;
                p
            })
            .collect(),
    };
    let backdrop_hash = asset(&mut files, "backdrop.json", &backdrop)?;
    let minimap_hash = asset(&mut files, "minimap.json", &minimap)?;
    let collision_sha256 = asset(&mut files, "collision.json", &collision)?;
    let sim_config_hash = asset(&mut files, "sim-config.json", &source.sim_config)?;
    let debug = DebugOverlay {
        schema_version: 1,
        colliders: collision.colliders.clone(),
        reservations: source.reservations.clone(),
        accesses: source.accesses.clone(),
    };
    let debug_sha256 = asset(&mut files, "debug.json", &debug)?;
    let mut manifest = WorldManifest {
        schema_version: 1,
        world_asset_id: source.world_asset_id.clone(),
        content_hash: String::new(),
        sim_config_hash,
        sim_config_url: "sim-config.json".into(),
        bounds: b.clone(),
        roads: source.roads.clone(),
        collision_url: "collision.json".into(),
        collision_sha256,
        debug_url: "debug.json".into(),
        debug_sha256,
        chunks,
        districts: source.districts.clone(),
        backdrop: VisualAsset {
            id: "backdrop".into(),
            url: "backdrop.json".into(),
            sha256: backdrop_hash,
        },
        minimap: VisualAsset {
            id: "minimap".into(),
            url: "minimap.json".into(),
            sha256: minimap_hash,
        },
        road_graph: graph(source, false)?,
        pedestrian_graph: graph(source, true)?,
        reservations: source.reservations.clone(),
        accesses: source.accesses.clone(),
        spawns: source.spawns.clone(),
        parking: source.parking.clone(),
        zones: source.zones.clone(),
        mission_templates: source.mission_templates.clone(),
        vehicle_templates: source.vehicle_templates.clone(),
        vehicle_visuals,
    };
    manifest.content_hash = canonical::manifest_hash(&manifest)?;
    validate::package(&manifest, &collision, &files)?;
    asset(&mut files, "manifest.json", &manifest)?;
    Ok(BakedPackage { manifest, files })
}

fn graph(source: &Authoring, pedestrian: bool) -> Result<RoadGraph> {
    let offset = if pedestrian {
        source.roads.carriageway_width / 2.0 + source.roads.sidewalk_width / 2.0
    } else {
        source.roads.carriageway_width / 4.0
    };
    let mut graph = RoadGraph {
        nodes: Vec::new(),
        edges: Vec::new(),
        accesses: Vec::new(),
    };
    let xs: Vec<_> = source
        .roads
        .x
        .iter()
        .flat_map(|x| [x - offset, x + offset])
        .collect();
    let zs: Vec<_> = source
        .roads
        .z
        .iter()
        .flat_map(|z| [z - offset, z + offset])
        .collect();
    let width = if pedestrian { 1.5 } else { 3.0 };
    let mut node = |position| {
        let id = graph.nodes.len() as u32 + 1;
        graph.nodes.push(GraphNode { id, position });
        id
    };
    let mut grid = Vec::new();
    for &x in &xs {
        let mut row = Vec::new();
        for &z in &zs {
            row.push(node([x, 0.0, z]));
        }
        grid.push(row);
    }
    for (xi, &x) in xs.iter().enumerate() {
        for (zi, &z) in zs.iter().enumerate() {
            if xi + 1 < xs.len() {
                let (a, b) = (grid[xi][zi], grid[xi + 1][zi]);
                let (from, to) = if zi % 2 == 1 { (a, b) } else { (b, a) };
                edge(
                    &mut graph,
                    from,
                    to,
                    width,
                    if pedestrian { 1.4 } else { 12.0 },
                    if pedestrian && xi % 2 == 0 {
                        EdgeKind::Crossing
                    } else if pedestrian {
                        EdgeKind::Sidewalk
                    } else {
                        EdgeKind::Lane
                    },
                );
                if pedestrian {
                    let e = graph.edges.last().expect("edge").clone();
                    edge(&mut graph, e.to, e.from, e.width, e.speed_limit, e.kind);
                }
            }
            if zi + 1 < zs.len() {
                let (a, b) = (grid[xi][zi], grid[xi][zi + 1]);
                let (from, to) = if xi % 2 == 0 { (a, b) } else { (b, a) };
                edge(
                    &mut graph,
                    from,
                    to,
                    width,
                    if pedestrian { 1.4 } else { 12.0 },
                    if pedestrian && zi % 2 == 0 {
                        EdgeKind::Crossing
                    } else if pedestrian {
                        EdgeKind::Sidewalk
                    } else {
                        EdgeKind::Lane
                    },
                );
                if pedestrian {
                    let e = graph.edges.last().expect("edge").clone();
                    edge(&mut graph, e.to, e.from, e.width, e.speed_limit, e.kind);
                }
            }
            let _ = (x, z);
        }
    }
    for access in &source.accesses {
        let axis = if access.road_axis == Axis::X { 0 } else { 2 };
        let mut points = [access.position; 2];
        points[0][axis] = access.road_center - offset;
        points[1][axis] = access.road_center + offset;
        if (points[0][axis] - access.position[axis]).abs()
            > (points[1][axis] - access.position[axis]).abs()
        {
            points.swap(0, 1);
        }
        let start = split_node(&mut graph, access.position);
        let near = split_node(&mut graph, points[0]);
        let far = split_node(&mut graph, points[1]);
        for (from, to) in [(start, near), (near, far)] {
            edge(
                &mut graph,
                from,
                to,
                if pedestrian { 1.5 } else { 5.0 },
                if pedestrian { 1.4 } else { 4.0 },
                EdgeKind::Driveway,
            );
            edge(
                &mut graph,
                to,
                from,
                if pedestrian { 1.5 } else { 5.0 },
                if pedestrian { 1.4 } else { 4.0 },
                EdgeKind::Driveway,
            );
        }
        graph.accesses.push(GraphAccess {
            access_id: access.id.clone(),
            node_id: start,
        });
    }
    graph.edges.sort_by_key(|e| (e.from, e.to));
    graph.edges.dedup_by_key(|e| (e.from, e.to));
    Ok(graph)
}
fn edge(graph: &mut RoadGraph, from: u32, to: u32, width: f64, speed_limit: f64, kind: EdgeKind) {
    if from != to {
        graph.edges.push(GraphEdge {
            from,
            to,
            width,
            speed_limit,
            kind,
        });
    }
}
fn split_node(graph: &mut RoadGraph, position: Vec3) -> u32 {
    let id = if let Some(n) = graph.nodes.iter().find(|n| n.position == position) {
        n.id
    } else {
        let id = graph.nodes.len() as u32 + 1;
        graph.nodes.push(GraphNode { id, position });
        id
    };
    let old = std::mem::take(&mut graph.edges);
    for e in old {
        let a = graph.nodes[(e.from - 1) as usize].position;
        let b = graph.nodes[(e.to - 1) as usize].position;
        let axis = if a[0] == b[0] { 2 } else { 0 };
        let other = 2 - axis;
        if position[other] == a[other]
            && position[axis] > a[axis].min(b[axis])
            && position[axis] < a[axis].max(b[axis])
        {
            edge(graph, e.from, id, e.width, e.speed_limit, e.kind);
            edge(graph, id, e.to, e.width, e.speed_limit, e.kind);
        } else {
            graph.edges.push(e);
        }
    }
    id
}
