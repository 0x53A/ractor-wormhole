// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use ractor::{ActorRef, RpcReplyPort};

pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong,
}

pub enum ClientToServerMessage {
    Print(String),
    GetPingPong(RpcReplyPort<ActorRef<PingPongMsg>>),
}

pub enum ServerToClientMessage {
    Ask(RpcReplyPort<String>),
}
