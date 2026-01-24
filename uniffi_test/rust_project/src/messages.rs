//! Message types for the chat protocol.
//!
//! These types define the actor message protocol. They use ractor's ActorRef and RpcReplyPort
//! generics on the Rust side, which get wrapped into concrete FFI types at the boundary.

use ractor::{ActorRef, RpcReplyPort};
use ractor_wormhole::WormholeTransmaterializable;

// =============================================================================
// Simple Value Types
// =============================================================================

/// A user's display name/alias.
#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UserAlias(pub String);

impl From<String> for UserAlias {
    fn from(s: String) -> Self {
        UserAlias(s)
    }
}

impl From<UserAlias> for String {
    fn from(val: UserAlias) -> Self {
        val.0
    }
}

/// A chat message's content.
#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct ChatMessageContent(pub String);

impl From<String> for ChatMessageContent {
    fn from(s: String) -> Self {
        ChatMessageContent(s)
    }
}

impl From<ChatMessageContent> for String {
    fn from(val: ChatMessageContent) -> Self {
        val.0
    }
}

// =============================================================================
// Actor Message Types
// =============================================================================

/// Messages sent to the Hub actor.
///
/// The Hub is the entry point - clients connect here first and receive
/// a reference to their dedicated chat server actor.
#[derive(Debug, WormholeTransmaterializable)]
pub enum HubMessage {
    /// A client wants to connect.
    ///
    /// - `ActorRef<ChatClientMessage>`: The client's actor for receiving notifications
    /// - `RpcReplyPort<...>`: Reply channel to send back the user alias and server actor ref
    Connect(
        ActorRef<ChatClientMessage>,
        RpcReplyPort<(UserAlias, ActorRef<ChatServerMessage>)>,
    ),
}

/// Messages sent to the ChatServer actor (from client to server).
#[derive(Debug, WormholeTransmaterializable)]
pub enum ChatServerMessage {
    /// Post a message to the chat.
    ///
    /// - `ChatMessageContent`: The message text
    /// - `RpcReplyPort<()>`: Acknowledgment that the message was received
    PostMessage(ChatMessageContent, RpcReplyPort<()>),

    /// Request the list of connected users.
    ListUsers(RpcReplyPort<Vec<UserAlias>>),
}

/// Messages sent to the ChatClient actor (from server to client).
#[derive(Debug, Clone, WormholeTransmaterializable)]
pub enum ChatClientMessage {
    /// A new user has connected.
    UserConnected(UserAlias),

    /// A user has disconnected.
    UserDisconnected(UserAlias),

    /// A message was received from another user.
    /// Fields: (sender, content)
    MessageReceived(UserAlias, ChatMessageContent),

    /// The server is shutting down, client should disconnect.
    ServerShutdown,
}

// =============================================================================
// Type Aliases for Complex Generic Types
// =============================================================================
// These make it easier to reference the specific instantiations we need to wrap.

/// The reply type for a Connect request.
pub type ConnectReply = (UserAlias, ActorRef<ChatServerMessage>);

/// RpcReplyPort for Connect - used when a client connects to the hub.
pub type ConnectReplyPort = RpcReplyPort<ConnectReply>;

/// RpcReplyPort for acknowledgments (unit type).
pub type AckReplyPort = RpcReplyPort<()>;

/// RpcReplyPort for user list queries.
pub type UserListReplyPort = RpcReplyPort<Vec<UserAlias>>;
