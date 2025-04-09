use clap::Parser;
use ractor::ActorRef;
use shared::HubMessage;
use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{
    nexus::start_nexus,
    portal::{NexusResult, Portal, PortalActorMessage},
    util::{ActorRef_Ask, ActorRef_Map, FnActor},
};


/// spawn a new hub actor, which receives a handle to the one and only chat server, and handle to the specific portal
pub async fn spawn_hub(
    chat_server: ActorRef<crate::chat_server::Msg>,
    portal: ActorRef<PortalActorMessage>,
) -> NexusResult<ActorRef<HubMessage>> {
    let (actor_ref, _) = FnActor::<HubMessage>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                HubMessage::Connect(client_actor_ref, rpc_reply_port) => {
                    let new_user_alias = chat_server
                        .ask(
                            |rpc| crate::chat_server::Msg::Connect(portal.clone(), client_actor_ref, rpc),
                            None,
                        )
                        .await
                        .unwrap();

                    // build a new proxy actor for the client
                    let portal_copy = portal.clone();
                    let (proxy, _) = chat_server
                        .clone()
                        .map(move |msg| crate::chat_server::Msg::Public(portal_copy.clone(), msg))
                        .await
                        .unwrap();

                    let _ = rpc_reply_port.send((new_user_alias, proxy));
                }
            }
        }
    })
    .await?;

    Ok(actor_ref)
}