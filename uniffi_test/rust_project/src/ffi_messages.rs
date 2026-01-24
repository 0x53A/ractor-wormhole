//! FFI-safe message enums.
//!
//! These mirror the internal Rust message types but are UniFFI-compatible.
//! Each enum variant maps 1:1 to the internal message type.

// =============================================================================
// ChatClient Messages
// =============================================================================

/// FFI-safe representation of ChatClientMessage.
///
/// This enum is exposed to Kotlin and used for both:
/// - Receiving messages via `ChatClientHandler.receive(msg)`
/// - Sending messages via `FfiChatClientActorRef.send(msg)`
#[derive(uniffi::Enum, Clone, Debug)]
pub enum FfiChatClientMessage {
    /// A new user has connected.
    UserConnected { alias: String },

    /// A user has disconnected.
    UserDisconnected { alias: String },

    /// A message was received from another user.
    MessageReceived { sender: String, content: String },

    /// The server is shutting down.
    ServerShutdown,
}

// =============================================================================
// ChatServer Messages
// =============================================================================

/// FFI-safe representation of ChatServerMessage.
#[derive(uniffi::Enum, Clone, Debug)]
pub enum FfiChatServerMessage {
    /// Post a message to the chat.
    /// The reply will be sent via the RPC mechanism.
    PostMessage { content: String },

    /// Request the list of connected users.
    /// The reply will be sent via the RPC mechanism.
    ListUsers,
}

// =============================================================================
// Hub Messages
// =============================================================================

/// FFI-safe representation of HubMessage.
#[derive(uniffi::Enum, Clone, Debug)]
pub enum FfiHubMessage {
    /// A client wants to connect.
    /// The client_actor_ref_id is the ID of the client's actor for receiving notifications.
    /// The reply will contain the assigned alias and server actor ref.
    Connect,
}

// =============================================================================
// Conversions from FFI types to internal types
// =============================================================================

use crate::messages::{ChatClientMessage, ChatMessageContent, UserAlias};

impl From<FfiChatClientMessage> for ChatClientMessage {
    fn from(msg: FfiChatClientMessage) -> Self {
        match msg {
            FfiChatClientMessage::UserConnected { alias } => {
                ChatClientMessage::UserConnected(UserAlias(alias))
            }
            FfiChatClientMessage::UserDisconnected { alias } => {
                ChatClientMessage::UserDisconnected(UserAlias(alias))
            }
            FfiChatClientMessage::MessageReceived { sender, content } => {
                ChatClientMessage::MessageReceived(
                    UserAlias(sender),
                    ChatMessageContent(content),
                )
            }
            FfiChatClientMessage::ServerShutdown => ChatClientMessage::ServerShutdown,
        }
    }
}

impl From<ChatClientMessage> for FfiChatClientMessage {
    fn from(msg: ChatClientMessage) -> Self {
        match msg {
            ChatClientMessage::UserConnected(alias) => {
                FfiChatClientMessage::UserConnected { alias: alias.0 }
            }
            ChatClientMessage::UserDisconnected(alias) => {
                FfiChatClientMessage::UserDisconnected { alias: alias.0 }
            }
            ChatClientMessage::MessageReceived(sender, content) => {
                FfiChatClientMessage::MessageReceived {
                    sender: sender.0,
                    content: content.0,
                }
            }
            ChatClientMessage::ServerShutdown => FfiChatClientMessage::ServerShutdown,
        }
    }
}
