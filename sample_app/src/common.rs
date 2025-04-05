// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use ractor::{ActorRef, RpcReplyPort};
use ractor_cluster_derive::RactorMessage;
use ractor_wormhole_derive::WormholeTransmaterializable;

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UserAlias(String);
#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct ChatMessage(String);

#[derive(Debug, RactorMessage, WormholeTransmaterializable)]
pub enum ChatServerMessage {
    PostMessage(ChatMessage),
}

#[derive(Debug, RactorMessage, WormholeTransmaterializable)]
pub enum ChatClientMessage {
    AssignAlias(String),
    MessageReceived(UserAlias, ChatMessage),
}

// ----------------------------------------------------------------------------------

#[derive(Debug, RactorMessage, WormholeTransmaterializable)]
pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong,
}

#[derive(Debug, RactorMessage, WormholeTransmaterializable)]
pub enum ClientToServerMessage {
    Print(String),
    GetPingPong(RpcReplyPort<ActorRef<PingPongMsg>>),
}

#[derive(Debug, RactorMessage, WormholeTransmaterializable)]
pub enum ServerToClientMessage {
    Ask(RpcReplyPort<String>),
}
