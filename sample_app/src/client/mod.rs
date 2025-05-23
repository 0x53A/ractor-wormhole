use ractor::ActorRef;
use ractor_wormhole::{
    conduit::websocket,
    nexus::start_nexus,
    portal::{Portal, PortalActorMessage},
    util::ActorRef_Ask,
};
use std::time::Duration;
use tokio::time;

use crate::common::{PingPongMsg, start_pingpong_actor};

pub async fn run(server_url: String) -> Result<(), anyhow::Error> {
    // Start the nexus actor
    let nexus = start_nexus(None, None).await.unwrap();

    // connect to the server
    let portal = websocket::client::tokio_tungstenite::connect_to_server(nexus, server_url).await?;

    // wait for the portal to be ready (handshake)
    portal
        .ask(
            PortalActorMessage::WaitForHandshake,
            Some(Duration::from_secs(5)),
        )
        .await?;

    // the server has published a named actor
    let remote_pingpong_actor_id = portal
        .ask(
            |rpc| PortalActorMessage::QueryNamedRemoteActor("pingpong".to_string(), rpc),
            None,
        )
        .await??;

    let remote_pingpong: ActorRef<PingPongMsg> = portal
        .instantiate_proxy_for_remote_actor(remote_pingpong_actor_id)
        .await?;

    let local_pingpong = start_pingpong_actor().await?;

    remote_pingpong.send_message(PingPongMsg::Ping(local_pingpong.clone()))?;

    println!("Sent ping to remote pingpong actor");

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}
