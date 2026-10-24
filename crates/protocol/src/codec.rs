//! MessagePack named maps on output; direct Serde decoding on input.
use crate::{Result, invalid, limits::*, types::*, validate};
use serde::{Serialize, de::DeserializeOwned};
use std::io::Cursor;

fn decode<T: DeserializeOwned>(bytes: &[u8], limit: usize) -> Result<T> {
    check_size(bytes, limit)?;
    let mut decoder = rmp_serde::Deserializer::new(Cursor::new(bytes));
    decoder.set_max_depth(MAX_DEPTH);
    let value = T::deserialize(&mut decoder).map_err(|_| invalid("invalid MessagePack message"))?;
    if decoder.position() != bytes.len() as u64 {
        return Err(invalid("trailing MessagePack data"));
    }
    Ok(value)
}
fn check_size(bytes: &[u8], limit: usize) -> Result<()> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err(invalid("empty or oversized frame"));
    }
    Ok(())
}
fn encode(value: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let bytes =
        rmp_serde::to_vec_named(value).map_err(|_| invalid("MessagePack encoding failed"))?;
    check_size(&bytes, limit)?;
    Ok(bytes)
}
pub fn decode_client(bytes: &[u8]) -> Result<ClientEnvelope> {
    let envelope: ClientEnvelope = decode(bytes, CLIENT_BYTES)?;
    if matches!(envelope.message, ClientMessage::InputBatch(_)) {
        check_size(bytes, UNRELIABLE_BYTES)?;
    }
    validate::validate_client(&envelope)?;
    Ok(envelope)
}
pub fn decode_server(bytes: &[u8], delivery: Delivery) -> Result<ServerEnvelope> {
    decode(bytes, server_limit(delivery))
}
pub fn encode_client(envelope: &ClientEnvelope) -> Result<Vec<u8>> {
    encode(
        envelope,
        if matches!(envelope.message, ClientMessage::InputBatch(_)) {
            UNRELIABLE_BYTES
        } else {
            CLIENT_BYTES
        },
    )
}
pub fn encode_server(envelope: &ServerEnvelope, delivery: Delivery) -> Result<Vec<u8>> {
    encode(envelope, server_limit(delivery))
}
fn server_limit(delivery: Delivery) -> usize {
    if delivery == Delivery::Reliable {
        RELIABLE_BYTES
    } else {
        UNRELIABLE_BYTES
    }
}
