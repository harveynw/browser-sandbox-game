use crate::{Result, contract, invalid, limits::*, scan, types::*, validate};
use serde_json::Value;
use std::{
    io::{self, Write},
    sync::LazyLock,
};

static CLIENT_SCHEMA: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    jsonschema::validator_for(&contract::schema::<ClientEnvelope>()).expect("valid client schema")
});
static SERVER_SCHEMA: LazyLock<jsonschema::Validator> = LazyLock::new(|| {
    jsonschema::validator_for(&contract::schema::<ServerEnvelope>()).expect("valid server schema")
});

fn decoded_value(bytes: &[u8], limit: usize) -> Result<Value> {
    scan::scan(bytes, limit)?;
    let mut value: Value =
        rmp_serde::from_slice(bytes).map_err(|_| invalid("invalid MessagePack value"))?;
    normalize_numbers(&mut value);
    Ok(value)
}

// JS has one number type. A float marker containing an exact integer must have
// the same meaning as an integer marker, irrespective of the sender's encoder.
fn normalize_numbers(value: &mut Value) {
    match value {
        Value::Number(number) if number.is_f64() => {
            let n = number.as_f64().expect("f64 number");
            if n.fract() == 0.0 && n.abs() <= MAX_SAFE_INTEGER as f64 {
                *value = Value::from(n as i64);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(normalize_numbers),
        Value::Object(values) => values.values_mut().for_each(normalize_numbers),
        _ => {}
    }
}

pub fn decode_client(bytes: &[u8]) -> Result<ClientEnvelope> {
    let value = decoded_value(bytes, CLIENT_BYTES)?;
    if !CLIENT_SCHEMA.is_valid(&value) {
        return Err(invalid("client schema validation failed"));
    }
    let envelope: ClientEnvelope =
        serde_json::from_value(value).map_err(|_| invalid("client value validation failed"))?;
    if matches!(envelope.message, ClientMessage::InputBatch(_)) && bytes.len() > UNRELIABLE_BYTES {
        return Err(invalid("input batch byte limit"));
    }
    validate::validate_client(&envelope)?;
    Ok(envelope)
}

pub fn decode_server(bytes: &[u8], delivery: Delivery) -> Result<ServerEnvelope> {
    let limit = if delivery == Delivery::Reliable {
        RELIABLE_BYTES
    } else {
        UNRELIABLE_BYTES
    };
    let value = decoded_value(bytes, limit)?;
    if !SERVER_SCHEMA.is_valid(&value) {
        return Err(invalid("server schema validation failed"));
    }
    let envelope: ServerEnvelope =
        serde_json::from_value(value).map_err(|_| invalid("server value validation failed"))?;
    validate::validate_server(&envelope, delivery)?;
    Ok(envelope)
}

pub fn encode_client(envelope: &ClientEnvelope) -> Result<Vec<u8>> {
    let limit = if matches!(envelope.message, ClientMessage::InputBatch(_)) {
        UNRELIABLE_BYTES
    } else {
        CLIENT_BYTES
    };
    let bytes = encode_bounded(envelope, limit)?;
    decode_client(&bytes)?;
    Ok(bytes)
}

pub fn encode_server(envelope: &ServerEnvelope, delivery: Delivery) -> Result<Vec<u8>> {
    let limit = if delivery == Delivery::Reliable {
        RELIABLE_BYTES
    } else {
        UNRELIABLE_BYTES
    };
    let bytes = encode_bounded(envelope, limit)?;
    decode_server(&bytes, delivery)?;
    Ok(bytes)
}

fn encode_bounded<T: serde::Serialize>(value: &T, limit: usize) -> Result<Vec<u8>> {
    let mut buffer = LimitedBuffer {
        bytes: Vec::new(),
        limit,
    };
    // The same struct-map mode as to_vec_named, with bounded output allocation.
    value
        .serialize(&mut rmp_serde::Serializer::new(&mut buffer).with_struct_map())
        .map_err(|_| invalid("encoding failed or byte limit exceeded"))?;
    Ok(buffer.bytes)
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    limit: usize,
}
impl Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other("frame byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
