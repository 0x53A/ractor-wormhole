// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use ractor::{ActorRef, RpcReplyPort};
use ractor_cluster_derive::RactorMessage;
use ractor_wormhole_derive::WormholeSerializable;

#[derive(Debug, Clone, WormholeSerializable)]
pub struct UserAlias(String);
#[derive(Debug, Clone, WormholeSerializable)]
pub struct ChatMessage(String);

#[derive(Debug, RactorMessage, WormholeSerializable)]
pub enum ChatServerMessage {
    PostMessage(ChatMessage),
}

#[derive(Debug, RactorMessage, WormholeSerializable)]
pub enum ChatClientMessage {
    AssignAlias(String),
    MessageReceived(UserAlias, ChatMessage),
}

// ----------------------------------------------------------------------------------

#[derive(Debug, RactorMessage, WormholeSerializable)]
pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong,
}

#[derive(Debug, RactorMessage, WormholeSerializable)]
pub enum ClientToServerMessage {
    Print(String),
    GetPingPong(RpcReplyPort<ActorRef<PingPongMsg>>),
}

#[derive(Debug, RactorMessage, WormholeSerializable)]
pub enum ServerToClientMessage {
    Ask(RpcReplyPort<String>),
}
