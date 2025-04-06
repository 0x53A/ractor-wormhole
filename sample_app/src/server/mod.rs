mod alias_gen;

use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{conduit::websocket, nexus::start_nexus, portal::Portal, util::FnActor};

use crate::common::start_pingpong_actor;

pub async fn run(bind: SocketAddr) -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    // create the nexus. whenever a new portal is opened (aka a websocket-client connects),
    //  the callback will be invoked
    let nexus = start_nexus(Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    websocket::server::start_server(nexus, bind).await?;

    let pinpong = start_pingpong_actor().await?;

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        // note: the portal needs a little bit of time for the handshake
        //  todo: expose this, so the app can wait for the portal to be ready
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // new connection: publish our main actor on it
        msg.actor_ref
            .publish_named_actor("pingpong".to_string(), pinpong.clone())
            .await?;
    }

    // note: this is never reached as the server is never stopped and the loop above never terminates
    Ok(())
}
