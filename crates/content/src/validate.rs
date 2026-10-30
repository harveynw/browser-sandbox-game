//! Bake-time geometry/reference checks for axis-aligned collision boxes.
use crate::{Result, canonical, types::*};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
const EPS: f64 = 0.000001;
fn require(ok: bool, message: impl Into<String>) -> Result<()> {
    if ok { Ok(()) } else { Err(message.into()) }
}
pub fn overlap(a: &Rect, b: &Rect) -> bool {
    (0..2).all(|i| a.min[i] < b.max[i] - EPS && a.max[i] > b.min[i] + EPS)
}
pub fn footprint(c: &StaticCollider) -> Rect {
    Rect {
        min: [
            c.center[0] - c.half_extents[0],
            c.center[2] - c.half_extents[2],
        ],
        max: [
            c.center[0] + c.half_extents[0],
            c.center[2] + c.half_extents[2],
        ],
    }
}
fn around(p: Vec3, half: [f64; 2]) -> Rect {
    Rect {
        min: [p[0] - half[0], p[2] - half[1]],
        max: [p[0] + half[0], p[2] + half[1]],
    }
}
fn contains(a: &Rect, b: &Rect) -> bool {
    (0..2).all(|i| b.min[i] >= a.min[i] - EPS && b.max[i] <= a.max[i] + EPS)
}
fn bounds_rect(b: &Bounds) -> Rect {
    Rect {
        min: [b.min[0], b.min[2]],
        max: [b.max[0], b.max[2]],
    }
}
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.:-".contains(&b))
}
fn unique<'a>(items: impl Iterator<Item = &'a str>, label: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for item in items {
        require(
            id(item) && seen.insert(item),
            format!("invalid/duplicate {label} ID: {item}"),
        )?;
    }
    Ok(())
}
fn finite(p: Vec3) -> bool {
    p.iter().all(|n| n.is_finite())
}
fn anchor(
    position: Vec3,
    reservation: &str,
    access: &str,
    radius: f64,
    source: &Authoring,
) -> Result<()> {
    require(
        finite(position) && position[1] == 0.0,
        "anchor must be finite at ground Y=0",
    )?;
    let r = source
        .reservations
        .iter()
        .find(|r| r.id == reservation)
        .ok_or_else(|| format!("unknown reservation {reservation}"))?;
    require(
        r.clear && contains(&r.rect, &around(position, [radius, radius])),
        format!("anchor outside clear reservation {reservation}"),
    )?;
    let a = source
        .accesses
        .iter()
        .find(|a| a.id == access)
        .ok_or_else(|| format!("unknown access {access}"))?;
    require(
        a.position == position,
        format!("anchor/access mismatch {access}"),
    )
}
pub fn authoring(a: &Authoring) -> Result<()> {
    // Authoring contains no optional fields. Null also catches Serde's non-finite -> null encoding.
    let value = serde_json::to_value(a).map_err(|e| e.to_string())?;
    fn has_null(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Null => true,
            serde_json::Value::Array(v) => v.iter().any(has_null),
            serde_json::Value::Object(v) => v.values().any(has_null),
            _ => false,
        }
    }
    require(!has_null(&value), "null/non-finite authored value")?;
    require(
        canonical::bytes(a)?.len() <= 2 * 1024 * 1024,
        "authoring exceeds 2 MiB",
    )?;
    require(
        a.schema_version == 1 && a.sim_config.schema_version == 1 && a.sim_config.tick_rate == 30,
        "unsupported content/config version",
    )?;
    require(id(&a.world_asset_id), "invalid world asset ID")?;
    let full = a.bounds.min == [-384.0, -2.0, -384.0] && a.bounds.max == [384.0, 64.0, 384.0];
    require(
        full || (a.bounds.min == [-256.0, -2.0, 0.0] && a.bounds.max == [0.0, 64.0, 256.0]),
        "unsupported city/slice bounds",
    )?;
    require(
        a.districts.len() == if full { 6 } else { 0 },
        "district count mismatch",
    )?;
    unique(a.districts.iter().map(|d| d.id.as_str()), "district")?;
    for (i, d) in a.districts.iter().enumerate() {
        require(
            contains(&bounds_rect(&a.bounds), &d.rect)
                && d.rect.max[0] - d.rect.min[0] == 256.0
                && d.rect.max[1] - d.rect.min[1] == 384.0,
            "district dimensions",
        )?;
        for other in a.districts.iter().skip(i + 1) {
            require(!overlap(&d.rect, &other.rect), "district overlap")?;
        }
    }
    require(
        a.roads.x.len() == if full { 7 } else { 3 } && a.roads.z.len() == if full { 7 } else { 3 },
        "slice requires three road lines on each axis",
    )?;
    if full {
        require(
            a.roads.x == [-352.0, -256.0, -128.0, 0.0, 128.0, 256.0, 352.0]
                && a.roads.z == a.roads.x,
            "full city grid/ring positions",
        )?;
    }
    require(
        a.roads.carriageway_width == 12.0 && a.roads.sidewalk_width == 3.0,
        "slice requires 12m roads and 3m sidewalks",
    )?;
    for (lines, min, max) in [
        (&a.roads.x, a.bounds.min[0], a.bounds.max[0]),
        (&a.roads.z, a.bounds.min[2], a.bounds.max[2]),
    ] {
        require(
            lines.windows(2).all(|p| p[1] - p[0] >= 40.0)
                && lines[0] - 9.0 >= min + 1.0
                && lines[lines.len() - 1] + 9.0 <= max - 1.0,
            "road envelopes outside boundary or too close",
        )?;
    }
    let r = &a.buildings;
    require(
        r.grid == 4.0
            && r.size >= 4.0
            && r.size <= 40.0
            && r.spacing >= 3.0
            && r.spacing <= 20.0
            && r.setback >= 12.0
            && r.setback <= 20.0
            && (r.size + r.spacing) % r.grid == 0.0,
        "invalid building grid/setback/separation",
    )?;
    require(
        a.reservations.len() <= 128
            && a.accesses.len() <= 64
            && a.landmarks.len() <= 16
            && !a.spawns.is_empty()
            && !a.parking.is_empty()
            && !a.mission_templates.is_empty(),
        "slice content counts invalid",
    )?;
    unique(a.reservations.iter().map(|r| r.id.as_str()), "reservation")?;
    unique(a.accesses.iter().map(|r| r.id.as_str()), "access")?;
    unique(a.spawns.iter().map(|r| r.id.as_str()), "spawn")?;
    unique(a.parking.iter().map(|r| r.id.as_str()), "parking")?;
    unique(a.zones.iter().map(|r| r.id.as_str()), "zone")?;
    unique(
        a.vehicle_templates.iter().map(|r| r.id.as_str()),
        "vehicle template",
    )?;
    unique(
        a.mission_templates.iter().map(|r| r.id.as_str()),
        "mission template",
    )?;
    unique(a.landmarks.iter().map(|r| r.id.as_str()), "landmark")?;
    let world = bounds_rect(&a.bounds);
    for r in &a.reservations {
        require(
            (0..2).all(|i| r.rect.max[i] > r.rect.min[i]) && contains(&world, &r.rect),
            format!("invalid reservation {}", r.id),
        )?;
    }
    for access in &a.accesses {
        let lines = if access.road_axis == Axis::X {
            &a.roads.x
        } else {
            &a.roads.z
        };
        require(
            lines.contains(&access.road_center)
                && finite(access.position)
                && access.position[1] == 0.0,
            "invalid road access",
        )?;
        let r = a
            .reservations
            .iter()
            .find(|r| r.id == access.driveway_id)
            .ok_or("missing driveway")?;
        let axis = if access.road_axis == Axis::X { 0 } else { 1 };
        require(
            r.clear
                && r.rect.min[axis] <= access.road_center
                && r.rect.max[axis] >= access.road_center
                && r.rect.max[1 - axis] - r.rect.min[1 - axis] >= 5.0,
            "driveway must be clear, at least 5m wide and reach its road",
        )?;
    }
    let c = &a.sim_config.character;
    require(
        c.height > 2.0 * c.radius
            && c.radius > 0.0
            && c.height <= 3.0
            && c.walk_speed > 0.0
            && c.sprint_speed >= c.walk_speed
            && c.ground_acceleration > 0.0
            && c.air_acceleration > 0.0
            && c.gravity > 0.0
            && c.jump_speed > 0.0
            && c.max_fall_speed > 0.0
            && c.skin > 0.0
            && c.skin < c.radius
            && c.step_height >= 0.0
            && c.step_height < c.height
            && c.ground_snap >= 0.0
            && c.slope_limit_degrees > 0.0
            && c.slope_limit_degrees < 90.0
            && c.turn_speed > 0.0
            && c.jump_cooldown_ticks > 0,
        "invalid character constants",
    )?;
    let v = &a.sim_config.vehicle;
    require(
        v.rolling_resistance > 0.0
            && v.handbrake > 0.0
            && v.steering_slow >= v.steering_fast
            && v.steering_fast > 0.0
            && v.steering_slow < 1.0
            && v.steering_rate > 0.0
            && v.max_yaw_rate > 0.0
            && v.translation_substep > 0.0
            && v.yaw_substep > 0.0
            && v.max_substeps > 0
            && v.max_substeps <= 32,
        "invalid vehicle constants",
    )?;
    for v in &a.vehicle_templates {
        require(
            id(&v.visual_asset_id)
                && v.dimensions.iter().all(|n| *n > 0.0)
                && v.top_speed > 0.0
                && v.reverse_speed > 0.0
                && v.acceleration > 0.0
                && v.brake > 0.0
                && v.wheelbase > 0.0
                && v.wheelbase < v.dimensions[2]
                && v.health > 0
                && v.health <= 160
                && v.exit_offsets.len() == 3,
            "invalid vehicle template",
        )?;
    }
    for s in &a.spawns {
        require(
            s.yaw == 0.0 && s.safe_radius >= c.radius + c.skin,
            "invalid slice spawn clearance",
        )?;
        anchor(
            s.position,
            &s.reservation_id,
            &s.access_id,
            s.safe_radius,
            a,
        )?;
    }
    for p in &a.parking {
        require(p.yaw == 0.0, "parking must be axis-aligned")?;
        anchor(p.position, &p.reservation_id, &p.access_id, 0.0, a)?;
        require(
            a.vehicle_templates.iter().any(|v| v.id == p.template_id),
            "missing parking vehicle template",
        )?;
    }
    for z in &a.zones {
        require(z.radius > 0.0 && z.radius <= 10.0, "invalid zone radius")?;
        anchor(z.center, &z.reservation_id, &z.access_id, z.radius, a)?;
        for id in &z.allowed_templates {
            require(
                z.kind == ZoneKind::Terminal
                    && a.mission_templates.iter().any(|m| {
                        &m.id == id
                            && (m.terminal_zone_id == z.id
                                || matches!(m.objective, MissionObjective::Recovery { .. }))
                    }),
                "terminal references incompatible mission",
            )?;
        }
    }
    for m in &a.mission_templates {
        let terminal = a
            .zones
            .iter()
            .find(|z| z.id == m.terminal_zone_id)
            .ok_or("missing mission terminal")?;
        require(
            terminal.kind == ZoneKind::Terminal
                && terminal.allowed_templates.contains(&m.id)
                && m.duration_ticks > 0
                && m.reward_credits <= 10000,
            "invalid mission start/reward/duration",
        )?;
        require(m.variants.len() <= 16, "too many mission variants")?;
        let mut names = BTreeSet::new();
        for variant in &m.variants {
            require(
                !variant.name.trim().is_empty()
                    && variant.name.len() <= 64
                    && names.insert(&variant.name)
                    && variant.duration_ticks > 0
                    && variant.duration_ticks <= 30 * 3600
                    && variant.reward_credits <= 10000
                    && std::mem::discriminant(&variant.objective)
                        == std::mem::discriminant(&m.objective),
                "invalid mission variant",
            )?;
        }
        for objective in
            std::iter::once(&m.objective).chain(m.variants.iter().map(|v| &v.objective))
        {
            match objective {
                MissionObjective::Courier {
                    destination_zone_id,
                    dwell_ticks,
                    max_speed,
                } => require(
                    a.zones
                        .iter()
                        .any(|z| &z.id == destination_zone_id && z.kind == ZoneKind::Delivery)
                        && *dwell_ticks > 0
                        && *max_speed > 0.0,
                    "invalid courier destination/dwell",
                )?,
                MissionObjective::TimeTrial {
                    checkpoints,
                    gate_radius,
                } => {
                    require(
                        full && (3..=32).contains(&checkpoints.len())
                            && (checkpoints[0][0] - terminal.center[0])
                                .hypot(checkpoints[0][2] - terminal.center[2])
                                <= *gate_radius
                            && checkpoints.first() == checkpoints.last()
                            && *gate_radius == 8.0,
                        "invalid time trial gates",
                    )?;
                    for point in checkpoints {
                        require(
                            finite(*point)
                                && point[1] == 0.0
                                && contains(
                                    &bounds_rect(&a.bounds),
                                    &around(*point, [*gate_radius; 2]),
                                ),
                            "trial gate outside map",
                        )?;
                    }
                }
                MissionObjective::Recovery {
                    parking_ids,
                    garage_zone_id,
                } => {
                    require(
                        full && parking_ids.len() == 8
                            && a.zones
                                .iter()
                                .any(|z| &z.id == garage_zone_id && z.kind == ZoneKind::Garage),
                        "invalid recovery lot/garage",
                    )?;
                    let unique: BTreeSet<_> = parking_ids.iter().collect();
                    require(unique.len() == 8, "duplicate recovery slots")?;
                    for id in parking_ids {
                        require(
                            a.parking.iter().any(|p| &p.id == id && p.mission_reserved),
                            "unknown/unreserved recovery slot",
                        )?;
                    }
                }
            }
        }
    }
    for l in &a.landmarks {
        require(
            l.position[1] == 0.0
                && l.size.iter().all(|n| *n > 0.0)
                && l.color.len() == 7
                && l.color.starts_with('#')
                && l.color[1..].bytes().all(|b| b.is_ascii_hexdigit()),
            "invalid landmark",
        )?;
    }
    Ok(())
}
fn clear(
    rect: &Rect,
    feet: f64,
    height: f64,
    colliders: &[StaticCollider],
    label: &str,
) -> Result<()> {
    require(
        !colliders.iter().any(|c| {
            c.role != ColliderRole::Ground
                && feet < c.center[1] + c.half_extents[1] - EPS
                && feet + height > c.center[1] - c.half_extents[1] + EPS
                && overlap(rect, &footprint(c))
        }),
        format!("{label} overlaps solid collider"),
    )
}
pub fn package(
    m: &WorldManifest,
    c: &CollisionPackage,
    files: &BTreeMap<String, Vec<u8>>,
) -> Result<()> {
    require(
        m.content_hash == canonical::manifest_hash(m)?,
        "manifest content hash mismatch",
    )?;
    require(
        c.schema_version == 1 && c.colliders.len() <= 4096,
        "invalid collision version/count",
    )?;
    let mut seen = BTreeSet::new();
    for collider in &c.colliders {
        require(
            collider.id > 0
                && seen.insert(collider.id)
                && finite(collider.center)
                && finite(collider.half_extents)
                && collider.half_extents.iter().all(|n| *n > 0.0)
                && collider.yaw == 0.0
                && (collider.flags == 1 || collider.flags == 3),
            "invalid/duplicate/rotated collider",
        )?;
        require(
            contains(&bounds_rect(&m.bounds), &footprint(collider))
                && collider.center[1] - collider.half_extents[1] >= m.bounds.min[1]
                && collider.center[1] + collider.half_extents[1] <= m.bounds.max[1],
            "collider outside map",
        )?;
        if matches!(
            collider.role,
            ColliderRole::Building | ColliderRole::Landmark
        ) {
            let rect = footprint(collider);
            for (axis, lines) in [(0, &m.roads.x), (1, &m.roads.z)] {
                for line in lines {
                    require(
                        rect.max[axis] <= line - 9.0 + EPS || rect.min[axis] >= line + 9.0 - EPS,
                        "road corridor blocked by solid",
                    )?;
                }
            }
        }
    }
    require(
        c.colliders
            .iter()
            .filter(|c| c.role == ColliderRole::Ground)
            .count()
            == 1,
        "need one ground collider",
    )?;
    let ground = c
        .colliders
        .iter()
        .find(|c| c.role == ColliderRole::Ground)
        .ok_or("no ground")?;
    require(
        footprint(ground) == bounds_rect(&m.bounds)
            && ground.center[1] + ground.half_extents[1] == 0.0,
        "ground must support the full slice at Y=0",
    )?;
    require(
        c.colliders
            .iter()
            .filter(|c| c.role == ColliderRole::Boundary)
            .count()
            == 4,
        "need four world boundary walls",
    )?;
    for r in m.reservations.iter().filter(|r| r.clear) {
        clear(
            &r.rect,
            0.0,
            64.0,
            &c.colliders,
            &format!("reservation {}", r.id),
        )?;
    }
    let buildings: Vec<_> = c
        .colliders
        .iter()
        .filter(|c| c.role == ColliderRole::Building)
        .collect();
    for (i, a) in buildings.iter().enumerate() {
        for b in buildings.iter().skip(i + 1) {
            let mut rect = footprint(a);
            for axis in 0..2 {
                rect.min[axis] -= 3.0;
                rect.max[axis] += 3.0;
            }
            require(!overlap(&rect, &footprint(b)), "buildings need 3m passage")?;
        }
    }
    let config: SimConfig =
        serde_json::from_slice(files.get(&m.sim_config_url).ok_or("missing sim config")?)
            .map_err(|e| e.to_string())?;
    for s in &m.spawns {
        clear(
            &around(s.position, [s.safe_radius, s.safe_radius]),
            0.0,
            config.character.height,
            &c.colliders,
            &format!("spawn {}", s.id),
        )?;
    }
    for p in &m.parking {
        let v = m
            .vehicle_templates
            .iter()
            .find(|v| v.id == p.template_id)
            .ok_or("parking template")?;
        let rect = around(p.position, [v.dimensions[0] / 2.0, v.dimensions[2] / 2.0]);
        let reservation = m
            .reservations
            .iter()
            .find(|r| r.id == p.reservation_id)
            .ok_or("parking reservation")?;
        require(
            contains(&reservation.rect, &rect),
            "vehicle does not fit parking reservation",
        )?;
        clear(
            &rect,
            0.0,
            v.dimensions[1],
            &c.colliders,
            &format!("parking {}", p.id),
        )?;
        for offset in &v.exit_offsets {
            let pos = [p.position[0] + offset[0], 0.0, p.position[2] + offset[2]];
            clear(
                &around(pos, [config.character.radius + config.character.skin; 2]),
                0.0,
                config.character.height,
                &c.colliders,
                "vehicle exit",
            )?;
        }
    }
    for (i, a) in m.parking.iter().enumerate() {
        for b in m.parking.iter().skip(i + 1) {
            let va = m
                .vehicle_templates
                .iter()
                .find(|v| v.id == a.template_id)
                .ok_or("template")?;
            let vb = m
                .vehicle_templates
                .iter()
                .find(|v| v.id == b.template_id)
                .ok_or("template")?;
            require(
                !overlap(
                    &around(a.position, [va.dimensions[0] / 2.0, va.dimensions[2] / 2.0]),
                    &around(b.position, [vb.dimensions[0] / 2.0, vb.dimensions[2] / 2.0]),
                ),
                "parked vehicles overlap",
            )?;
        }
    }
    graph(&m.road_graph, m, &c.colliders)?;
    graph(&m.pedestrian_graph, m, &c.colliders)?;
    for mission in &m.mission_templates {
        for objective in
            std::iter::once(&mission.objective).chain(mission.variants.iter().map(|v| &v.objective))
        {
            if let MissionObjective::TimeTrial {
                checkpoints,
                gate_radius,
            } = objective
            {
                for point in checkpoints {
                    require(
                        m.road_graph.nodes.iter().any(|n| {
                            (n.position[0] - point[0]).hypot(n.position[2] - point[2])
                                <= *gate_radius
                        }),
                        "trial gate disconnected from road graph",
                    )?;
                }
            }
        }
    }
    for (url, hash) in std::iter::once((&m.collision_url, &m.collision_sha256))
        .chain(std::iter::once((&m.sim_config_url, &m.sim_config_hash)))
        .chain(std::iter::once((&m.debug_url, &m.debug_sha256)))
        .chain([
            (&m.backdrop.url, &m.backdrop.sha256),
            (&m.minimap.url, &m.minimap.sha256),
        ])
        .chain(m.chunks.iter().map(|c| (&c.visual_url, &c.sha256)))
        .chain(m.vehicle_visuals.iter().map(|v| (&v.url, &v.sha256)))
    {
        require(
            !url.contains('/') && !url.contains("..") && url.ends_with(".json"),
            "unsafe asset path",
        )?;
        require(
            canonical::sha256(files.get(url).ok_or("missing package asset")?) == *hash,
            "asset hash mismatch",
        )?;
    }
    let debug: DebugOverlay = serde_json::from_slice(files.get(&m.debug_url).ok_or("debug")?)
        .map_err(|e| e.to_string())?;
    require(
        debug.colliders == c.colliders
            && debug.reservations == m.reservations
            && debug.accesses == m.accesses,
        "debug overlay drift",
    )?;
    require(
        m.chunks.len() == if m.districts.is_empty() { 4 } else { 36 },
        "city/slice chunk count",
    )?;
    let stored: CollisionPackage =
        serde_json::from_slice(files.get(&m.collision_url).ok_or("collision")?)
            .map_err(|e| e.to_string())?;
    require(
        stored == *c,
        "collision bytes differ from validated colliders",
    )?;
    for v in &m.vehicle_templates {
        let asset = m
            .vehicle_visuals
            .iter()
            .find(|a| a.id == v.visual_asset_id)
            .ok_or("missing vehicle visual asset")?;
        let visual: VisualChunk =
            serde_json::from_slice(files.get(&asset.url).ok_or("vehicle visual")?)
                .map_err(|e| e.to_string())?;
        require(visual.id == asset.id, "vehicle visual ID mismatch")?;
        let body = visual
            .primitives
            .iter()
            .find(|p| p.id == "body")
            .ok_or("vehicle body missing")?;
        require(
            body.half_extents == v.dimensions.map(|n| n / 2.0)
                && body.center == [0.0, v.dimensions[1] / 2.0, 0.0],
            "vehicle visual dimensions disagree with template",
        )?;
    }
    let mut volumes: BTreeMap<u32, f64> = BTreeMap::new();
    for chunk in &m.chunks {
        let visual: VisualChunk =
            serde_json::from_slice(files.get(&chunk.visual_url).ok_or("visual")?)
                .map_err(|e| e.to_string())?;
        require(
            visual.id == chunk.id && visual.schema_version == 1,
            "chunk ID/version mismatch",
        )?;
        unique(visual.primitives.iter().map(|p| p.id.as_str()), "primitive")?;
        let mut references = BTreeSet::new();
        for p in visual.primitives {
            require(
                finite(p.center) && p.half_extents.iter().all(|n| n.is_finite() && *n > 0.0),
                "invalid visual primitive",
            )?;
            for axis in 0..3 {
                require(
                    p.center[axis] - p.half_extents[axis] >= chunk.bounds.min[axis] - EPS
                        && p.center[axis] + p.half_extents[axis] <= chunk.bounds.max[axis] + EPS,
                    "visual outside owning chunk",
                )?;
            }
            if let Some(id) = p.collider_id {
                require(references.insert(id), "duplicate collider visual in chunk")?;
                let collider = c
                    .colliders
                    .iter()
                    .find(|c| c.id == id)
                    .ok_or("visual references missing collider")?;
                for axis in 0..3 {
                    require(
                        p.center[axis] - p.half_extents[axis]
                            >= collider.center[axis] - collider.half_extents[axis] - EPS
                            && p.center[axis] + p.half_extents[axis]
                                <= collider.center[axis] + collider.half_extents[axis] + EPS,
                        "visual/collision face mismatch",
                    )?;
                }
                let lo = [0, 1, 2].map(|a| {
                    (collider.center[a] - collider.half_extents[a]).max(chunk.bounds.min[a])
                });
                let hi = [0, 1, 2].map(|a| {
                    (collider.center[a] + collider.half_extents[a]).min(chunk.bounds.max[a])
                });
                require(
                    (0..3).all(|a| {
                        (p.center[a] - (lo[a] + hi[a]) / 2.0).abs() < EPS
                            && (p.half_extents[a] - (hi[a] - lo[a]) / 2.0).abs() < EPS
                    }),
                    "clipped visual/collider face mismatch",
                )?;
                *volumes.entry(id).or_default() += 8.0 * p.half_extents.iter().product::<f64>();
            }
        }
    }
    for collider in &c.colliders {
        require(
            (volumes.get(&collider.id).copied().unwrap_or(0.0)
                - 8.0 * collider.half_extents.iter().product::<f64>())
            .abs()
                < EPS,
            "visual/collision coverage mismatch",
        )?;
    }
    Ok(())
}
fn graph(g: &RoadGraph, m: &WorldManifest, colliders: &[StaticCollider]) -> Result<()> {
    require(
        !g.nodes.is_empty() && g.nodes.len() <= 4096 && g.edges.len() <= 16384,
        "graph counts invalid",
    )?;
    let nodes: BTreeMap<_, _> = g.nodes.iter().map(|n| (n.id, n.position)).collect();
    require(
        nodes.len() == g.nodes.len() && !nodes.contains_key(&0),
        "duplicate graph node",
    )?;
    for p in nodes.values() {
        require(
            finite(*p) && p[1] == 0.0 && contains(&bounds_rect(&m.bounds), &around(*p, [0.0; 2])),
            "graph node outside ground map",
        )?;
    }
    let mut edges = BTreeSet::new();
    for e in &g.edges {
        let a = nodes.get(&e.from).ok_or("missing graph edge source")?;
        let b = nodes.get(&e.to).ok_or("missing graph edge target")?;
        require(
            e.from != e.to
                && edges.insert((e.from, e.to))
                && e.width > 0.0
                && e.speed_limit > 0.0
                && (a[0] == b[0] || a[2] == b[2]),
            "invalid/diagonal/duplicate graph edge",
        )?;
        let rect = Rect {
            min: [
                a[0].min(b[0]) - e.width / 2.0,
                a[2].min(b[2]) - e.width / 2.0,
            ],
            max: [
                a[0].max(b[0]) + e.width / 2.0,
                a[2].max(b[2]) + e.width / 2.0,
            ],
        };
        clear(&rect, 0.0, 2.0, colliders, "navigation edge")?;
    }
    // Strong connectivity, including access spurs, proves every mission start/finish route.
    for reverse in [false, true] {
        let start = g.nodes[0].id;
        let mut visited = BTreeSet::from([start]);
        let mut queue = VecDeque::from([start]);
        while let Some(id) = queue.pop_front() {
            for e in &g.edges {
                let (from, to) = if reverse {
                    (e.to, e.from)
                } else {
                    (e.from, e.to)
                };
                if from == id && visited.insert(to) {
                    queue.push_back(to);
                }
            }
        }
        require(
            visited.len() == nodes.len(),
            "navigation graph not strongly connected",
        )?;
    }
    unique(
        g.accesses.iter().map(|a| a.access_id.as_str()),
        "graph access",
    )?;
    require(g.accesses.len() == m.accesses.len(), "graph missing access")?;
    for a in &m.accesses {
        let link = g
            .accesses
            .iter()
            .find(|link| link.access_id == a.id)
            .ok_or("missing anchor graph connection")?;
        require(
            nodes.get(&link.node_id) == Some(&a.position),
            "graph access position mismatch",
        )?;
    }
    Ok(())
}
