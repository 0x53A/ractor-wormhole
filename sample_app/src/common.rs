// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::WormholeTransmaterializable;
use ractor_wormhole::{portal::NexusResult, util::FnActor};

// ----------------------------------------------------------------------------------

#[derive(Debug, WormholeTransmaterializable)]
pub enum PingPongMsg {
    Ping(ActorRef<PingPongMsg>),
    Pong(ActorRef<PingPongMsg>),
}

// ----------------------------------------------------------------------------------

pub async fn start_pingpong_actor() -> NexusResult<ActorRef<PingPongMsg>> {
    let (local_pinpong, _) = FnActor::<PingPongMsg>::start_fn(async |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                PingPongMsg::Ping(rpc_reply_port) => {
                    println!("Received ping, sending pong");
                    let self_ref = ctx.actor_ref.clone();
                    rpc_reply_port
                        .send_after(Duration::from_millis(500), || PingPongMsg::Pong(self_ref));
                }
                PingPongMsg::Pong(rpc_reply_port) => {
                    println!("Received pong, sending ping");
                    let self_ref = ctx.actor_ref.clone();
                    rpc_reply_port
                        .send_after(Duration::from_millis(500), || PingPongMsg::Ping(self_ref));
                }
            }
        }
    })
    .await?;

    Ok(local_pinpong)
}
