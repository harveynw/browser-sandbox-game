//! A small set of cross-language examples. JSON is only the readable test manifest.
use crate::{
    Result, decode_client, decode_server, encode_client, encode_server, invalid, types::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fixture {
    pub name: String,
    pub direction: String,
    pub delivery: Delivery,
    pub valid: bool,
    pub hex: String,
    pub value: Option<Value>,
}
pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn from_hex(value: &str) -> Result<Vec<u8>> {
    if !value.len().is_multiple_of(2) || !value.is_ascii() {
        return Err(invalid("invalid fixture hex"));
    }
    (0..value.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&value[i..i + 2], 16).map_err(|_| invalid("invalid fixture hex"))
        })
        .collect()
}
pub fn decoded_fixture(fixture: &Fixture) -> Result<Value> {
    let bytes = from_hex(&fixture.hex)?;
    match fixture.direction.as_str() {
        "client" => serde_json::to_value(decode_client(&bytes)?),
        "server" => serde_json::to_value(decode_server(&bytes, fixture.delivery)?),
        _ => return Err(invalid("invalid fixture direction")),
    }
    .map_err(|_| invalid("fixture JSON"))
}
fn tagged(kind: &str, data: Value) -> Value {
    json!({"kind":kind,"data":data})
}
fn character(driving: bool) -> Value {
    json!({"motor":{"position":[1.0,0.0,2.0],"velocity":[0.0,0.0,-4.0],"yaw":0.5,"grounded":true,"jump_cooldown_ticks":0},"health":100,"mode":if driving{"driving"}else{"on_foot"},"vehicle_id":if driving{json!(2)}else{Value::Null}})
}
fn owner(driving: bool) -> Value {
    let player = character(driving);
    let state = if driving {
        tagged(
            "vehicle",
            json!({"motor":{"position":[1.0,0.0,2.0],"velocity":[0.0,0.0,-12.0],"yaw":0.5,"speed":12.0,"steering":0.25},"health":160,"driver_id":1,"disabled":false}),
        )
    } else {
        tagged("character", player.clone())
    };
    json!({"server_tick":30,"consumed_through_tick":30,"control_epoch":1,"player_id":1,"controlled_entity_id":if driving{2}else{1},"state":state,"player_state":player})
}
pub fn positive() -> Result<Vec<Fixture>> {
    let hash = "a".repeat(64);
    let frames:Vec<_>=(31..=33).map(|tick|json!({"tick":tick,"move_x":32767,"move_z":-32767,"look_yaw":0.5,"look_pitch":-0.5,"buttons":15})).collect();
    let r = Delivery::Reliable;
    let u = Delivery::Unreliable;
    let examples = vec![
        (
            "chat_send",
            "client",
            r,
            tagged("chat", json!("Hello *world*!")),
        ),
        (
            "chat_line",
            "server",
            r,
            tagged(
                "chat",
                json!({"id":1,"username":"Guest One","text":"Hello _everyone_!","server_tick":30}),
            ),
        ),
        (
            "chat_error",
            "server",
            r,
            tagged(
                "chat_error",
                json!("Please wait a second between messages."),
            ),
        ),
        (
            "client_hello",
            "client",
            r,
            tagged(
                "client_hello",
                json!({"protocol_version":3,"build_id":"test-v2","content_hash":hash,"sim_config_hash":hash,"join_ticket":"b".repeat(64),"display_name":"Player 🐈"}),
            ),
        ),
        (
            "client_ready",
            "client",
            r,
            tagged("client_ready", json!({"baseline_id":1})),
        ),
        (
            "input_batch",
            "client",
            u,
            tagged("input_batch", json!({"control_epoch":1,"frames":frames})),
        ),
        (
            "action_exit_vehicle",
            "client",
            r,
            tagged(
                "action",
                json!({"request_id":1,"control_epoch":1,"action":tagged("exit_vehicle",json!({}))}),
            ),
        ),
        (
            "action_fire",
            "client",
            r,
            tagged(
                "action",
                json!({"request_id":2,"control_epoch":1,"action":tagged("fire",json!({"aim_yaw":0.5,"aim_pitch":-0.5}))}),
            ),
        ),
        (
            "ping",
            "client",
            u,
            tagged("ping", json!({"nonce":u32::MAX,"client_time_ms":1234.5})),
        ),
        ("leave", "client", r, tagged("leave", json!({}))),
        (
            "owner_foot",
            "server",
            u,
            tagged("owner_state", owner(false)),
        ),
        (
            "owner_driving",
            "server",
            u,
            tagged("owner_state", owner(true)),
        ),
        (
            "control_changed",
            "server",
            r,
            tagged("control_changed", owner(true)),
        ),
        (
            "snapshot",
            "server",
            u,
            tagged(
                "snapshot",
                json!({"server_tick":30,"records":[{"entity_id":1,"visibility_id":1,"state":tagged("character",character(false))}]}),
            ),
        ),
        (
            "player_status",
            "server",
            r,
            tagged(
                "player_status",
                json!({"revision":1,"server_tick":30,"credits":150,"completed_jobs":1,"heat":0,"pursuit":"clear","magazine":12,"reload_end_tick":null,"mission":null,"repair":null,"respawn_tick":null,"protected_until_tick":null}),
            ),
        ),
    ];
    let mut fixtures = Vec::new();
    for (name, direction, delivery, message) in examples {
        let value = json!({"v":3,"world_id":"world-0001","session_id":if name=="client_hello"{Value::Null}else{json!("session-0001")},"message":message});
        let (bytes, value) = if direction == "client" {
            let typed: ClientEnvelope =
                serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
            (
                encode_client(&typed)?,
                serde_json::to_value(typed).expect("fixture JSON"),
            )
        } else {
            let typed: ServerEnvelope =
                serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
            (
                encode_server(&typed, delivery)?,
                serde_json::to_value(typed).expect("fixture JSON"),
            )
        };
        fixtures.push(Fixture {
            name: name.into(),
            direction: direction.into(),
            delivery,
            valid: true,
            hex: to_hex(&bytes),
            value: Some(value),
        });
    }
    // Worst-case IDs/ticks and float widths must still fit the RTC packet budget.
    for (base, name) in [
        ("input_batch", "max_input_batch"),
        ("owner_driving", "max_owner_state"),
    ] {
        let mut fixture = fixtures
            .iter()
            .find(|f| f.name == base)
            .expect("fixture")
            .clone();
        fixture.name = name.into();
        let value = fixture.value.as_mut().expect("value");
        value["world_id"] = json!("w".repeat(64));
        value["session_id"] = json!("s".repeat(64));
        let data = &mut value["message"]["data"];
        data["control_epoch"] = json!(u32::MAX);
        if base == "input_batch" {
            for (i, frame) in data["frames"]
                .as_array_mut()
                .expect("frames")
                .iter_mut()
                .enumerate()
            {
                frame["tick"] = json!(u32::MAX - 2 + i as u32);
                frame["look_yaw"] = json!(std::f32::consts::PI);
                frame["look_pitch"] = json!(1.2_f32);
            }
        } else {
            for field in [
                "server_tick",
                "consumed_through_tick",
                "player_id",
                "controlled_entity_id",
            ] {
                data[field] = json!(u32::MAX);
            }
            data["controlled_entity_id"] = json!(u32::MAX - 1);
            data["player_state"]["vehicle_id"] = json!(u32::MAX - 1);
            data["state"]["data"]["driver_id"] = json!(u32::MAX);
            for pointer in ["/state/data/motor", "/player_state/motor"] {
                let motor = data.pointer_mut(pointer).expect("motor");
                motor["position"] = json!([f32::MAX, -f32::MAX, f32::MAX]);
                motor["velocity"] = json!([-f32::MAX, f32::MAX, -f32::MAX]);
                motor["yaw"] = json!(std::f32::consts::PI);
            }
        }
        let bytes = if base == "input_batch" {
            encode_client(
                &serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?,
            )?
        } else {
            encode_server(
                &serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?,
                u,
            )?
        };
        fixture.hex = to_hex(&bytes);
        fixtures.push(fixture);
    }
    Ok(fixtures)
}
pub fn negative(positive: &[Fixture]) -> Result<Vec<Fixture>> {
    let base = positive
        .iter()
        .find(|f| f.name == "input_batch")
        .expect("input fixture");
    let bytes = from_hex(&base.hex)?;
    let mut trailing = bytes.clone();
    trailing.push(0xc0);
    let mut wrong_version = base.value.clone().expect("value");
    wrong_version["v"] = json!(2);
    [
        ("empty", vec![]),
        ("truncated", bytes[..bytes.len() - 1].to_vec()),
        ("trailing", trailing),
        (
            "wrong_version",
            rmp_serde::to_vec_named(&wrong_version).map_err(|_| invalid("fixture encoding"))?,
        ),
    ]
    .into_iter()
    .map(|(name, bytes)| {
        Ok(Fixture {
            name: name.into(),
            valid: false,
            hex: to_hex(&bytes),
            value: None,
            ..base.clone()
        })
    })
    .collect()
}
