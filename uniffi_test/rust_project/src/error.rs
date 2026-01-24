//! Error types for the UniFFI bindings.

/// Errors that can occur in the wormhole FFI layer.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WormholeError {
    #[error("Runtime not initialized")]
    RuntimeNotInitialized,

    #[error("Connection failed: {reason}")]
    ConnectionFailed { reason: String },

    #[error("Handshake timeout")]
    HandshakeTimeout,

    #[error("Actor not found: {name}")]
    ActorNotFound { name: String },

    #[error("Send failed: {reason}")]
    SendFailed { reason: String },

    #[error("Serialization error: {reason}")]
    SerializationError { reason: String },

    #[error("Internal error: {reason}")]
    InternalError { reason: String },

    #[error("Reply port already consumed")]
    ReplyPortConsumed,

    #[error("Actor already stopped")]
    ActorStopped,
}

impl From<anyhow::Error> for WormholeError {
    fn from(err: anyhow::Error) -> Self {
        WormholeError::InternalError {
            reason: err.to_string(),
        }
    }
}

impl From<ractor::MessagingErr<()>> for WormholeError {
    fn from(err: ractor::MessagingErr<()>) -> Self {
        WormholeError::SendFailed {
            reason: err.to_string(),
        }
    }
}
