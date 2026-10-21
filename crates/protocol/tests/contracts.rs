use protocol::{fixtures, limits, scan, *};
use serde_json::json;

fn examples() -> Vec<fixtures::Fixture> {
    fixtures::positive().expect("positive fixtures")
}
fn example(name: &str) -> fixtures::Fixture {
    examples()
        .into_iter()
        .find(|f| f.name == name)
        .expect("named fixture")
}

#[test]
fn every_reference_example_crosses_the_real_codec_and_all_negative_frames_fail() {
    let positive = examples();
    assert!(positive.len() >= 70);
    for fixture in &positive {
        assert!(
            fixtures::decoded_fixture(fixture).is_ok(),
            "{}",
            fixture.name
        );
    }
    for fixture in fixtures::negative(&positive).expect("negative fixtures") {
        assert!(
            fixtures::decoded_fixture(&fixture).is_err(),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn maximal_realtime_messages_fit_the_packet_budget() {
    for name in ["max_input_batch", "max_owner_state"] {
        let fixture = example(name);
        assert!(fixture.hex.len() / 2 <= limits::UNRELIABLE_BYTES, "{name}");
    }
}

#[test]
fn fixtures_cover_every_discriminant_and_string_enum_in_generated_schemas() {
    use std::collections::BTreeSet;
    fn strings(value: &serde_json::Value, result: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::String(s) => {
                result.insert(s.clone());
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    strings(item, result);
                }
            }
            serde_json::Value::Object(items) => {
                for item in items.values() {
                    strings(item, result);
                }
            }
            _ => {}
        }
    }
    fn choices(value: &serde_json::Value, result: &mut BTreeSet<String>) {
        match value {
            serde_json::Value::Object(items) => {
                if let Some(serde_json::Value::String(s)) = items.get("const") {
                    result.insert(s.clone());
                }
                if let Some(serde_json::Value::Array(values)) = items.get("enum") {
                    for value in values {
                        if let Some(s) = value.as_str() {
                            result.insert(s.into());
                        }
                    }
                }
                for child in items.values() {
                    choices(child, result);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    choices(item, result);
                }
            }
            _ => {}
        }
    }
    let mut observed = BTreeSet::new();
    for fixture in examples() {
        strings(&fixture.value.expect("value"), &mut observed);
    }
    let mut expected = BTreeSet::new();
    choices(&contract::schema::<ClientEnvelope>(), &mut expected);
    choices(&contract::schema::<ServerEnvelope>(), &mut expected);
    assert!(
        expected.is_subset(&observed),
        "missing variants: {:?}",
        expected.difference(&observed).collect::<Vec<_>>()
    );
}

#[test]
fn phase_scope_epoch_baseline_and_input_window_are_checked_at_admission() {
    let fixture = example("input_batch");
    let message = decode_client(&fixtures::from_hex(&fixture.hex).expect("hex")).expect("decode");
    let world = OpaqueId("world-0001".into());
    let session = OpaqueId("session-0001".into());
    let mut context = ClientContext {
        world_id: &world,
        session_id: Some(&session),
        phase: Phase::Playing,
        control_epoch: Some(ControlEpoch(1)),
        next_tick: 31,
        baseline_id: Some(BaselineId(1)),
    };
    assert!(validate_client_context(&message, &context).is_ok());
    context.phase = Phase::Loading;
    assert!(validate_client_context(&message, &context).is_err());
    let ready = example("client_ready");
    let ready = decode_client(&fixtures::from_hex(&ready.hex).expect("hex")).expect("ready");
    assert!(validate_client_context(&ready, &context).is_ok());
    context.baseline_id = Some(BaselineId(2));
    assert!(validate_client_context(&ready, &context).is_err());
    context.phase = Phase::Playing;
    context.control_epoch = Some(ControlEpoch(2));
    assert!(validate_client_context(&message, &context).is_err());
    context.control_epoch = Some(ControlEpoch(1));
    context.next_tick = 32;
    assert!(validate_client_context(&message, &context).is_err());
    context.next_tick = 1;
    assert!(validate_client_context(&message, &context).is_err());
    context.next_tick = 31;
    context.session_id = None;
    assert!(validate_client_context(&message, &context).is_err());
}

#[test]
fn required_nulls_empty_maps_tuple_shape_and_kind_tags_are_enforced() {
    let mut value = example("owner_foot").value.expect("value");
    value["message"]["data"]["player_state"]
        .as_object_mut()
        .expect("state")
        .remove("vehicle_id");
    assert!(
        decode_server(
            &rmp_serde::to_vec_named(&value).expect("encode"),
            Delivery::Unreliable
        )
        .is_err()
    );
    let mut value = example("action_exit_vehicle").value.expect("value");
    value["message"]["data"]["action"]["data"] = json!({"unexpected":1});
    assert!(decode_client(&rmp_serde::to_vec_named(&value).expect("encode")).is_err());
    let mut value = example("owner_foot").value.expect("value");
    value["message"]["data"]["state"]["data"]["motor"]["position"] = json!([1, 2]);
    assert!(
        decode_server(
            &rmp_serde::to_vec_named(&value).expect("encode"),
            Delivery::Unreliable
        )
        .is_err()
    );
}

#[test]
fn semantic_state_inconsistencies_and_unreliable_control_changes_are_rejected() {
    let mut value = example("owner_driving").value.expect("value");
    value["message"]["data"]["state"]["data"]["driver_id"] = json!(3);
    assert!(
        decode_server(
            &rmp_serde::to_vec_named(&value).expect("encode"),
            Delivery::Unreliable
        )
        .is_err()
    );
    let fixture = example("control_changed");
    assert!(
        decode_server(
            &fixtures::from_hex(&fixture.hex).expect("hex"),
            Delivery::Unreliable
        )
        .is_err()
    );
}

#[test]
fn encoder_rejects_invalid_values_constructed_without_deserialization() {
    let bytes = fixtures::from_hex(&example("input_batch").hex).expect("hex");
    let mut value = decode_client(&bytes).expect("decode");
    if let ClientMessage::InputBatch(batch) = &mut value.message {
        batch.frames[0].look_yaw = Yaw(f32::NAN);
    }
    assert!(encode_client(&value).is_err());
}

#[test]
fn scanner_checks_depth_nodes_strings_and_allocation_sizes() {
    let mut nodes = vec![0xdc, 0, 128];
    for _ in 0..128 {
        nodes.push(0x9f);
        nodes.extend([0xc0; 15]);
    }
    assert!(scan::scan(&nodes, limits::RELIABLE_BYTES).is_err());
    let mut string = vec![0xda, 2, 1];
    string.extend([b'a'; 513]);
    assert!(scan::scan(&string, limits::RELIABLE_BYTES).is_err());
    let mut max = vec![0xdc, 0, 128];
    max.extend([0xc0; 128]);
    assert!(scan::scan(&max, limits::RELIABLE_BYTES).is_ok());
    assert!(scan::scan(&[0xdd, 0xff, 0xff, 0xff, 0xff], limits::CLIENT_BYTES).is_err());
}

#[test]
fn arbitrary_bounded_bytes_and_truncations_do_not_panic() {
    let mut random = 0x1234_5678_u32;
    for length in 0..256 {
        let bytes: Vec<_> = (0..length)
            .map(|_| {
                random ^= random << 13;
                random ^= random >> 17;
                random ^= random << 5;
                random as u8
            })
            .collect();
        assert!(std::panic::catch_unwind(|| decode_client(&bytes)).is_ok());
    }
    let bytes = fixtures::from_hex(&example("input_batch").hex).expect("hex");
    for length in 0..bytes.len() {
        assert!(decode_client(&bytes[..length]).is_err());
    }
}
