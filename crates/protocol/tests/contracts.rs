use protocol::{fixtures, limits, *};
use serde_json::{Value, json};
fn example(name: &str) -> fixtures::Fixture {
    fixtures::positive()
        .unwrap()
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
}
fn input() -> Value {
    example("input_batch").value.unwrap()
}
fn packed(value: &Value) -> Vec<u8> {
    rmp_serde::to_vec_named(value).unwrap()
}
#[test]
fn reference_messages_round_trip_and_broken_frames_fail() {
    let positive = fixtures::positive().unwrap();
    for f in &positive {
        assert!(fixtures::decoded_fixture(f).is_ok(), "{}", f.name);
    }
    for f in fixtures::negative(&positive).unwrap() {
        assert!(fixtures::decoded_fixture(&f).is_err(), "{}", f.name);
    }
    for name in ["max_input_batch", "max_owner_state"] {
        assert!(example(name).hex.len() / 2 <= limits::UNRELIABLE_BYTES);
    }
}
#[test]
fn serde_rejects_bad_client_types_versions_and_nonfinite_or_out_of_range_inputs() {
    for (pointer, replacement) in [
        ("/v", json!(1)),
        ("/v", json!(2)),
        ("/message/kind", json!("not_a_message")),
        ("/message/data/control_epoch", json!(-1)),
        ("/message/data/frames/0/move_x", json!(32768)),
        ("/message/data/frames/0/buttons", json!(16)),
        ("/message/data/frames/0/look_pitch", json!(20)),
        ("/message/data/frames/0/tick", json!(3.5)),
    ] {
        let mut value = input();
        *value.pointer_mut(pointer).unwrap() = replacement;
        assert!(decode_client(&packed(&value)).is_err(), "{pointer}");
    }
    let mut envelope = decode_client(&packed(&input())).unwrap();
    if let ClientMessage::InputBatch(batch) = &mut envelope.message {
        batch.frames[0].look_yaw = Yaw(f32::NAN);
    }
    // Trusted encoding is intentionally simple; incoming client data is still checked.
    assert!(decode_client(&encode_client(&envelope).unwrap()).is_err());
}
#[test]
fn client_batches_are_small_unique_and_ascending() {
    for frames in [
        json!([]),
        Value::Array(vec![input()["message"]["data"]["frames"][0].clone(); 4]),
    ] {
        let mut value = input();
        value["message"]["data"]["frames"] = frames;
        assert!(decode_client(&packed(&value)).is_err());
    }
    let mut value = input();
    value["message"]["data"]["frames"][1]["tick"] = json!(31);
    assert!(decode_client(&packed(&value)).is_err());
}
#[test]
fn serde_defaults_replace_custom_unknown_field_and_required_null_guards() {
    let mut value = input();
    value["future_field"] = json!(true);
    assert!(decode_client(&packed(&value)).is_ok());
    let mut value = example("owner_foot").value.unwrap();
    value["message"]["data"]["player_state"]
        .as_object_mut()
        .unwrap()
        .remove("vehicle_id");
    assert!(decode_server(&packed(&value), Delivery::Unreliable).is_ok());
    // Shape constraints still come from the Rust types, including fixed-size vectors.
    value["message"]["data"]["state"]["data"]["motor"]["position"] = json!([1, 2]);
    assert!(decode_server(&packed(&value), Delivery::Unreliable).is_err());
}
#[test]
fn protocol_does_not_revalidate_gameplay_relationships_on_server_output() {
    let mut value = example("owner_driving").value.unwrap();
    value["message"]["data"]["state"]["data"]["driver_id"] = json!(3);
    let typed: ServerEnvelope = serde_json::from_value(value).unwrap();
    let bytes = encode_server(&typed, Delivery::Reliable).unwrap();
    assert_eq!(decode_server(&bytes, Delivery::Reliable).unwrap(), typed);
}
#[test]
fn frame_depth_and_truncation_limits_use_the_decoder() {
    assert!(decode_client(&vec![0; limits::CLIENT_BYTES + 1]).is_err());
    assert!(decode_server(&vec![0; limits::UNRELIABLE_BYTES + 1], Delivery::Unreliable).is_err());
    assert!(decode_client(&[0xdd, 255, 255, 255, 255]).is_err());
    let mut value = input();
    let mut nested = json!(null);
    for _ in 0..64 {
        nested = json!([nested]);
    }
    value["extra"] = nested;
    assert!(decode_client(&packed(&value)).is_err());
    // An enormous declared frames array must fail through the library, without a custom scan.
    let mut declared = packed(&input());
    let field = declared.windows(6).position(|w| w == b"frames").unwrap() + 6;
    declared.splice(field..field + 1, [0xdd, 255, 255, 255, 255]);
    assert!(decode_client(&declared).is_err());
    let bytes = packed(&input());
    for end in 0..bytes.len() {
        assert!(decode_client(&bytes[..end]).is_err());
    }
    let mut seed = 0x12345678_u32;
    for length in 0..256 {
        let bytes: Vec<_> = (0..length)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        assert!(std::panic::catch_unwind(|| decode_client(&bytes)).is_ok());
    }
}

#[test]
fn tshirt_colour_is_a_bounded_rgb_integer() {
    for value in [0, 0x123abc, 0xffffff] {
        let colour: protocol::TshirtColour =
            serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(colour.0, value);
    }
    for value in [
        serde_json::json!(-1),
        serde_json::json!(16777216),
        serde_json::json!(1.5),
        serde_json::json!("#123abc"),
    ] {
        assert!(serde_json::from_value::<protocol::TshirtColour>(value).is_err());
    }
}
