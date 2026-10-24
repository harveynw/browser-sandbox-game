//! The small input-batch invariant shared by decoding and server admission.
//! Session/phase/epoch/ownership and gameplay checks belong to server::world.
use crate::{Result, invalid, limits::MAX_INPUT_FRAMES, types::*};
pub fn validate_client(envelope: &ClientEnvelope) -> Result<()> {
    if let ClientMessage::InputBatch(batch) = &envelope.message
        && (batch.frames.is_empty()
            || batch.frames.len() > MAX_INPUT_FRAMES
            || batch.frames.windows(2).any(|p| p[0].tick >= p[1].tick))
    {
        return Err(invalid(
            "input frames must contain 1..3 unique ascending ticks",
        ));
    }
    Ok(())
}
