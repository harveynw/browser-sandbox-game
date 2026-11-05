//! Bespoke, stepped low-poly landmarks. Coordinates are fractions of the authored
//! envelope, so visual detail, collision and chunk clipping use one source of truth.
use crate::types::*;

pub(crate) fn parts(l: &Landmark) -> Vec<Primitive> {
    let mut out = Vec::new();
    let mut part = |name: &str, p: Vec3, size: Vec3, color: &str, solid: bool| {
        out.push(Primitive {
            id: format!("landmark_{}_{}_{}", l.id, name, out.len()),
            role: VisualRole::Landmark,
            center: std::array::from_fn(|a| {
                ((l.position[a] + p[a] * l.size[a]) * 10000.0).round() / 10000.0
            }),
            half_extents: std::array::from_fn(|a| (size[a] * l.size[a] * 5000.0).round() / 10000.0),
            color: color.into(),
            // A sentinel consumed by the baker, which assigns the actual ID.
            collider_id: solid.then_some(0),
            building: None,
        });
    };
    let stone = "#d7cbb0";
    let dark = "#36494b";
    let metal = "#597577";
    let glass = "#9fc6ce";
    let copper = "#588d7b";
    match l.id.as_str() {
        "quay_water_tower" => {
            for x in [-0.32, 0.32] {
                for z in [-0.32, 0.32] {
                    part("foot", [x, 0.025, z], [0.24, 0.05, 0.24], stone, true);
                    part("leg", [x, 0.32, z], [0.10, 0.59, 0.10], metal, true);
                }
            }
            for y in [0.23, 0.48, 0.60] {
                for s in [-0.32, 0.32] {
                    part("frame", [s, y, 0.0], [0.10, 0.025, 0.74], metal, true);
                    part("frame", [0.0, y, s], [0.74, 0.025, 0.10], metal, true);
                }
            }
            // Crossed volumes form a chamfered tank rather than a square tower.
            for (w, d) in [(0.94, 0.62), (0.62, 0.94)] {
                part("tank", [0.0, 0.77, 0.0], [w, 0.28, d], &l.color, true);
                for y in [0.65, 0.88] {
                    part(
                        "tank-band",
                        [0.0, y, 0.0],
                        [w + 0.02, 0.018, d + 0.02],
                        stone,
                        false,
                    );
                }
            }
            for i in 0..5 {
                let f = f64::from(i);
                part(
                    "roof",
                    [0.0, 0.92 + f * 0.015, 0.0],
                    [0.84 - f * 0.14, 0.02, 0.84 - f * 0.14],
                    copper,
                    true,
                );
            }
            for i in 0..22 {
                part(
                    "ladder-rung",
                    [0.0, 0.04 + f64::from(i) * 0.027, -0.38],
                    [0.18, 0.006, 0.035],
                    stone,
                    false,
                );
            }
        }
        "civic_clock" => {
            part("plinth", [0.0, 0.025, 0.0], [1.0, 0.05, 1.0], stone, true);
            part(
                "shaft",
                [0.0, 0.37, 0.0],
                [0.68, 0.69, 0.68],
                &l.color,
                true,
            );
            for y in [0.12, 0.44, 0.65, 0.81] {
                part("cornice", [0.0, y, 0.0], [0.84, 0.025, 0.84], stone, true);
            }
            part(
                "clock-room",
                [0.0, 0.73, 0.0],
                [0.76, 0.14, 0.76],
                stone,
                true,
            );
            for axis in [0, 2] {
                for sign in [-1.0, 1.0] {
                    let mut face = |name: &str, u: f64, y: f64, w: f64, h: f64, color: &str| {
                        let depth = if name == "dial-inset" { 0.390 } else { 0.382 };
                        let mut p = [u, y, sign * depth];
                        let mut s = [w, h, 0.012];
                        if axis == 0 {
                            p.swap(0, 2);
                            s.swap(0, 2);
                        }
                        part(name, p, s, color, false);
                    };
                    face("dial", 0.0, 0.73, 0.49, 0.115, dark);
                    face("dial-inset", 0.0, 0.73, 0.40, 0.095, "#faf0ce");
                    // Raised hands avoid coplanar faces.
                    for (u, y, w, h) in [(0.0, 0.748, 0.025, 0.043), (0.07, 0.73, 0.16, 0.008)] {
                        let mut p = [u, y, sign * 0.397];
                        let mut s = [w, h, 0.008];
                        if axis == 0 {
                            p.swap(0, 2);
                            s.swap(0, 2);
                        }
                        part("clock-hand", p, s, dark, false);
                    }
                    for u in [-0.22, 0.22] {
                        let mut p = [u, 0.37, sign * 0.346];
                        let mut s = [0.07, 0.41, 0.015];
                        if axis == 0 {
                            p.swap(0, 2);
                            s.swap(0, 2);
                        }
                        part("pilaster", p, s, stone, false);
                    }
                }
            }
            for i in 0..8 {
                let f = f64::from(i);
                part(
                    "copper-spire",
                    [0.0, 0.83 + f * 0.02, 0.0],
                    [0.70 - f * 0.085, 0.025, 0.70 - f * 0.085],
                    copper,
                    true,
                );
            }
            part(
                "finial",
                [0.0, 0.985, 0.0],
                [0.045, 0.03, 0.045],
                "#dbb462",
                true,
            );
        }
        "iron_chimney" => {
            part("furnace", [0.0, 0.09, 0.0], [1.0, 0.18, 1.0], dark, true);
            for i in 0..12 {
                let f = f64::from(i);
                let w = 0.68 - f * 0.027;
                for (x, z) in [(w, w * 0.68), (w * 0.68, w)] {
                    part(
                        "brick-stack",
                        [0.0, 0.21 + f * 0.062, 0.0],
                        [x, 0.062, z],
                        &l.color,
                        true,
                    );
                }
                if i % 3 == 0 {
                    part(
                        "iron-band",
                        [0.0, 0.23 + f * 0.062, 0.0],
                        [w + 0.025, 0.012, w * 0.70],
                        metal,
                        false,
                    );
                }
            }
            part("neck", [0.0, 0.93, 0.0], [0.39, 0.04, 0.39], &l.color, true);
            for x in [-0.21, 0.21] {
                part("rim", [x, 0.97, 0.0], [0.08, 0.06, 0.50], stone, true);
                part("rim", [0.0, 0.97, x], [0.50, 0.06, 0.08], stone, true);
            }
            part(
                "soot",
                [0.0, 0.941, 0.0],
                [0.34, 0.005, 0.34],
                "#202b30",
                false,
            );
            for x in [-0.28, 0.0, 0.28] {
                part(
                    "furnace-door",
                    [x, 0.075, -0.503],
                    [0.18, 0.09, 0.008],
                    "#a86436",
                    false,
                );
            }
        }
        "docks_crane_west" | "docks_crane_east" => {
            let mirror = if l.id.ends_with("east") { -1.0 } else { 1.0 };
            for x in [-0.34, 0.34] {
                for z in [-0.34, 0.34] {
                    part("bogie", [x, 0.025, z], [0.24, 0.05, 0.24], dark, true);
                    part(
                        "gantry-leg",
                        [x, 0.31, z],
                        [0.08, 0.57, 0.08],
                        &l.color,
                        true,
                    );
                }
                part(
                    "gantry-beam",
                    [x, 0.59, 0.0],
                    [0.12, 0.045, 0.82],
                    &l.color,
                    true,
                );
            }
            part(
                "crossbeam",
                [0.0, 0.62, 0.0],
                [0.86, 0.045, 0.80],
                &l.color,
                true,
            );
            part(
                "cab",
                [-0.23 * mirror, 0.685, 0.0],
                [0.30, 0.09, 0.46],
                "#d5b368",
                true,
            );
            part(
                "cab-glass",
                [-0.23 * mirror, 0.70, -0.233],
                [0.24, 0.04, 0.01],
                glass,
                false,
            );
            part(
                "mast",
                [-0.16 * mirror, 0.82, 0.0],
                [0.10, 0.36, 0.15],
                &l.color,
                true,
            );
            for y in [0.78, 0.85] {
                part("jib", [0.0, y, 0.0], [1.0, 0.025, 0.18], &l.color, true);
            }
            for i in 0..10 {
                part(
                    "jib-strut",
                    [-0.45 + f64::from(i) * 0.10, 0.815, 0.0],
                    [0.025, 0.07, 0.18],
                    stone,
                    true,
                );
            }
            part(
                "counterweight",
                [-0.41 * mirror, 0.745, 0.0],
                [0.18, 0.08, 0.30],
                dark,
                true,
            );
            part(
                "cable",
                [0.42 * mirror, 0.56, 0.0],
                [0.012, 0.44, 0.012],
                dark,
                false,
            );
            part(
                "hook",
                [0.42 * mirror, 0.335, 0.0],
                [0.06, 0.025, 0.07],
                "#d9ae51",
                true,
            );
        }
        "market_canopy" | "heights_park_pavilion" => {
            let market = l.id == "market_canopy";
            let roof = if market { "#b95e49" } else { copper };
            // An open colonnade; no invisible solid envelope under the roof.
            for x in [-0.40, 0.40] {
                for z in [-0.40, 0.0, 0.40] {
                    part(
                        "column-base",
                        [x, 0.025, z],
                        [0.16, 0.05, 0.16],
                        stone,
                        true,
                    );
                    part(
                        "column",
                        [x, 0.29, z],
                        [0.075, 0.53, 0.075],
                        if market { dark } else { stone },
                        true,
                    );
                    part("capital", [x, 0.55, z], [0.14, 0.04, 0.14], stone, true);
                }
            }
            part("eaves", [0.0, 0.59, 0.0], [1.0, 0.06, 1.0], stone, true);
            for i in 0..9 {
                let f = f64::from(i);
                let w = 0.98 - f * 0.09;
                part(
                    "hipped-roof",
                    [0.0, 0.63 + f * 0.03, 0.0],
                    [w, 0.035, if market { 0.94 } else { w }],
                    roof,
                    true,
                );
            }
            part("lantern", [0.0, 0.91, 0.0], [0.18, 0.07, 0.18], glass, true);
            part(
                "lantern-cap",
                [0.0, 0.96, 0.0],
                [0.26, 0.03, 0.26],
                roof,
                true,
            );
            if market {
                for z in [-0.25, 0.25] {
                    part(
                        "stall",
                        [0.29, 0.075, z],
                        [0.18, 0.15, 0.27],
                        "#b38b55",
                        true,
                    );
                    for i in 0..4 {
                        part(
                            "produce",
                            [0.29, 0.16, z - 0.09 + f64::from(i) * 0.06],
                            [0.14, 0.025, 0.04],
                            if i % 2 == 0 { "#d7ae45" } else { "#719652" },
                            false,
                        );
                    }
                }
                for i in 0..10 {
                    part(
                        "striped-valance",
                        [-0.45 + f64::from(i) * 0.10, 0.55, -0.47],
                        [0.10, 0.06, 0.035],
                        if i % 2 == 0 { &l.color } else { "#f0dcb5" },
                        false,
                    );
                }
            } else {
                for x in [-0.28, 0.28] {
                    part(
                        "bench",
                        [x, 0.065, 0.0],
                        [0.10, 0.04, 0.44],
                        "#967451",
                        true,
                    );
                    part(
                        "bench-foot",
                        [x, 0.025, 0.0],
                        [0.07, 0.05, 0.32],
                        dark,
                        true,
                    );
                }
            }
        }
        "garage_beacon" => {
            part(
                "service-base",
                [0.0, 0.08, 0.0],
                [1.0, 0.16, 1.0],
                stone,
                true,
            );
            for x in [-0.27, 0.27] {
                part("sign-post", [x, 0.49, 0.0], [0.12, 0.70, 0.14], dark, true);
            }
            part(
                "enamel-sign",
                [0.0, 0.78, 0.0],
                [0.95, 0.25, 0.28],
                &l.color,
                true,
            );
            for sign in [-1.0, 1.0] {
                for (x, y, w, h) in [
                    (-0.21, 0.78, 0.06, 0.17),
                    (0.0, 0.85, 0.44, 0.03),
                    (0.19, 0.81, 0.06, 0.10),
                    (0.0, 0.77, 0.40, 0.03),
                ] {
                    part(
                        "parking-letter",
                        [x, y, sign * 0.145],
                        [w, h, 0.015],
                        "#fff0c8",
                        false,
                    );
                }
            }
            for i in 0..4 {
                part(
                    "crown",
                    [0.0, 0.92 + f64::from(i) * 0.02, 0.0],
                    [0.80 - f64::from(i) * 0.18, 0.02, 0.34],
                    stone,
                    true,
                );
            }
        }
        _ => part("shell", [0.0, 0.5, 0.0], [1.0, 1.0, 1.0], &l.color, true),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn city() -> Authoring {
        serde_json::from_str(include_str!("../../../content/source/city.json")).unwrap()
    }

    #[test]
    fn all_city_landmarks_are_bespoke_and_stay_in_their_reserved_envelopes() {
        for l in city().landmarks {
            let model = parts(&l);
            assert!(model.len() >= 15, "{} is still a placeholder", l.id);
            for p in model {
                for a in 0..3 {
                    let min = l.position[a] - if a == 1 { 0.0 } else { l.size[a] / 2.0 };
                    let max = min + l.size[a];
                    // Shallow facade relief is allowed outside the structural envelope.
                    assert!(p.half_extents[a] > 0.0);
                    assert!(p.center[a] - p.half_extents[a] >= min - 0.08, "{}", p.id);
                    assert!(p.center[a] + p.half_extents[a] <= max + 0.08, "{}", p.id);
                }
            }
        }
    }

    #[test]
    fn open_landmarks_leave_a_player_sized_path_through_the_middle() {
        for l in city().landmarks.into_iter().filter(|l| {
            matches!(
                l.id.as_str(),
                "quay_water_tower"
                    | "market_canopy"
                    | "heights_park_pavilion"
                    | "docks_crane_west"
                    | "docks_crane_east"
            )
        }) {
            for p in parts(&l).into_iter().filter(|p| p.collider_id.is_some()) {
                let overlaps_path = (p.center[0] - l.position[0]).abs() < p.half_extents[0] + 0.35
                    && p.center[1] - p.half_extents[1] < 1.8
                    && p.center[1] + p.half_extents[1] > 0.0;
                assert!(!overlaps_path, "{} blocks the central passage", p.id);
            }
        }
    }
}
