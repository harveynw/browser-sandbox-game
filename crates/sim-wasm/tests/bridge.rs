use sim_wasm::bridge::*;
fn trace() -> sim_core::trace::CharacterTrace {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/simulation/character-trace.json"
    ))
    .unwrap()
}
#[test]
fn native_byte_bridge_replays_frozen_checkpoints_and_retains_static_world() {
    let trace = trace();
    let mut world = PredictionWorld::default();
    world
        .load_collision(
            &rmp_serde::to_vec_named(&trace.collision).unwrap(),
            &rmp_serde::to_vec_named(&trace.sim_config).unwrap(),
        )
        .unwrap();
    let initial = PredictedFrame {
        version: 1,
        tick: 0,
        control_epoch: protocol::ControlEpoch(1),
        controlled_entity_id: protocol::EntityId(7),
        motor: trace.initial_motor.clone(),
        vehicle: None,
    };
    world.reset(&encode(&initial).unwrap()).unwrap();
    let mut tick = 0;
    for segment in trace.segments {
        for _ in 0..segment.ticks {
            tick += 1;
            let request = StepRequest {
                version: 1,
                control_epoch: initial.control_epoch,
                controlled_entity_id: initial.controlled_entity_id,
                input: protocol::InputFrame {
                    tick,
                    move_x: protocol::InputAxis(segment.move_x),
                    move_z: protocol::InputAxis(segment.move_z),
                    look_yaw: protocol::Yaw(segment.look_yaw),
                    look_pitch: protocol::Pitch(0.0),
                    buttons: protocol::Buttons(segment.buttons),
                },
            };
            let frame: PredictedFrame = decode(
                &world.step(&encode(&request).unwrap()).unwrap(),
                CONTROL_BYTES,
            )
            .unwrap();
            if let Some(expected) = trace.checkpoints.iter().find(|c| c.tick == tick) {
                assert_eq!(frame.motor, expected.motor);
            }
        }
    }
    world.reset(&encode(&initial).unwrap()).unwrap();
    assert_eq!(
        decode::<PredictedFrame>(&world.current_state().unwrap(), CONTROL_BYTES)
            .unwrap()
            .tick,
        0
    );
}
