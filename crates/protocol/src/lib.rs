//! Version 1 shared wire contract. Decoding validates shape and stateless rules;
//! callers must also validate session context before applying client intent.

#[cfg(feature = "codec")]
pub mod codec;
#[cfg(feature = "codec")]
pub mod contract;
#[cfg(feature = "codec")]
pub mod fixtures;
pub mod limits;
pub mod scan;
pub mod types;
pub mod validate;

#[cfg(feature = "codec")]
pub use codec::{decode_client, decode_server, encode_client, encode_server};
pub use types::*;
pub use validate::{ClientContext, Phase, validate_client_context};

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
