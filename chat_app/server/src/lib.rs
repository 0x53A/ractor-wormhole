#![feature(never_type)]

pub mod alias_gen;
pub mod chat_server;
pub mod http_server;
pub mod hub;

use anyhow::anyhow;
use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::{
    nexus::start_nexus,
    portal::{Portal, PortalActorMessage},
    util::{ActorRef_Ask, FnActor},
};
use std::net::SocketAddr;

pub async fn run_chat_server(bind: SocketAddr) -> Result<(), anyhow::Error> {
    let chat_server = chat_server::start_chatserver_actor().await?;

    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    let nexus = start_nexus(None, Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    println!("Starting server, binding to: {bind}");
    let nexus_clone = nexus.clone();
    tokio::spawn(async move {
        http_server::http_server_fn(nexus_clone, bind)
            .await
            .unwrap();
    });

    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        let result = handle_connected_client(&chat_server, msg).await;
        if let Err(err) = result {
            eprintln!("Error handling connected client: {err:#}");
        }
    }

    Ok(())
}

async fn handle_connected_client(
    chat_server: &ActorRef<chat_server::Msg>,
    msg: ractor_wormhole::nexus::OnActorConnectedMessage,
) -> Result<(), anyhow::Error> {
    let hub_actor = hub::spawn_hub(chat_server.clone(), msg.actor_ref.clone()).await?;

    msg.actor_ref
        .publish_named_actor("hub".to_string(), hub_actor.clone())
        .await?;

    msg.actor_ref
        .ask(
            PortalActorMessage::WaitForHandshake,
            Some(Duration::from_secs(5)),
        )
        .await?;

    Ok(())
}
