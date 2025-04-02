// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong
}

pub enum ClientToServerMessage {
    Print(String),
    GetPingPong(RpcReplyPort<ActorRef<>),
}


pub enum ServerToClientMessage {
    Ask(RpcReplyPort<String>),
}