//! Handler traits that Kotlin implements.
//!
//! Each actor type has a corresponding Handler trait with a single `receive` method.
//! This makes the interface mechanical and easy to generate with proc macros.
//!
//! Pattern:
//! ```ignore
//! trait {ActorType}Handler {
//!     fn receive(&self, msg: Ffi{ActorType}Message);
//! }
//! ```

use std::sync::Arc;

use crate::ffi_messages::FfiChatClientMessage;
use crate::ffi_rpc::{FfiAckReplyPort, FfiConnectReplyPort, FfiUserListReplyPort};

// =============================================================================
// Transport Callbacks (unchanged - not actor-based)
// =============================================================================

/// FFI-safe representation of conduit messages.
#[derive(uniffi::Enum, Clone, Debug)]
pub enum FfiConduitMessage {
    /// Text message (JSON for handshake).
    Text { content: String },
    /// Binary message (serialized actor messages).
    Binary { data: Vec<u8> },
    /// Connection close.
    Close { reason: Option<String> },
}

impl From<ractor_wormhole::conduit::ConduitMessage> for FfiConduitMessage {
    fn from(msg: ractor_wormhole::conduit::ConduitMessage) -> Self {
        use ractor_wormhole::conduit::ConduitMessage;
        match msg {
            ConduitMessage::Text(s) => FfiConduitMessage::Text { content: s },
            ConduitMessage::Binary(data) => FfiConduitMessage::Binary { data },
            ConduitMessage::Close(reason) => FfiConduitMessage::Close { reason },
        }
    }
}

impl From<FfiConduitMessage> for ractor_wormhole::conduit::ConduitMessage {
    fn from(msg: FfiConduitMessage) -> Self {
        use ractor_wormhole::conduit::ConduitMessage;
        match msg {
            FfiConduitMessage::Text { content } => ConduitMessage::Text(content),
            FfiConduitMessage::Binary { data } => ConduitMessage::Binary(data),
            FfiConduitMessage::Close { reason } => ConduitMessage::Close(reason),
        }
    }
}

/// Callback interface for the transport layer.
#[uniffi::export(callback_interface)]
pub trait TransportCallback: Send + Sync {
    /// Called when Rust needs to send a message to the remote peer.
    fn send_message(&self, message: FfiConduitMessage);

    /// Called when the connection should be closed.
    fn close(&self);
}

/// Callback for connection lifecycle events.
#[uniffi::export(callback_interface)]
pub trait ConnectionCallback: Send + Sync {
    fn on_handshake_complete(&self);
    fn on_closed(&self, reason: Option<String>);
    fn on_error(&self, error: String);
}

// =============================================================================
// Actor Handler Traits
// =============================================================================

/// Handler for ChatClient messages.
///
/// Implement this to receive messages sent to a ChatClient actor.
/// This is the uniform pattern: one `receive` method per handler.
#[uniffi::export(callback_interface)]
pub trait ChatClientHandler: Send + Sync {
    /// Called when a message arrives for this actor.
    fn receive(&self, msg: FfiChatClientMessage);
}

/// Handler for ChatServer messages.
///
/// Implement this to handle messages sent to a ChatServer actor.
/// Note: RPC reply ports are passed separately since they require async handling.
#[uniffi::export(callback_interface)]
pub trait ChatServerHandler: Send + Sync {
    /// Called when a PostMessage request arrives.
    /// Call `reply_port.ack()` to acknowledge.
    fn receive_post_message(&self, content: String, reply_port: Arc<FfiAckReplyPort>);

    /// Called when a ListUsers request arrives.
    /// Call `reply_port.reply(users)` to respond.
    fn receive_list_users(&self, reply_port: Arc<FfiUserListReplyPort>);
}

/// Handler for Hub messages.
///
/// Implement this to handle client connection requests.
#[uniffi::export(callback_interface)]
pub trait HubHandler: Send + Sync {
    /// Called when a client wants to connect.
    ///
    /// - `client_actor`: Reference to the client's actor (for sending notifications)
    /// - `reply_port`: Call `reply(alias, server_actor)` to complete the connection
    fn receive_connect(
        &self,
        client_actor: Arc<crate::ffi_actors::FfiChatClientActorRef>,
        reply_port: Arc<FfiConnectReplyPort>,
    );
}
