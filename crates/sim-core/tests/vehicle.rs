use content::{Authoring, BoxShape, ColliderRole, CollisionPackage, StaticCollider};
use protocol::{Buttons, InputAxis, Scalar, Yaw};
use sim_core::{
    CollisionQueries, CollisionWorld, SimConfig, StepContext, neutral_input, spawn_vehicle,
    step_vehicle,
};
fn source() -> Authoring {
    serde_json::from_str(include_str!("../../../content/source/slice.json")).unwrap()
}
fn room(wall: bool) -> (SimConfig, CollisionWorld) {
    let config = SimConfig::from_content(&source().sim_config).unwrap();
    let mut colliders = vec![StaticCollider {
        id: 1,
        shape: BoxShape::Box,
        role: ColliderRole::Ground,
        center: [0.0, -0.5, 0.0],
        half_extents: [128.0, 0.5, 128.0],
        yaw: 0.0,
        flags: 3,
    }];
    if wall {
        colliders.push(StaticCollider {
            id: 2,
            shape: BoxShape::Box,
            role: ColliderRole::Building,
            center: [0.0, 2.0, -5.0],
            half_extents: [20.0, 2.0, 0.05],
            yaw: 0.0,
            flags: 3,
        });
    }
    let world = CollisionWorld::from_content(
        &CollisionPackage {
            schema_version: 1,
            colliders,
        },
        &config,
    )
    .unwrap();
    (config, world)
}
#[test]
fn driving_from_slice_parking_keeps_a_valid_flat_motor() {
    let source = source();
    let package = content::bake::bake(&source).unwrap();
    let config = SimConfig::from_content(&source.sim_config).unwrap();
    let collision = serde_json::from_slice(&package.files["collision.json"]).unwrap();
    let world = CollisionWorld::from_content(&collision, &config).unwrap();
    let mut state = spawn_vehicle([-152.0, 0.0, 176.0], 0.0).unwrap();
    for tick in 1..=300 {
        let mut input = neutral_input(tick, Yaw(0.0));
        if tick <= 45 {
            input.move_z = InputAxis(32767);
        }
        let next = step_vehicle(
            &state,
            &input,
            &source.vehicle_templates[0],
            &StepContext {
                tick,
                collision: &world,
                config: &config,
            },
        );
        assert!(
            next.is_ok(),
            "tick {tick}, state {state:?}: {:?}",
            next.err().map(|e| e.to_string())
        );
        state = next.unwrap().motor;
        assert!(
            state.position.0[1].0.abs() <= 0.001,
            "tick {tick}: {state:?}"
        );
    }
}
#[test]
fn held_handbrake_overrides_throttle_stops_both_directions_and_releases() {
    let (config, world) = room(false);
    let template = &source().vehicle_templates[0];
    for direction in [-1.0_f32, 1.0] {
        let mut state = spawn_vehicle([0.0, 0.0, 0.0], 0.0).unwrap();
        state.speed = Scalar(direction * 6.0);
        state.velocity = protocol::Vec3([Scalar(0.0), Scalar(0.0), Scalar(-direction * 6.0)]);
        let mut coast = state.clone();
        for tick in 1..=30 {
            let context = StepContext {
                tick,
                collision: &world,
                config: &config,
            };
            coast = step_vehicle(&coast, &neutral_input(tick, Yaw(0.0)), template, &context)
                .unwrap()
                .motor;
            let mut input = neutral_input(tick, Yaw(0.0));
            input.move_z = InputAxis((direction * 32767.0) as i16);
            input.buttons = Buttons(4);
            state = step_vehicle(&state, &input, template, &context)
                .unwrap()
                .motor;
        }
        assert!(
            coast.speed.0.abs() > 4.0,
            "coasting must remain distinct from braking"
        );
        assert_eq!(state.speed.0, 0.0, "Space must stop despite held throttle");
        assert!(
            state.position.0[2].0.abs() < 2.0,
            "handbrake stopping distance"
        );
        let stopped = state.position.clone();
        for tick in 31..=60 {
            let mut input = neutral_input(tick, Yaw(0.0));
            input.move_z = InputAxis((direction * 32767.0) as i16);
            input.buttons = Buttons(4);
            state = step_vehicle(
                &state,
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
        }
        assert_eq!(state.position, stopped, "held brake prevents creeping");
        let mut input = neutral_input(61, Yaw(0.0));
        input.move_z = InputAxis((direction * 32767.0) as i16);
        state = step_vehicle(
            &state,
            &input,
            template,
            &StepContext {
                tick: 61,
                collision: &world,
                config: &config,
            },
        )
        .unwrap()
        .motor;
        assert!(
            state.speed.0 * direction > 0.2,
            "release restores acceleration"
        );
    }
}
#[test]
fn throttle_brake_reverse_steering_and_handbrake_are_bounded() {
    let (config, world) = room(false);
    let template = &source().vehicle_templates[0];
    let mut state = spawn_vehicle([0.0, 0.0, 40.0], 0.0).unwrap();
    for tick in 1..=90 {
        let mut input = neutral_input(tick, Yaw(0.0));
        input.move_z = InputAxis(32767);
        state = step_vehicle(
            &state,
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
    }
    assert!((state.speed.0 - 21.0).abs() < 0.05);
    let forward = state.speed.0;
    let mut reverse = neutral_input(91, Yaw(0.0));
    reverse.move_z = InputAxis(-32767);
    state = step_vehicle(
        &state,
        &reverse,
        template,
        &StepContext {
            tick: 91,
            collision: &world,
            config: &config,
        },
    )
    .unwrap()
    .motor;
    assert!((state.speed.0 - (forward - 0.4)).abs() < 0.001);
    for tick in 92..=200 {
        reverse.tick = tick;
        state = step_vehicle(
            &state,
            &reverse,
            template,
            &StepContext {
                tick,
                collision: &world,
                config: &config,
            },
        )
        .unwrap()
        .motor;
    }
    assert!(state.speed.0 < 0.0 && state.speed.0 >= -7.001);
    let mut handbrake = neutral_input(201, Yaw(0.0));
    handbrake.buttons = Buttons(4);
    let before = state.speed.0.abs();
    let after = step_vehicle(
        &state,
        &handbrake,
        template,
        &StepContext {
            tick: 201,
            collision: &world,
            config: &config,
        },
    )
    .unwrap()
    .motor;
    assert!(after.speed.0.abs() < before - 0.3);
    let mut stationary = spawn_vehicle([0.0, 0.0, 0.0], 0.0).unwrap();
    for tick in 1..=10 {
        let mut i = neutral_input(tick, Yaw(0.0));
        i.move_x = InputAxis(32767);
        stationary = step_vehicle(
            &stationary,
            &i,
            template,
            &StepContext {
                tick,
                collision: &world,
                config: &config,
            },
        )
        .unwrap()
        .motor;
    }
    assert_eq!(stationary.yaw.0, 0.0);
    let mut right = neutral_input(11, Yaw(0.0));
    right.move_x = InputAxis(32767);
    right.move_z = InputAxis(32767);
    let turned = step_vehicle(
        &stationary,
        &right,
        template,
        &StepContext {
            tick: 11,
            collision: &world,
            config: &config,
        },
    )
    .unwrap()
    .motor;
    assert!(turned.yaw.0 < 0.0 && turned.position.0[0].0 > 0.0);
}
#[test]
fn thirty_metre_per_second_sweep_cannot_tunnel_through_a_thin_wall() {
    let (config, world) = room(true);
    let mut template = source().vehicle_templates.remove(0);
    template.top_speed = 30.0;
    let mut state = spawn_vehicle([0.0, 0.0, -2.5], 0.0).unwrap();
    state.speed = Scalar(30.0);
    let result = step_vehicle(
        &state,
        &neutral_input(1, Yaw(0.0)),
        &template,
        &StepContext {
            tick: 1,
            collision: &world,
            config: &config,
        },
    )
    .unwrap();
    assert!(result.motor.position.0[2].0 > -2.9);
    assert!(result.impact_delta_speed > 10.0);
    assert!(result.contacts.iter().any(|c| c.collider_id == 2));
    assert!(
        !world
            .overlaps_vehicle(
                result.motor.position.0.map(|v| v.0),
                result.motor.yaw.0,
                template.dimensions.map(|v| v as f32)
            )
            .unwrap()
    );
}
#[test]
fn proxy_exclusion_removal_and_character_collision_share_the_query_world() {
    let (_, mut world) = room(false);
    let proxy = sim_core::collision::VehicleProxy {
        entity_id: 9,
        position: [0.0, 0.0, 0.0],
        yaw: 0.0,
        dimensions: [1.8, 1.4, 4.2],
    };
    world
        .set_vehicle_proxies(std::slice::from_ref(&proxy))
        .unwrap();
    assert!(
        world
            .overlaps_character([0.0, 0.0, 0.0], sim_core::QueryFilter::MOVEMENT)
            .unwrap()
    );
    let recovered = world.recover_character([0.0, 0.02, 0.0]).unwrap();
    assert!(
        !world
            .overlaps_character(recovered, sim_core::QueryFilter::MOVEMENT)
            .unwrap()
    );
    assert!((recovered[0].abs() - 1.29).abs() < 0.01);
    world.exclude_vehicle(Some(9));
    assert!(
        !world
            .overlaps_vehicle(proxy.position, proxy.yaw, proxy.dimensions)
            .unwrap()
    );
    world.exclude_vehicle(None);
    assert!(
        world
            .overlaps_vehicle(proxy.position, proxy.yaw, proxy.dimensions)
            .unwrap()
    );
    world.set_vehicle_proxies(&[]).unwrap();
    assert!(
        !world
            .overlaps_character([0.0, 0.0, 0.0], sim_core::QueryFilter::MOVEMENT)
            .unwrap()
    );
    for id in 10..210 {
        let mut p = proxy.clone();
        p.entity_id = id;
        world.set_vehicle_proxies(&[p]).unwrap();
    }
    assert_eq!(world.collider_count(), 2); // Ground plus one reused proxy slot.
}

#[test]
fn recorded_native_vehicle_trace_retains_all_sixty_checkpoints() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/simulation/vehicle-trace.json"
    ))
    .unwrap();
    let config =
        SimConfig::from_content(&serde_json::from_value(fixture["sim_config"].clone()).unwrap())
            .unwrap();
    let template: content::VehicleTemplate =
        serde_json::from_value(fixture["template"].clone()).unwrap();
    let world = CollisionWorld::from_content(
        &serde_json::from_value(fixture["collision"].clone()).unwrap(),
        &config,
    )
    .unwrap();
    let mut motor: protocol::VehicleMotor =
        serde_json::from_value(fixture["initial_motor"].clone()).unwrap();
    let mut tick = 0;
    let mut checked = 0;
    for segment in fixture["segments"].as_array().unwrap() {
        for _ in 0..segment["ticks"].as_u64().unwrap() {
            tick += 1;
            let mut input = neutral_input(tick, Yaw(0.0));
            input.move_x = InputAxis(segment["move_x"].as_i64().unwrap() as i16);
            input.move_z = InputAxis(segment["move_z"].as_i64().unwrap() as i16);
            input.buttons = Buttons(segment["buttons"].as_u64().unwrap() as u8);
            motor = step_vehicle(
                &motor,
                &input,
                &template,
                &StepContext {
                    tick,
                    collision: &world,
                    config: &config,
                },
            )
            .unwrap()
            .motor;
            if let Some(c) = fixture["checkpoints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["tick"].as_u64() == Some(u64::from(tick)))
            {
                let expected: protocol::VehicleMotor =
                    serde_json::from_value(c["motor"].clone()).unwrap();
                for i in 0..3 {
                    assert!((motor.position.0[i].0 - expected.position.0[i].0).abs() < 0.0001);
                }
                assert!((motor.speed.0 - expected.speed.0).abs() < 0.0001);
                checked += 1;
            }
        }
    }
    assert_eq!((tick, checked), (1800, 60));
}
