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
    for xpair in source.roads.x.windows(2) {
        for zpair in source.roads.z.windows(2) {
            let mut x = ((xpair[0] + recipe.setback) / recipe.grid).ceil() * recipe.grid;
            while x + recipe.size <= xpair[1] - recipe.setback {
                let mut z = ((zpair[0] + recipe.setback) / recipe.grid).ceil() * recipe.grid;
                while z + recipe.size <= zpair[1] - recipe.setback {
                    let rect = Rect {
                        min: [x, z],
                        max: [x + recipe.size, z + recipe.size],
                    };
                    if !source
                        .reservations
                        .iter()
                        .any(|r| validate::overlap(&rect, &r.rect))
                    {
                        rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
                        let height = 8.0 + 4.0 * f64::from(rng % 4);
                        collider(
                            &mut colliders,
                            ColliderRole::Building,
                            [x + recipe.size / 2.0, height / 2.0, z + recipe.size / 2.0],
                            [recipe.size / 2.0, height / 2.0, recipe.size / 2.0],
                        );
                    }
                    z += recipe.size + recipe.spacing;
                }
                x += recipe.size + recipe.spacing;
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
        visuals.push(primitive(
            format!("solid_{}", c.id),
            role,
            c.center,
            c.half_extents,
            color,
            Some(c.id),
        ));
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
                    VisualRole::Building
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
