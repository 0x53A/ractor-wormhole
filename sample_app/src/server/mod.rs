use std::net::SocketAddr;

use anyhow::anyhow;
use ractor::concurrency::Duration;
use ractor_wormhole::{
    conduit::websocket,
    nexus::start_nexus,
    portal::{Portal, PortalActorMessage},
    util::{ActorRef_Ask, FnActor},
};

use crate::common::start_pingpong_actor;

pub async fn run(bind: SocketAddr) -> Result<(), anyhow::Error> {
    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    // create the nexus. whenever a new portal is opened (aka a websocket-client connects),
    //  the callback will be invoked
    let nexus = start_nexus(None, Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    websocket::server::tokio_tungstenite::start_server(nexus, bind).await?;

    let pinpong = start_pingpong_actor().await?;

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        msg.actor_ref
            .ask(
                PortalActorMessage::WaitForHandshake,
                Some(Duration::from_secs(5)),
            )
            .await?;

        // new connection: publish our main actor on it
        msg.actor_ref
            .publish_named_actor("pingpong".to_string(), pinpong.clone())
            .await?;
    }

    // note: this is never reached as the server is never stopped and the loop above never terminates
    Ok(())
}
