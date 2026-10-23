//! Executable reference examples, not a second definition of the wire types.
use crate::{
    Result, decode_client, decode_server, encode_client, encode_server, invalid, types::*,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub name: String,
    pub direction: String,
    pub delivery: Delivery,
    pub valid: bool,
    pub hex: String,
    pub value: Option<Value>,
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
        "client" => {
            serde_json::to_value(decode_client(&bytes)?).map_err(|_| invalid("fixture JSON"))
        }
        "server" => serde_json::to_value(decode_server(&bytes, fixture.delivery)?)
            .map_err(|_| invalid("fixture JSON")),
        _ => Err(invalid("invalid fixture direction")),
    }
}

fn envelope(message: Value, pre_auth: bool) -> Value {
    json!({"v": 2, "world_id": "world-0001",
        "session_id": if pre_auth { Value::Null } else { json!("session-0001") },
        "message": message })
}

fn tagged(kind: &str, data: Value) -> Value {
    json!({"kind":kind, "data":data})
}
fn character(mode: &str) -> Value {
    json!({"motor":{"position":[1.0,0.0,2.0], "velocity":[0.0,0.0,-4.0],
        "yaw":0.5, "grounded":true, "jump_cooldown_ticks":0},
        "health":100, "mode":mode,
        "vehicle_id":if mode == "driving" { json!(2) } else { Value::Null }})
}
fn vehicle() -> Value {
    json!({"motor":{"position":[1.0,0.0,2.0],"yaw":0.5,"speed":12.0,
        "steering":0.25,"velocity":[0.0,0.0,-12.0]},
        "health":160,"driver_id":1,"disabled":false})
}
fn owner(driving: bool) -> Value {
    let character = character(if driving { "driving" } else { "on_foot" });
    json!({"server_tick":30, "consumed_through_tick":30, "control_epoch":1,
        "player_id":1, "controlled_entity_id":if driving {2} else {1},
        "state":if driving {tagged("vehicle",vehicle())} else {tagged("character",character.clone())},
        "player_state":character})
}
fn input(tick: u32) -> Value {
    json!({"tick":tick,"move_x":32767,"move_z":-32767,"look_yaw":0.5,
        "look_pitch":-0.5,"buttons":15})
}
fn status() -> Value {
    json!({"revision":1,"server_tick":30,"credits":150,"completed_jobs":1,
        "heat":0,"pursuit":"clear","magazine":12,"reload_end_tick":null,
        "mission":null,"repair":null,"respawn_tick":null,"protected_until_tick":null})
}

pub fn positive() -> Result<Vec<Fixture>> {
    let mut result = Vec::new();
    let mut push =
        |name: &str, direction: &str, kind: &str, data: Value, delivery: Delivery| -> Result<()> {
            let value = envelope(
                tagged(kind, data),
                matches!(kind, "client_hello" | "server_hello" | "reject"),
            );
            let (bytes, canonical) = if direction == "client" {
                let typed = serde_json::from_value(value)
                    .map_err(|e| invalid(format!("fixture {name}: {e}")))?;
                let bytes = encode_client(&typed)?;
                (bytes, serde_json::to_value(typed).expect("fixture JSON"))
            } else {
                let typed = serde_json::from_value(value)
                    .map_err(|e| invalid(format!("fixture {name}: {e}")))?;
                let bytes = encode_server(&typed, delivery)?;
                (bytes, serde_json::to_value(typed).expect("fixture JSON"))
            };
            result.push(Fixture {
                name: name.into(),
                direction: direction.into(),
                delivery,
                valid: true,
                hex: to_hex(&bytes),
                value: Some(canonical),
            });
            Ok(())
        };
    let r = Delivery::Reliable;
    let u = Delivery::Unreliable;
    let hash = "a".repeat(64);
    push(
        "client_hello",
        "client",
        "client_hello",
        json!({"protocol_version":2,
        "build_id":"test-v2","content_hash":hash,"sim_config_hash":hash,
        "join_ticket":"b".repeat(64),"display_name":"Player 🐈"}),
        r,
    )?;
    push(
        "client_ready",
        "client",
        "client_ready",
        json!({"baseline_id":1}),
        r,
    )?;
    push(
        "input_batch",
        "client",
        "input_batch",
        json!({"control_epoch":1,
        "frames":[input(31),input(32),input(33)]}),
        u,
    )?;
    for (kind, data) in [
        ("enter_vehicle", json!({"vehicle_id":2})),
        ("exit_vehicle", json!({})),
        ("fire", json!({"aim_yaw":0.5,"aim_pitch":-0.5})),
        ("reload", json!({})),
        (
            "accept_mission",
            json!({"template_id":"courier_quay_market"}),
        ),
        ("cancel_mission", json!({})),
        ("repair", json!({"zone_id":"repair_ironworks"})),
    ] {
        push(
            &format!("action_{kind}"),
            "client",
            "action",
            json!({"request_id":1,
            "control_epoch":1,"action":tagged(kind,data)}),
            r,
        )?;
    }
    push(
        "ping",
        "client",
        "ping",
        json!({"nonce":u32::MAX,"client_time_ms":1234.5}),
        r,
    )?;
    for reason in ["stale", "overflow", "invalid_state"] {
        push(
            &format!("resync_{reason}"),
            "client",
            "resync_request",
            json!({"reason":reason}),
            r,
        )?;
    }
    push("leave", "client", "leave", json!({}), r)?;
    push(
        "server_hello",
        "server",
        "server_hello",
        json!({"protocol_version":2,"build_id":"test-v2",
        "content_hash":hash,"sim_config_hash":hash,"tick_rate":30,"snapshot_rate":10}),
        r,
    )?;
    for code in [
        "version_mismatch",
        "content_mismatch",
        "invalid_ticket",
        "server_full",
        "invalid_name",
        "unavailable",
    ] {
        push(
            &format!("reject_{code}"),
            "server",
            "reject",
            json!({"code":code,"detail":"Unable to join"}),
            r,
        )?;
    }
    push(
        "welcome",
        "server",
        "welcome",
        json!({"player_id":1,"server_tick":30,"input_lead_ticks":3,
        "profile":{"profile_id":"profile-0001","display_name":"Player 🐈","credits":150,"completed_jobs":1}}),
        r,
    )?;
    push(
        "baseline_begin",
        "server",
        "baseline_begin",
        json!({"baseline_id":1,"server_tick":30}),
        r,
    )?;
    for (entity_kind, state) in [
        ("player", tagged("character", character("on_foot"))),
        ("vehicle", tagged("vehicle", vehicle())),
        ("pedestrian", tagged("character", character("on_foot"))),
        ("police", tagged("character", character("on_foot"))),
        ("police_car", tagged("vehicle", vehicle())),
    ] {
        push(
            &format!("show_{entity_kind}"),
            "server",
            "entity_show",
            json!({"baseline_id":null,
            "visibility_id":1,"descriptor":{"id":2,
            "entity_kind":if entity_kind == "police_car" {"police"} else {entity_kind},
            "template_id":"compact","display_name":null},"state":state,"state_tick":30}),
            r,
        )?;
    }
    push(
        "baseline_end",
        "server",
        "baseline_end",
        json!({"baseline_id":1,"entity_count":96}),
        r,
    )?;
    push(
        "start_play",
        "server",
        "start_play",
        json!({"baseline_id":1,"owner":owner(false)}),
        r,
    )?;
    for reason in ["out_of_range", "destroyed", "disconnected"] {
        push(
            &format!("hide_{reason}"),
            "server",
            "entity_hide",
            json!({"entity_id":2,"visibility_id":1,"server_tick":30,"reason":reason}),
            r,
        )?;
    }
    for mode in ["on_foot", "driving", "downed", "arrested"] {
        push(
            &format!("snapshot_{mode}"),
            "server",
            "snapshot",
            json!({"server_tick":30,"records":[{
            "entity_id":1,"visibility_id":1,"state":tagged("character",character(mode))}]}),
            u,
        )?;
    }
    push(
        "snapshot_vehicle",
        "server",
        "snapshot",
        json!({"server_tick":30,"records":[{
        "entity_id":2,"visibility_id":1,"state":tagged("vehicle",vehicle())}]}),
        u,
    )?;
    push("owner_foot", "server", "owner_state", owner(false), u)?;
    push("owner_driving", "server", "owner_state", owner(true), u)?;
    push(
        "control_changed",
        "server",
        "control_changed",
        owner(true),
        r,
    )?;
    for code in [
        "ok",
        "not_ready",
        "stale_epoch",
        "stale_request",
        "invalid_target",
        "out_of_range",
        "occupied",
        "too_fast",
        "exit_blocked",
        "cooldown",
        "no_ammo",
        "dead",
        "unavailable",
        "insufficient_funds",
        "already_active",
        "not_active",
        "pursuit_active",
    ] {
        push(
            &format!("result_{code}"),
            "server",
            "action_result",
            json!({"request_id":1,
            "applied_tick":30,"ok":code=="ok","code":code,"affected_entity_id":null}),
            r,
        )?;
    }
    for phase in [
        "dwell",
        "debit_pending",
        "refund_pending",
        "completed",
        "cancelled",
        "failed",
    ] {
        let mut value = status();
        value["repair"] = json!({"vehicle_id":2,"zone_id":"repair_ironworks","phase":phase,"price":50,"dwell_ticks":60,"code":if phase=="failed" {"insufficient_funds"} else {"ok"}});
        push(
            &format!("repair_{phase}"),
            "server",
            "player_status",
            value,
            r,
        )?;
    }
    push("status_clear", "server", "player_status", status(), r)?;
    for pursuit in ["pursuing", "searching", "arresting"] {
        let mut value = status();
        value["heat"] = json!(3);
        value["pursuit"] = json!(pursuit);
        push(
            &format!("status_{pursuit}"),
            "server",
            "player_status",
            value,
            r,
        )?;
    }
    for phase in ["active", "completed", "failed"] {
        let mut value = status();
        value["reload_end_tick"] = json!(45);
        value["respawn_tick"] = json!(60);
        value["protected_until_tick"] = json!(90);
        value["mission"] = json!({"instance_id":"mission-0001","template_id":"courier_quay_market",
            "phase":phase,"checkpoint_index":u16::MAX,"deadline_tick":u32::MAX,
            "target_entity_id":2,"target_position":[1.0,0.0,2.0]});
        push(
            &format!("mission_{phase}"),
            "server",
            "player_status",
            value,
            r,
        )?;
    }
    push(
        "event_shot",
        "server",
        "world_event",
        tagged(
            "shot",
            json!({"event_id":1,"server_tick":30,
        "shooter_id":1,"origin":[1.0,1.5,2.0],"end":[1.0,1.5,-78.0],"hit_entity_id":2}),
        ),
        r,
    )?;
    push(
        "event_impact",
        "server",
        "world_event",
        tagged(
            "impact",
            json!({"event_id":1,"server_tick":30,
        "entity_id":2,"position":[1.0,0.0,2.0],"severity":1.0}),
        ),
        u,
    )?;
    push(
        "pong",
        "server",
        "pong",
        json!({"nonce":u32::MAX,"client_time_ms":1234.5,
        "server_tick":u32::MAX,"server_time_ms":crate::limits::MAX_SAFE_INTEGER}),
        r,
    )?;
    push(
        "pong_rtc",
        "server",
        "pong",
        json!({"nonce":u32::MAX,"client_time_ms":1234.5,
        "server_tick":u32::MAX,"server_time_ms":crate::limits::MAX_SAFE_INTEGER}),
        u,
    )?;
    push(
        "disconnect",
        "server",
        "disconnect",
        json!({"code":"shutdown","detail":"Server stopped"}),
        r,
    )?;
    for (base_name, name) in [
        ("input_batch", "max_input_batch"),
        ("owner_driving", "max_owner_state"),
    ] {
        let base = result
            .iter()
            .find(|f| f.name == base_name)
            .expect("boundary base")
            .clone();
        let mut value = base.value.expect("fixture value");
        value["world_id"] = json!("w".repeat(64));
        value["session_id"] = json!("s".repeat(64));
        let data = &mut value["message"]["data"];
        if base_name == "input_batch" {
            data["control_epoch"] = json!(u32::MAX);
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
            data["server_tick"] = json!(u32::MAX);
            data["consumed_through_tick"] = json!(u32::MAX);
            data["control_epoch"] = json!(u32::MAX);
            data["player_id"] = json!(u32::MAX);
            data["controlled_entity_id"] = json!(u32::MAX - 1);
            data["player_state"]["vehicle_id"] = json!(u32::MAX - 1);
            data["state"]["data"]["driver_id"] = json!(u32::MAX);
            for motor in ["/player_state/motor", "/state/data/motor"] {
                let m = data.pointer_mut(motor).expect("motor");
                m["position"] = json!([f32::MAX, -f32::MAX, f32::MAX]);
                m["velocity"] = json!([-f32::MAX, f32::MAX, -f32::MAX]);
                m["yaw"] = json!(std::f32::consts::PI);
            }
            data["state"]["data"]["motor"]["speed"] = json!(-f32::MAX);
            data["state"]["data"]["motor"]["steering"] = json!(0.55_f32);
            data["player_state"]["motor"]["jump_cooldown_ticks"] = json!(u16::MAX);
        }
        let bytes = if base.direction == "client" {
            encode_client(
                &serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?,
            )?
        } else {
            encode_server(
                &serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?,
                u,
            )?
        };
        result.push(Fixture {
            name: name.into(),
            direction: base.direction,
            delivery: u,
            valid: true,
            hex: to_hex(&bytes),
            value: Some(value),
        });
    }
    Ok(result)
}

pub fn negative(positive: &[Fixture]) -> Result<Vec<Fixture>> {
    let mut result = Vec::new();
    let base = positive
        .iter()
        .find(|f| f.name == "input_batch")
        .expect("input fixture");
    let value = base.value.clone().expect("fixture value");
    let mut add = |name: &str, bytes: Vec<u8>| {
        result.push(Fixture {
            name: name.into(),
            direction: "client".into(),
            delivery: Delivery::Reliable,
            valid: false,
            hex: to_hex(&bytes),
            value: None,
        });
    };
    let mut mutations: Vec<(&str, Value)> = Vec::new();
    macro_rules! mutation {
        ($name:literal, $path:expr, $replacement:expr) => {{
            let mut v = value.clone();
            *v.pointer_mut($path).expect("fixture path") = $replacement;
            mutations.push(($name, v));
        }};
    }
    mutation!("wrong_version", "/v", json!(3));
    mutation!("old_version", "/v", json!(1));
    mutation!("unknown_kind", "/message/kind", json!("teleport"));
    mutation!("zero_epoch", "/message/data/control_epoch", json!(0));
    mutation!(
        "axis_overflow",
        "/message/data/frames/0/move_x",
        json!(32768)
    );
    mutation!(
        "axis_negative_endpoint",
        "/message/data/frames/0/move_x",
        json!(-32768)
    );
    mutation!(
        "reserved_buttons",
        "/message/data/frames/0/buttons",
        json!(16)
    );
    mutation!("bad_yaw", "/message/data/frames/0/look_yaw", json!(4));
    mutation!("bad_pitch", "/message/data/frames/0/look_pitch", json!(2));
    mutation!(
        "tick_overflow",
        "/message/data/frames/0/tick",
        json!(4_294_967_296_u64)
    );
    mutation!("tick_fraction", "/message/data/frames/0/tick", json!(1.5));
    mutation!("wrong_scope", "/session_id", Value::Null);
    mutation!("empty_frames", "/message/data/frames", json!([]));
    mutation!("duplicate_tick", "/message/data/frames/1/tick", json!(31));
    mutation!("unsorted_tick", "/message/data/frames/1/tick", json!(1));
    let mut missing = value.clone();
    missing
        .as_object_mut()
        .expect("object")
        .remove("session_id");
    mutations.push(("missing_nullable", missing));
    let mut extra = value.clone();
    extra["extra"] = json!(1);
    mutations.push(("unknown_field", extra));
    let mut enum_extra = value.clone();
    enum_extra["message"]["extra"] = json!(1);
    mutations.push(("unknown_enum_field", enum_extra));
    let mut four = value.clone();
    four["message"]["data"]["frames"] = json!([input(31), input(32), input(33), input(34)]);
    mutations.push(("four_inputs", four));
    for (name, value) in mutations {
        add(
            name,
            rmp_serde::to_vec_named(&value).map_err(|_| invalid("fixture encode"))?,
        );
    }
    let mut trailing = from_hex(&base.hex)?;
    trailing.push(0xc0);
    add("trailing", trailing);
    for (name, bytes) in [
        ("empty", vec![]),
        ("reserved_marker", vec![0xc1]),
        ("truncated", vec![0x81, 0xa1, b'v']),
        ("duplicate_keys", vec![0x82, 0xa1, b'v', 1, 0xa1, b'v', 1]),
        ("invalid_utf8", vec![0xa1, 0xff]),
        ("binary", vec![0xc4, 1, 0]),
        ("extension", vec![0xd4, 0, 0]),
        ("huge_array", vec![0xdd, 0xff, 0xff, 0xff, 0xff]),
        ("huge_map", vec![0xdf, 0xff, 0xff, 0xff, 0xff]),
        ("huge_string", vec![0xdb, 0xff, 0xff, 0xff, 0xff]),
        ("non_string_key", vec![0x81, 1, 1]),
        ("nan", vec![0xca, 0x7f, 0xc0, 0, 0]),
        ("infinity", vec![0xcb, 0x7f, 0xf0, 0, 0, 0, 0, 0, 0]),
        ("unsafe_integer", vec![0xcf, 0, 0x20, 0, 0, 0, 0, 0, 0]),
        ("deep", {
            let mut v = vec![0x91; 18];
            v.push(0xc0);
            v
        }),
        ("oversized", vec![0; crate::limits::CLIENT_BYTES + 1]),
        (
            "positional_struct",
            rmp_serde::to_vec_named(&json!([
                1, "world-0001", "session-0001", {"kind":"leave","data":{}}
            ]))
            .map_err(|_| invalid("fixture encode"))?,
        ),
    ] {
        add(name, bytes);
    }
    for (base_name, name, path, replacement) in [
        (
            "client_hello",
            "invalid_hash",
            "/message/data/content_hash",
            json!("A".repeat(64)),
        ),
        (
            "client_hello",
            "short_name",
            "/message/data/display_name",
            json!("ab"),
        ),
        (
            "client_hello",
            "control_name",
            "/message/data/display_name",
            json!("A\nB"),
        ),
        (
            "client_hello",
            "untrimmed_name",
            "/message/data/display_name",
            json!(" Bob"),
        ),
        (
            "client_hello",
            "long_ticket",
            "/message/data/join_ticket",
            json!("a".repeat(65)),
        ),
        (
            "action_exit_vehicle",
            "nonempty_action",
            "/message/data/action/data",
            json!({"x":1}),
        ),
        (
            "owner_foot",
            "checkpoint_mismatch",
            "/message/data/consumed_through_tick",
            json!(29),
        ),
        (
            "owner_foot",
            "missing_owner_link",
            "/message/data/player_state/vehicle_id",
            json!(2),
        ),
        (
            "owner_foot",
            "wrong_tuple_length",
            "/message/data/state/data/motor/position",
            json!([1, 2]),
        ),
        (
            "owner_foot",
            "character_health_overflow",
            "/message/data/player_state/health",
            json!(101),
        ),
        (
            "owner_driving",
            "wrong_driver",
            "/message/data/state/data/driver_id",
            json!(3),
        ),
        (
            "owner_driving",
            "vehicle_health_overflow",
            "/message/data/state/data/health",
            json!(161),
        ),
        (
            "owner_driving",
            "disabled_driver",
            "/message/data/state/data/disabled",
            json!(true),
        ),
        (
            "show_player",
            "descriptor_mismatch",
            "/message/data/descriptor/entity_kind",
            json!("vehicle"),
        ),
        (
            "repair_dwell",
            "repair_price",
            "/message/data/repair/price",
            json!(51),
        ),
        (
            "repair_dwell",
            "repair_dwell_overflow",
            "/message/data/repair/dwell_ticks",
            json!(61),
        ),
        (
            "status_clear",
            "cash_overflow",
            "/message/data/credits",
            json!(1_000_000_001_u32),
        ),
        (
            "status_clear",
            "magazine_overflow",
            "/message/data/magazine",
            json!(13),
        ),
        (
            "status_clear",
            "heat_overflow",
            "/message/data/heat",
            json!(4),
        ),
        (
            "status_clear",
            "pursuit_mismatch",
            "/message/data/pursuit",
            json!("pursuing"),
        ),
        (
            "result_ok",
            "result_mismatch",
            "/message/data/ok",
            json!(false),
        ),
        (
            "event_impact",
            "severity_overflow",
            "/message/data/data/severity",
            json!(1.5),
        ),
        (
            "server_hello",
            "wrong_tick_rate",
            "/message/data/tick_rate",
            json!(60),
        ),
        (
            "welcome",
            "zero_entity",
            "/message/data/player_id",
            json!(0),
        ),
        (
            "welcome",
            "input_lead_overflow",
            "/message/data/input_lead_ticks",
            json!(7),
        ),
        (
            "baseline_end",
            "baseline_count_overflow",
            "/message/data/entity_count",
            json!(97),
        ),
        (
            "pong",
            "unsafe_time",
            "/message/data/server_time_ms",
            json!(9_007_199_254_740_992_u64),
        ),
        (
            "snapshot_vehicle",
            "duplicate_snapshot",
            "/message/data/records",
            {
                let record =
                    json!({"entity_id":2,"visibility_id":1,"state":tagged("vehicle",vehicle())});
                json!([record, record])
            },
        ),
    ] {
        let base = positive
            .iter()
            .find(|f| f.name == base_name)
            .expect("mutation base");
        let mut value = base.value.clone().expect("value");
        *value.pointer_mut(path).expect("mutation path") = replacement;
        result.push(Fixture {
            name: name.into(),
            direction: base.direction.clone(),
            delivery: base.delivery,
            valid: false,
            hex: to_hex(&rmp_serde::to_vec_named(&value).map_err(|_| invalid("fixture encode"))?),
            value: None,
        });
    }
    let base = positive
        .iter()
        .find(|f| f.name == "owner_foot")
        .expect("owner");
    let mut missing = base.value.clone().expect("value");
    missing["message"]["data"]["player_state"]
        .as_object_mut()
        .expect("state")
        .remove("vehicle_id");
    result.push(Fixture {
        name: "missing_nested_null".into(),
        direction: "server".into(),
        delivery: Delivery::Unreliable,
        valid: false,
        hex: to_hex(&rmp_serde::to_vec_named(&missing).map_err(|_| invalid("fixture encode"))?),
        value: None,
    });
    let base = positive
        .iter()
        .find(|f| f.name == "control_changed")
        .expect("control");
    result.push(Fixture {
        name: "unreliable_control_change".into(),
        direction: "server".into(),
        delivery: Delivery::Unreliable,
        valid: false,
        hex: base.hex.clone(),
        value: None,
    });
    Ok(result)
}
