use sim_core::{CollisionQueries, CollisionWorld, QueryFilter, SimConfig, StepContext};
#[test]
fn full_collision_supports_every_lane_sidewalk_access_and_all_three_vehicle_motors() {
    let source: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/city.json")).unwrap();
    let baked = content::bake::bake(&source).unwrap();
    let collision = serde_json::from_slice(&baked.files["collision.json"]).unwrap();
    let config = SimConfig::from_content(&source.sim_config).unwrap();
    let world = CollisionWorld::from_content(&collision, &config).unwrap();
    let mut samples = 0;
    for graph in [&baked.manifest.road_graph, &baked.manifest.pedestrian_graph] {
        for edge in &graph.edges {
            let a = graph
                .nodes
                .iter()
                .find(|n| n.id == edge.from)
                .unwrap()
                .position;
            let b = graph
                .nodes
                .iter()
                .find(|n| n.id == edge.to)
                .unwrap()
                .position;
            let steps = (((a[0] - b[0]).hypot(a[2] - b[2])) / 4.0).ceil().max(1.0) as u32;
            for step in 0..=steps {
                let t = f64::from(step) / f64::from(steps);
                let feet = [
                    (a[0] + (b[0] - a[0]) * t) as f32,
                    0.02,
                    (a[2] + (b[2] - a[2]) * t) as f32,
                ];
                assert!(
                    !world
                        .overlaps_character(feet, QueryFilter::MOVEMENT)
                        .unwrap(),
                    "obstructed graph at {feet:?}"
                );
                let floor = world
                    .raycast(
                        [feet[0], 0.5, feet[2]],
                        [0.0, -1.0, 0.0],
                        1.0,
                        QueryFilter::MOVEMENT,
                    )
                    .unwrap()
                    .unwrap();
                assert!(floor.point[1].abs() < 0.001);
                samples += 1;
            }
        }
    }
    assert!(samples > 10000);
    let mut visible_landmarks = std::collections::BTreeSet::new();
    for x in [-256.0_f32, -128.0, 0.0, 128.0, 256.0] {
        for z in [-256.0_f32, -128.0, 0.0, 128.0, 256.0] {
            for landmark in world_landmarks(&collision) {
                let target = [
                    landmark.center[0] as f32,
                    (landmark.center[1] + landmark.half_extents[1] - 0.1) as f32,
                    landmark.center[2] as f32,
                ];
                let delta = [target[0] - x, target[1] - 1.7, target[2] - z];
                let distance = delta.iter().map(|v| v * v).sum::<f32>().sqrt();
                if distance < 400.0
                    && world
                        .raycast([x, 1.7, z], delta, distance + 0.2, QueryFilter::VISION)
                        .unwrap()
                        .is_some_and(|h| h.collider_id == landmark.id)
                {
                    visible_landmarks.insert(landmark.id);
                }
            }
        }
    }
    assert!(
        visible_landmarks.len() >= 3,
        "major intersections need distinguishable landmarks"
    );
    println!(
        "Landmarks visible from major intersections: {}",
        visible_landmarks.len()
    );
    // Physics exists everywhere without reading any detailed visual chunks.
    for template in &source.vehicle_templates {
        let mut motor = sim_core::spawn_vehicle([-128.0, 0.0, 128.0], 0.7).unwrap();
        for tick in 1..=600 {
            let mut input = sim_core::neutral_input(tick, protocol::Yaw(0.0));
            input.move_z = protocol::InputAxis(32767);
            motor = sim_core::step_vehicle(
                &motor,
                &input,
                template,
                &StepContext {
                    tick,
                    collision: &world,
                    config: &config,
                },
            )
            .unwrap_or_else(|e| panic!("{} tick {tick}: {e}", template.id))
            .motor;
            assert!(
                !world
                    .overlaps_vehicle(
                        motor.position.0.map(|v| v.0),
                        motor.yaw.0,
                        template.dimensions.map(|v| v as f32)
                    )
                    .unwrap()
            );
        }
    }
    println!(
        "Full-city traversable graph samples: {samples}; static colliders: {}",
        world.collider_count()
    );
}

#[test]
fn compact_drives_a_continuous_full_outer_ring_without_visual_data() {
    let source: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/city.json")).unwrap();
    let baked = content::bake::bake(&source).unwrap();
    let config = SimConfig::from_content(&source.sim_config).unwrap();
    let collision = serde_json::from_slice(&baked.files["collision.json"]).unwrap();
    let world = CollisionWorld::from_content(&collision, &config).unwrap();
    let template = &source.vehicle_templates[0];
    let mut motor = sim_core::spawn_vehicle([-352.0, 0.0, 320.0], 0.0).unwrap();
    let targets = [
        [-352.0_f32, -352.0],
        [352.0, -352.0],
        [352.0, 352.0],
        [-352.0, 352.0],
        [-352.0, 320.0],
    ];
    let mut waypoint = 0;
    let mut distance = 0.0_f32;
    for tick in 1..=12000 {
        let p = motor.position.0.map(|v| v.0);
        let target = targets[waypoint];
        let dx = target[0] - p[0];
        let dz = target[1] - p[2];
        let remaining = dx.hypot(dz);
        if remaining < 10.0 {
            waypoint += 1;
            if waypoint == targets.len() {
                assert!(distance > 2600.0);
                println!(
                    "Continuous ring: {tick} ticks, {distance:.1} m, {} corners",
                    waypoint - 1
                );
                return;
            }
        }
        let target = targets[waypoint];
        let yaw = (-(target[0] - p[0])).atan2(-(target[1] - p[2]));
        let error = (yaw - motor.yaw.0 + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
            - std::f32::consts::PI;
        let target_speed = if remaining < 35.0 || error.abs() > 0.3 {
            6.0
        } else {
            12.0
        };
        let mut input = sim_core::neutral_input(tick, protocol::Yaw(0.0));
        input.move_x =
            protocol::InputAxis((-error * 1.8).clamp(-1.0, 1.0).mul_add(32767.0, 0.0) as i16);
        input.move_z = protocol::InputAxis(if motor.speed.0 > target_speed + 0.3 {
            -32767
        } else if motor.speed.0 < target_speed - 0.3 {
            32767
        } else {
            0
        });
        motor = sim_core::step_vehicle(
            &motor,
            &input,
            template,
            &StepContext {
                tick,
                collision: &world,
                config: &config,
            },
        )
        .unwrap()
        .motor;
        distance += (motor.position.0[0].0 - p[0]).hypot(motor.position.0[2].0 - p[2]);
        assert_eq!(motor.position.0[1].0, 0.0);
    }
    panic!(
        "ring traversal stuck at waypoint {waypoint}: {:?}",
        motor.position
    );
}

fn world_landmarks(
    collision: &content::CollisionPackage,
) -> impl Iterator<Item = &content::StaticCollider> {
    collision
        .colliders
        .iter()
        .filter(|c| c.role == content::ColliderRole::Landmark)
}
