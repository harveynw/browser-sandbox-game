//! Non-playable scenery, shared by the game and inspector through the backdrop.
use crate::types::*;

pub(crate) fn primitives() -> Vec<Primitive> {
    let mut out = Vec::new();
    let mut add = |name: &str, center: Vec3, half_extents: Vec3, color: &str| {
        out.push(Primitive {
            id: format!("surroundings_{name}"),
            role: VisualRole::Landmark,
            center: center.map(|v| (v * 10000.0).round() / 10000.0),
            half_extents: half_extents.map(|v| (v * 10000.0).round() / 10000.0),
            color: color.into(),
            collider_id: None,
            building: None,
        });
    };
    // The east coast meets the existing impassable city seawall. Broad shallow
    // bands descend towards the water without adding physics outside world bounds.
    add("sea", [2200.0, -2.0, 0.0], [1816.0, 0.2, 3600.0], "#447f98");
    add(
        "shallows",
        [506.0, -1.72, 0.0],
        [34.0, 0.05, 900.0],
        "#64a5ad",
    );
    add(
        "wet-sand",
        [465.0, -1.5, 0.0],
        [15.0, 0.15, 900.0],
        "#b7b18d",
    );
    add("beach", [423.0, -0.8, 0.0], [39.0, 0.55, 900.0], "#d6c59b");
    add("surf", [481.0, -1.64, 0.0], [1.5, 0.03, 900.0], "#c4ded7");
    // Land shelves fill gaps between the mountain bases; none cover the city.
    add(
        "west-land",
        [-984.0, -2.0, 0.0],
        [600.0, 1.0, 1500.0],
        "#52695d",
    );
    for sign in [-1.0, 1.0] {
        add(
            &format!("land-{sign}"),
            [-542.0, -2.0, sign * 984.0],
            [926.0, 1.0, 600.0],
            "#52695d",
        );
    }
    // Two irregular, overlapping ridgelines on each of the three inland sides.
    for side in 0..3 {
        for row in 0..2 {
            for i in 0..9 {
                let seed = (side * 73 + row * 41 + i * 29) as usize;
                let along = -730.0 + f64::from(i) * 142.0;
                let radius = 130.0 + (seed % 5) as f64 * 13.0;
                let distance = 590.0 + f64::from(row) * 270.0 + (seed % 3) as f64 * 28.0;
                let height = if row == 0 { 110.0 } else { 245.0 } + (seed % 7) as f64 * 23.0;
                let (x, z) = match side {
                    0 => (-distance, along),
                    1 => (along - 120.0, -distance),
                    _ => (along - 120.0, distance),
                };
                let colors = ["#69767c", "#78838a", "#5e6e75", "#889193"];
                let snow = if row == 1 { 0.30 } else { 0.0 };
                let rock_height = height * (1.0 - snow);
                add(
                    &format!("peak-{side}-{row}-{i}"),
                    [x, rock_height / 2.0 - 2.0, z],
                    [radius, rock_height / 2.0, radius * 0.85],
                    colors[seed % colors.len()],
                );
                if row == 1 {
                    // Rock ends at the snow line: adjacent meshes, no overlapping faces.
                    add(
                        &format!("snow-{side}-{row}-{i}"),
                        [x, height * (1.0 - snow / 2.0) - 2.0, z],
                        [radius * snow, height * snow / 2.0, radius * 0.85 * snow],
                        "#e6eef0",
                    );
                }
            }
        }
    }
    // Low polygonal foothills soften the join between the boundary and peaks.
    for side in 0..3 {
        for i in 0..12 {
            let along = -740.0 + f64::from(i) * 100.0;
            let (x, z) = match side {
                0 => (-462.0, along),
                1 => (along, -462.0),
                _ => (along, 462.0),
            };
            add(
                &format!("hill-{side}-{i}"),
                [x, 9.0, z],
                [76.0, 28.0 + f64::from(i % 4) * 7.0, 76.0],
                if i % 2 == 0 { "#667b62" } else { "#586e60" },
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snow_caps_meet_rock_without_overlapping_surfaces() {
        let parts = primitives();
        for snow in parts
            .iter()
            .filter(|p| p.id.starts_with("surroundings_snow-"))
        {
            let rock_id = snow.id.replace("_snow-", "_peak-");
            let rock = parts.iter().find(|p| p.id == rock_id).unwrap();
            let seam = rock.center[1] + rock.half_extents[1];
            assert!((snow.center[1] - snow.half_extents[1] - seam).abs() < 0.0002);
            for axis in [0, 2] {
                assert!((snow.half_extents[axis] - rock.half_extents[axis] * 0.30).abs() < 0.0002);
            }
        }
    }

    #[test]
    fn scenery_is_outside_playable_city_and_has_no_collision() {
        for p in primitives() {
            assert!(p.collider_id.is_none());
            assert!(p.half_extents.iter().all(|v| *v > 0.0));
            assert!(
                [0, 2]
                    .iter()
                    .any(|&a| p.center[a] - p.half_extents[a] >= 384.0
                        || p.center[a] + p.half_extents[a] <= -384.0),
                "{} intrudes into city",
                p.id
            );
        }
    }
}
