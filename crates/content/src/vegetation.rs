//! Trees are planted as complete roadside rows or compact park groves.
use crate::types::*;

fn clear(source: &Authoring, collision: &CollisionPackage, x: f64, z: f64) -> bool {
    let radius = 3.5;
    // Keep each model wholly inside one streaming chunk.
    if [x, z].iter().any(|v| {
        let local = (v + 384.0) % 128.0;
        local < radius || local > 128.0 - radius
    }) {
        return false;
    }
    if x - radius < source.bounds.min[0] + 2.0
        || x + radius > source.bounds.max[0] - 2.0
        || z - radius < source.bounds.min[2] + 2.0
        || z + radius > source.bounds.max[2] - 2.0
    {
        return false;
    }
    let road_clearance =
        source.roads.carriageway_width / 2.0 + source.roads.sidewalk_width + radius + 1.0;
    if source
        .roads
        .x
        .iter()
        .any(|v| (x - v).abs() < road_clearance)
        || source
            .roads
            .z
            .iter()
            .any(|v| (z - v).abs() < road_clearance)
    {
        return false;
    }
    let envelope = Rect {
        min: [x - radius, z - radius],
        max: [x + radius, z + radius],
    };
    if source
        .reservations
        .iter()
        .any(|r| crate::validate::overlap(&envelope, &r.rect))
    {
        return false;
    }
    if collision
        .colliders
        .iter()
        .filter(|c| matches!(c.role, ColliderRole::Building | ColliderRole::Landmark))
        .any(|c| {
            (x - c.center[0]).abs() < c.half_extents[0] + radius + 1.0
                && (z - c.center[2]).abs() < c.half_extents[2] + radius + 1.0
        })
    {
        return false;
    }
    true
}

fn model(
    result: &mut Vec<Primitive>,
    collection: &str,
    group: usize,
    member: usize,
    x: f64,
    z: f64,
    kind: u32,
    scale: f64,
) {
    let mut part = |name: &str, offset: Vec3, half: Vec3, color: &str| {
        result.push(Primitive {
            id: format!("tree_{collection}_{group}_{member}_{kind}_{name}"),
            role: VisualRole::Vegetation,
            center: [
                x + offset[0] * scale,
                offset[1] * scale,
                z + offset[2] * scale,
            ],
            half_extents: half.map(|v| v * scale),
            color: color.into(),
            collider_id: None,
            building: None,
        });
    };
    // Broadleaf, narrow poplar and tiered evergreen: small shared colour palette.
    part("trunk", [0.0, 1.6, 0.0], [0.24, 1.6, 0.24], "#72513b");
    match kind {
        0 => {
            part("crown", [0.0, 4.4, 0.0], [1.9, 1.5, 1.9], "#527b45");
            part("west", [-1.3, 3.6, 0.2], [1.1, 0.9, 1.3], "#426b3e");
            part("east", [1.2, 4.0, -0.3], [1.1, 1.1, 1.2], "#668b4c");
            part("top", [0.2, 5.5, 0.2], [1.1, 0.7, 1.2], "#668b4c");
        }
        1 => {
            part("lower", [0.0, 3.8, 0.0], [1.0, 1.5, 1.0], "#426b3e");
            part("crown", [0.0, 5.4, 0.0], [0.85, 1.4, 0.85], "#527b45");
            part("tip", [0.0, 6.7, 0.0], [0.5, 0.7, 0.5], "#668b4c");
        }
        _ => {
            part("lower", [0.0, 2.8, 0.0], [2.0, 0.7, 2.0], "#345f4b");
            part("middle", [0.0, 4.0, 0.0], [1.5, 0.7, 1.5], "#426b3e");
            part("upper", [0.0, 5.2, 0.0], [1.0, 0.7, 1.0], "#527b45");
            part("tip", [0.0, 6.2, 0.0], [0.45, 0.5, 0.45], "#668b4c");
        }
    }
}

pub fn trees(source: &Authoring, collision: &CollisionPackage) -> Vec<Primitive> {
    let mut result = Vec::new();
    let mut planted: Vec<[f64; 2]> = Vec::new();
    let mut group = 0;
    let available = |x: f64, z: f64, planted: &[[f64; 2]]| {
        clear(source, collision, x, z) && planted.iter().all(|p| (p[0] - x).hypot(p[1] - z) >= 8.0)
    };
    // Groves are accepted as complete five-tree groups, never isolated survivors.
    // Their irregular outline still reads as a small planted space.
    for ix in 0..94 {
        for iz in 0..94 {
            let seed = source.buildings.seed
                ^ (ix as u32).wrapping_mul(73856093)
                ^ (iz as u32).wrapping_mul(19349663);
            if seed % 7 != 0 || group >= 12 {
                continue;
            }
            let x = -372.0 + f64::from((ix * 37) % 94) * 8.0;
            let z = -372.0 + f64::from((iz * 53) % 94) * 8.0;
            let points = [
                [x - 4.0, z - 4.0],
                [x + 4.0, z - 4.0],
                [x - 4.0, z + 4.0],
                [x + 4.0, z + 4.0],
                [x, z + 12.0],
            ];
            if !points.iter().all(|p| available(p[0], p[1], &planted)) {
                continue;
            }
            for (member, p) in points.iter().enumerate() {
                model(
                    &mut result,
                    "park",
                    group,
                    member,
                    p[0],
                    p[1],
                    (seed + member as u32) % 3,
                    0.9 + (member % 3) as f64 * 0.1,
                );
            }
            planted.extend(points);
            group += 1;
        }
    }
    // Each row is parallel to a road, with consistent species, height and 10m spacing.
    // Obstructions split runs; runs shorter than three trees are discarded.
    for (axis, roads, cross) in [
        (0, &source.roads.x, &source.roads.z),
        (1, &source.roads.z, &source.roads.x),
    ] {
        for &road in roads {
            for side in [-1.0, 1.0] {
                for block in cross.windows(2) {
                    let mut run = Vec::new();
                    let mut along = block[0] + 18.0;
                    while along <= block[1] - 18.0 + 10.0 {
                        let mut point = [0.0; 2];
                        point[axis] = road + side * 18.0;
                        point[1 - axis] = along;
                        let valid =
                            along <= block[1] - 18.0 && available(point[0], point[1], &planted);
                        if valid {
                            run.push(point);
                        }
                        if !valid || run.len() == 8 {
                            if run.len() >= 3 && planted.len() + run.len() <= 240 {
                                for (member, p) in run.iter().enumerate() {
                                    model(
                                        &mut result,
                                        "boulevard",
                                        group,
                                        member,
                                        p[0],
                                        p[1],
                                        (group % 2) as u32,
                                        1.0,
                                    );
                                }
                                planted.extend(run.iter().copied());
                                group += 1;
                            }
                            run.clear();
                        }
                        along += 10.0;
                    }
                }
            }
        }
    }
    result
}
