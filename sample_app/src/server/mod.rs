mod alias_gen;
mod connection;

use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{nexus::start_nexus, portal::UserFriendlyPortal, util::FnActor};

use crate::common::{ClientToServerMessage, start_pingpong_actor};

pub async fn run(bind: SocketAddr) -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    let nexus = start_nexus(Some(ctx_on_client_connected.actor_ref.clone())).await.map_err(|err| anyhow!(err)) ?;

    connection::start_server(nexus, bind).await?;

    let pinpong = start_pingpong_actor().await?;

    let pingpong_box = Box::new(pinpong.clone());

    // create a local actor and publish it on the connection
    let (local_actor, _) = FnActor::<ClientToServerMessage>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                ClientToServerMessage::GetPingPong(rpc_reply_port) => {
                    let _ = rpc_reply_port.send(*pingpong_box.clone());
                }
                ClientToServerMessage::Print(text) => {
                    println!("Received text from Client: {}", text)
                }
            }
        }
    })
    .await?;

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // new connection, publish our main actor on it
        msg.actor_ref
            .publish_named_actor("root".to_string(), local_actor.clone())
            .await?;
    }

    Ok(())
}
