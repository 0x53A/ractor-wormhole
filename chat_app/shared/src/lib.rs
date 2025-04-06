use ractor::{ActorRef, RpcReplyPort};
use ractor_wormhole::WormholeTransmaterializable;

// ----------------------------------------------------------------------------------
// These messages define the protocol between the client and server.
// ----------------------------------------------------------------------------------

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UserAlias(pub String);

impl From<UserAlias> for String {
    fn from(val: UserAlias) -> Self {
        val.0
    }
}

impl UserAlias {
    pub fn to_string(&self) -> String {
        self.0.clone()
    }
}

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct ChatMessage(pub String);

impl From<ChatMessage> for String {
    fn from(val: ChatMessage) -> Self {
        val.0
    }
}

impl ChatMessage {
    pub fn to_string(&self) -> String {
        self.0.clone()
    }
}

// ----------------------------------------------------------------------------------

// The server initially provides the "hub" where a client can register.

#[derive(Debug, WormholeTransmaterializable)]
pub enum HubMessage {
    Connect(
        ActorRef<ChatClientMessage>,
        /* note: if you wanted to implement authentication,
        you would pass some kind of user information in here */
        RpcReplyPort<(UserAlias, ActorRef<ChatServerMessage>)>,
    ),
}

// ----------------------------------------------------------------------------------

// The "hub" responds and returns a reference to the actual chat server.

/// messages sent to the server from the client
#[derive(Debug, WormholeTransmaterializable)]
pub enum ChatServerMessage {
    /// post a message to the server,
    ///  the unit-reply port is so the client can wait for the server to acknowledge the message
    PostMessage(ChatMessage, RpcReplyPort<()>),
}

// ----------------------------------------------------------------------------------

// The client posts a handle to itself to the server,
//  so that the server can push messages to the client.

/// Messages sent to the client from the server
#[derive(Debug, WormholeTransmaterializable)]
pub enum ChatClientMessage {
    /// a different user has connected
    UserConnected(UserAlias),
    /// a user has sent a message
    MessageReceived(UserAlias, ChatMessage),
}
