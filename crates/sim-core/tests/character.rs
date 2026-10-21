use content::{BoxShape, ColliderRole, CollisionPackage, StaticCollider};
use protocol::{Buttons, CharacterMotor, InputAxis, InputFrame, Yaw};
use sim_core::{
    CollisionQueries, CollisionWorld, QueryFilter, SimConfig, StepContext, neutral_input,
    spawn_character, step_character,
};
fn config() -> SimConfig {
    let a: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/slice.json")).unwrap();
    SimConfig::from_content(&a.sim_config).unwrap()
}
fn solid(id: u32, center: [f64; 3], half: [f64; 3]) -> StaticCollider {
    StaticCollider {
        id,
        shape: BoxShape::Box,
        role: ColliderRole::Building,
        center,
        half_extents: half,
        yaw: 0.0,
        flags: 3,
    }
}
fn room(extra: Vec<StaticCollider>) -> (SimConfig, CollisionWorld) {
    let c = config();
    let mut boxes = vec![solid(1, [0.0, -0.5, 0.0], [128.0, 0.5, 128.0])];
    boxes.extend(extra);
    let w = CollisionWorld::from_content(
        &CollisionPackage {
            schema_version: 1,
            colliders: boxes,
        },
        &c,
    )
    .unwrap();
    (c, w)
}
fn settled(w: &CollisionWorld, c: &SimConfig, p: [f32; 3]) -> CharacterMotor {
    let mut s = spawn_character(p, 0.0).unwrap();
    for tick in 1..=3 {
        s = step_character(
            &s,
            &neutral_input(tick, s.yaw),
            &StepContext {
                tick,
                collision: w,
                config: c,
            },
        )
        .unwrap()
        .motor;
    }
    assert!(s.grounded);
    s
}
fn input(tick: u32, x: i16, z: i16, buttons: u8) -> InputFrame {
    let mut i = neutral_input(tick, Yaw(0.0));
    i.move_x = InputAxis(x);
    i.move_z = InputAxis(z);
    i.buttons = Buttons(buttons);
    i
}
fn run(
    mut s: CharacterMotor,
    w: &CollisionWorld,
    c: &SimConfig,
    n: u32,
    x: i16,
    z: i16,
    b: u8,
) -> CharacterMotor {
    for tick in 1..=n {
        s = step_character(
            &s,
            &input(tick, x, z, b),
            &StepContext {
                tick,
                collision: w,
                config: c,
            },
        )
        .unwrap()
        .motor;
        assert!(
            !w.overlaps_character(s.position.0.map(|v| v.0), QueryFilter::MOVEMENT)
                .unwrap()
        );
    }
    s
}
fn speed(s: &CharacterMotor) -> f32 {
    s.velocity.0[0].0.hypot(s.velocity.0[2].0)
}
#[test]
fn walking_sprinting_diagonals_and_backward_limits() {
    let (c, w) = room(vec![]);
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    for (x, z, b, expected) in [
        (0, 32767, 0, 4.0),
        (0, 32767, 1, 7.0),
        (32767, 32767, 0, 4.0),
        (32767, 0, 1, 4.0),
        (0, -32767, 1, 4.0),
    ] {
        let result = run(s.clone(), &w, &c, 30, x, z, b);
        assert!(
            (speed(&result) - expected).abs() < 0.001,
            "{x},{z},{b}: {}",
            speed(&result)
        );
    }
    let diag = run(s, &w, &c, 30, 32767, 32767, 1);
    assert!(speed(&diag) <= 7.001 && diag.velocity.0[0].0 <= 4.001);
}
#[test]
fn ground_acceleration_and_neutral_braking_are_fixed_step() {
    let (c, w) = room(vec![]);
    let mut s = settled(&w, &c, [0.0, 0.0, 0.0]);
    for tick in 1..=4 {
        s = step_character(
            &s,
            &input(tick, 0, 32767, 0),
            &StepContext {
                tick,
                collision: &w,
                config: &c,
            },
        )
        .unwrap()
        .motor;
        assert!((speed(&s) - tick as f32).abs() < 0.001);
    }
    let yaw = s.yaw;
    let stopped = run(s, &w, &c, 5, 0, 0, 0);
    assert!(speed(&stopped) < 0.001);
    assert!((yaw.0 - stopped.yaw.0).abs() < 0.0001);
    assert!(stopped.grounded);
}
#[test]
fn jump_edge_cooldown_gravity_and_landing() {
    let (c, w) = room(vec![]);
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    let result = step_character(
        &s,
        &input(1, 0, 0, 2),
        &StepContext {
            tick: 1,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!(result.jumped);
    assert!(!result.motor.grounded);
    assert_eq!(result.motor.jump_cooldown_ticks, 5);
    assert!((result.motor.velocity.0[1].0 - 4.9).abs() < 0.0001);
    let second = step_character(
        &result.motor,
        &input(2, 0, 0, 2),
        &StepContext {
            tick: 2,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!(!second.jumped);
    assert!((second.motor.velocity.0[1].0 - 4.3).abs() < 0.0001);
    let landed = run(second.motor, &w, &c, 60, 0, 0, 0);
    assert!(landed.grounded);
    assert_eq!(landed.velocity.0[1].0, 0.0);
    assert!(landed.position.0[1].0.abs() < 0.03);
    let mut cooling = landed;
    cooling.jump_cooldown_ticks = 2;
    assert!(
        !step_character(
            &cooling,
            &input(1, 0, 0, 2),
            &StepContext {
                tick: 1,
                collision: &w,
                config: &c
            }
        )
        .unwrap()
        .jumped
    );
}
#[test]
fn wall_slide_and_diagonal_corner_do_not_penetrate() {
    let (c, w) = room(vec![
        solid(2, [2.0, 2.0, 0.0], [0.1, 2.0, 20.0]),
        solid(3, [0.0, 2.0, -4.0], [20.0, 2.0, 0.1]),
    ]);
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    let slide = run(s.clone(), &w, &c, 30, 32767, 32767, 0);
    assert!(slide.position.0[0].0 < 1.56);
    assert!(slide.position.0[2].0 < -2.0);
    let corner = run(s, &w, &c, 180, 32767, 32767, 1);
    assert!(corner.position.0[0].0 < 1.56 && corner.position.0[2].0 > -3.56);
    assert!(corner.grounded);
}
#[test]
fn a_low_ceiling_stops_upward_velocity() {
    let (c, w) = room(vec![solid(2, [0.0, 2.1, 0.0], [4.0, 0.1, 4.0])]);
    let mut s = settled(&w, &c, [0.0, 0.0, 0.0]);
    let mut hit = false;
    for tick in 1..=12 {
        let r = step_character(
            &s,
            &input(tick, 0, 0, if tick == 1 { 2 } else { 0 }),
            &StepContext {
                tick,
                collision: &w,
                config: &c,
            },
        )
        .unwrap();
        if r.hit_ceiling {
            hit = true;
            assert!(r.motor.velocity.0[1].0 <= 0.0);
        }
        s = r.motor;
    }
    assert!(hit);
    assert!(s.position.0[1].0 < 0.2);
}
#[test]
fn low_steps_climb_but_tall_obstacles_block() {
    for (height, climbs) in [(0.2, true), (0.4, false)] {
        let (c, w) = room(vec![solid(
            2,
            [0.0, height / 2.0, -2.5],
            [3.0, height / 2.0, 1.5],
        )]);
        let s = settled(&w, &c, [0.0, 0.0, 0.0]);
        let r = run(s, &w, &c, 20, 0, 32767, 0);
        if climbs {
            assert!(
                r.position.0[2].0 < -1.5,
                "step {height} stopped at {:?}",
                r.position
            );
            assert!(r.position.0[1].0 > 0.18);
        } else {
            assert!(r.position.0[2].0 > -0.66);
        }
    }
}
#[test]
fn ground_snap_does_not_cancel_a_launch_or_magnetize_an_airborne_actor() {
    let (c, w) = room(vec![solid(2, [0.0, 0.05, 0.0], [1.0, 0.05, 2.0])]);
    let s = settled(&w, &c, [0.0, 0.1, 0.0]);
    let r = run(s, &w, &c, 35, 32767, 0, 0);
    assert!(r.grounded);
    assert!(r.position.0[1].0 < 0.03);
    let airborne = spawn_character([5.0, 0.12, 0.0], 0.0).unwrap();
    let r = step_character(
        &airborne,
        &neutral_input(1, airborne.yaw),
        &StepContext {
            tick: 1,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!(r.motor.position.0[1].0 > 0.09);
    assert!(!r.motor.grounded);
}
#[test]
fn camera_yaw_convention_aim_turn_limit_and_shortest_arc() {
    let (c, w) = room(vec![]);
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    let mut i = input(1, 0, 32767, 0);
    i.look_yaw = Yaw(std::f32::consts::FRAC_PI_2);
    let r = step_character(
        &s,
        &i,
        &StepContext {
            tick: 1,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!(r.motor.velocity.0[0].0 < -0.99);
    assert!(r.motor.velocity.0[2].0.abs() < 0.001);
    assert!((r.motor.yaw.0 - 0.4).abs() < 0.0001);
    let mut s = s;
    s.yaw = Yaw(3.1);
    let mut i = input(1, 0, 0, 8);
    i.look_yaw = Yaw(-3.1);
    let r = step_character(
        &s,
        &i,
        &StepContext {
            tick: 1,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!((r.motor.yaw.0 + 3.1).abs() < 0.0001);
}
#[test]
fn filters_overlap_and_ray_identity_do_not_leak_engine_handles() {
    let mut blocker = solid(2, [0.0, 1.0, -3.0], [1.0, 1.0, 0.1]);
    blocker.flags = 2;
    let (c, w) = room(vec![blocker]);
    assert!(
        !w.overlaps_character([0.0, 0.0, -3.0], QueryFilter::MOVEMENT)
            .unwrap()
    );
    assert!(
        w.overlaps_character([0.0, 0.0, -3.0], QueryFilter::VISION)
            .unwrap()
    );
    let hit = w
        .raycast(
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -10.0],
            10.0,
            QueryFilter::VISION,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.collider_id, 2);
    assert!((hit.distance - 2.9).abs() < 0.001);
    assert!(
        w.raycast(
            [0.0, 1.0, 0.0],
            [0.0, 0.0, -1.0],
            10.0,
            QueryFilter {
                flags: 2,
                exclude_static_id: Some(2)
            }
        )
        .unwrap()
        .is_none()
    );
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    assert!(run(s, &w, &c, 60, 0, 32767, 0).position.0[2].0 < -7.0);
}
#[test]
fn invalid_restore_input_and_tick_fail_without_mutating_state() {
    let (c, w) = room(vec![]);
    let s = settled(&w, &c, [0.0, 0.0, 0.0]);
    let copy = s.clone();
    for mut i in [
        input(1, i16::MIN, 0, 0),
        input(1, 0, 0, 255),
        input(0, 0, 0, 0),
    ] {
        let tick = i.tick;
        if tick == 0 {
            i.tick = 2;
        }
        assert!(
            step_character(
                &s,
                &i,
                &StepContext {
                    tick,
                    collision: &w,
                    config: &c
                }
            )
            .is_err()
        );
    }
    let mut invalid = s.clone();
    invalid.position.0[0].0 = f32::NAN;
    assert!(
        step_character(
            &invalid,
            &neutral_input(1, s.yaw),
            &StepContext {
                tick: 1,
                collision: &w,
                config: &c
            }
        )
        .is_err()
    );
    assert_eq!(s, copy);
}
#[test]
fn free_fall_clamps_and_air_control_is_weaker() {
    let c = config();
    let w = CollisionWorld::from_content(
        &CollisionPackage {
            schema_version: 1,
            colliders: vec![],
        },
        &c,
    )
    .unwrap();
    let s = spawn_character([0.0, 500.0, 0.0], 0.0).unwrap();
    let first = step_character(
        &s,
        &input(1, 0, 32767, 0),
        &StepContext {
            tick: 1,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!((speed(&first.motor) - 1.0 / 3.0).abs() < 0.0001);
    let fallen = run(s, &w, &c, 100, 0, 0, 0);
    assert_eq!(fallen.velocity.0[1].0, -30.0);
    assert!(!fallen.grounded);
}
#[test]
fn baked_slice_spawns_have_real_capsule_support_and_clearance() {
    let a: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/slice.json")).unwrap();
    let p = content::bake::bake(&a).unwrap();
    let config = SimConfig::from_content(&a.sim_config).unwrap();
    let collision = serde_json::from_slice(&p.files["collision.json"]).unwrap();
    let world = CollisionWorld::from_content(&collision, &config).unwrap();
    assert_eq!(world.collider_count(), 35);
    for spawn in p.manifest.spawns {
        let s = settled(&world, &config, spawn.position.map(|n| n as f32));
        let r = run(s, &world, &config, 60, 0, 32767, 0);
        assert!(r.grounded);
    }
}
#[test]
fn recorded_sixty_second_trace_matches_all_frozen_checkpoints() {
    let trace: sim_core::trace::CharacterTrace = serde_json::from_str(include_str!(
        "../../../tests/fixtures/simulation/character-trace.json"
    ))
    .unwrap();
    let report = sim_core::trace::verify(&trace, 0.0001).unwrap();
    assert_eq!(report.ticks, 1800);
    assert_eq!(report.checkpoints, 13);
    let first = sim_core::trace::replay(&trace).unwrap();
    let second = sim_core::trace::replay(&trace).unwrap();
    for (a, b) in first.iter().zip(second) {
        assert_eq!(a.motor, b.motor);
    }
    let mut bad = trace;
    bad.checkpoints[0].motor.position.0[0].0 += 1.0;
    assert!(sim_core::trace::verify(&bad, 0.01).is_err());
}
#[test]
fn seeded_inputs_remain_finite_collision_safe_and_bounded() {
    let (c, w) = room(vec![
        solid(2, [6.0, 2.0, 0.0], [0.1, 2.0, 20.0]),
        solid(3, [0.0, 2.0, -6.0], [20.0, 2.0, 0.1]),
        solid(4, [-3.0, 0.1, -3.0], [1.0, 0.1, 1.0]),
    ]);
    for seed in [1u32, 17, 12345, 0xfeed] {
        let mut rng = seed;
        let mut s = settled(&w, &c, [0.0, 0.0, 0.0]);
        for tick in 1..=900 {
            rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
            let axis = |v: u32| match v % 3 {
                0 => -32767,
                1 => 0,
                _ => 32767,
            };
            let mut i = input(
                tick,
                axis(rng),
                axis(rng >> 4),
                if rng % 31 == 0 { 2 } else { (rng as u8) & 9 },
            );
            i.look_yaw = Yaw((rng % 6000) as f32 / 1000.0 - 3.0);
            let r = step_character(
                &s,
                &i,
                &StepContext {
                    tick,
                    collision: &w,
                    config: &c,
                },
            )
            .unwrap_or_else(|e| panic!("seed {seed} tick {tick} state {s:?} input {i:?}: {e}"));
            assert!(r.motor.position.0.iter().all(|n| n.0.is_finite()));
            assert!(
                !w.overlaps_character(r.motor.position.0.map(|n| n.0), QueryFilter::MOVEMENT)
                    .unwrap()
            );
            assert!(speed(&r.motor) <= 7.01);
            assert!(r.contacts.len() <= 16);
            s = r.motor;
        }
    }
}
#[test]
fn loaders_reject_invalid_configuration_and_collision_records() {
    let a: content::Authoring =
        serde_json::from_str(include_str!("../../../content/source/slice.json")).unwrap();
    let mut bad = a.sim_config.clone();
    bad.tick_rate = 60;
    assert!(SimConfig::from_content(&bad).is_err());
    bad = a.sim_config.clone();
    bad.character.gravity = f64::NAN;
    assert!(SimConfig::from_content(&bad).is_err());
    let c = config();
    let b = solid(1, [0.0, 0.0, 0.0], [1.0, 1.0, 1.0]);
    assert!(
        CollisionWorld::from_content(
            &CollisionPackage {
                schema_version: 1,
                colliders: vec![b.clone(), b.clone()]
            },
            &c
        )
        .is_err()
    );
    let mut bad = b;
    bad.yaw = f64::NAN;
    assert!(
        CollisionWorld::from_content(
            &CollisionPackage {
                schema_version: 1,
                colliders: vec![bad]
            },
            &c
        )
        .is_err()
    );
}

#[test]
fn grazing_step_corner_clips_instead_of_committing_a_penetrating_endpoint() {
    let (c, w) = room(vec![
        solid(2, [6.0, 2.0, 0.0], [0.1, 2.0, 20.0]),
        solid(3, [0.0, 2.0, -6.0], [20.0, 2.0, 0.1]),
        solid(4, [-3.0, 0.1, -3.0], [1.0, 0.1, 1.0]),
    ]);
    let mut state = spawn_character([-1.7123318, 0.020100763, -1.8905336], 2.6834838).unwrap();
    state.velocity = protocol::Vec3([-0.674107, 0.0, 1.3723432].map(protocol::Scalar));
    state.grounded = true;
    let mut input = input(205, 0, 32767, 8);
    input.look_yaw = Yaw(-0.27200007);
    let result = step_character(
        &state,
        &input,
        &StepContext {
            tick: 205,
            collision: &w,
            config: &c,
        },
    )
    .unwrap();
    assert!(result.endpoint_clamped);
    assert!(
        !w.overlaps_character(result.motor.position.0.map(|s| s.0), QueryFilter::MOVEMENT)
            .unwrap()
    );
}

#[test]
fn camera_sphere_keeps_radius_at_walls_corners_and_awnings() {
    let (_, w) = room(vec![
        solid(2, [0.0, 2.0, 3.0], [2.0, 2.0, 0.25]),
        solid(3, [5.0, 2.5, 0.0], [2.0, 0.25, 2.0]),
    ]);
    let hit = w
        .spherecast(
            [0.0, 1.45, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            0.2,
            QueryFilter::VISION,
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.collider_id, 2);
    assert!((hit.distance - 2.55).abs() < 0.001);
    // Center ray passes outside the wall, but the camera sphere grazes its corner.
    assert!(
        w.raycast([2.1, 1.45, 0.0], [0.0, 0.0, 1.0], 5.0, QueryFilter::VISION)
            .unwrap()
            .is_none()
    );
    assert!(
        w.spherecast(
            [2.1, 1.45, 0.0],
            [0.0, 0.0, 1.0],
            5.0,
            0.2,
            QueryFilter::VISION
        )
        .unwrap()
        .is_some()
    );
    let overhead = w
        .spherecast(
            [5.0, 1.45, 0.0],
            [0.0, 1.0, 0.0],
            3.0,
            0.2,
            QueryFilter::VISION,
        )
        .unwrap()
        .unwrap();
    assert_eq!(overhead.collider_id, 3);
    assert!((overhead.distance - 0.6).abs() < 0.001);
    assert_eq!(
        w.spherecast(
            [0.0, 1.45, 3.0],
            [0.0, 0.0, -1.0],
            5.0,
            0.2,
            QueryFilter::VISION
        )
        .unwrap()
        .unwrap()
        .distance,
        0.0
    );
    for radius in [f32::NAN, -0.1, 0.0, 3.0] {
        assert!(
            w.spherecast(
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
                5.0,
                radius,
                QueryFilter::VISION
            )
            .is_err()
        );
    }
}
