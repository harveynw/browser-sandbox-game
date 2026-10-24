//! Version 3 wire types and a small Serde/MessagePack codec.
//! The server owns session admission and authoritative gameplay checks.

#[cfg(feature = "codec")]
pub mod codec;
#[cfg(feature = "codec")]
pub mod contract;
#[cfg(feature = "codec")]
pub mod fixtures;
pub mod limits;
pub mod types;
pub mod validate;

#[cfg(feature = "codec")]
pub use codec::{decode_client, decode_server, encode_client, encode_server};
pub use types::*;

/// A bounded, non-sensitive error. Raw payloads are never included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError(pub String);

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ProtocolError {}

pub type Result<T> = std::result::Result<T, ProtocolError>;

pub(crate) fn invalid(message: impl Into<String>) -> ProtocolError {
    ProtocolError(message.into())
}
