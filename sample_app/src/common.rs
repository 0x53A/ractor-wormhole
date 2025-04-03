// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use ractor::{ActorRef, RpcReplyPort};
use ractor_cluster_derive::RactorMessage;

#[derive(Debug, RactorMessage)]
pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong,
}

#[derive(Debug, RactorMessage)]
pub enum ClientToServerMessage {
    Print(String),
    GetPingPong(RpcReplyPort<ActorRef<PingPongMsg>>),
}

#[derive(Debug, RactorMessage)]
pub enum ServerToClientMessage {
    Ask(RpcReplyPort<String>),
}
