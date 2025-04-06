#![feature(fn_traits)]
#![feature(try_find)]

mod alias_gen;
mod chat_server;

use clap::Parser;
use ractor::ActorRef;
use shared::HubMessage;
use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{
    conduit::websocket,
    nexus::start_nexus,
    portal::{NexusResult, Portal, PortalActorMessage},
    util::{ActorRef_Ask, ActorRef_Map, FnActor},
};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address to bind to (e.g. 127.0.0.1:8080)
    #[arg(long)]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {:#}", e); // Pretty format with all causes
        std::process::exit(1);
    }
}

/// spawn a new hub actor, which receives a handle to the one and only chat server, and handle to the specific portal
async fn spawn_hub(
    chat_server: ActorRef<chat_server::Msg>,
    portal: ActorRef<PortalActorMessage>,
) -> NexusResult<ActorRef<HubMessage>> {
    let (actor_ref, _) = FnActor::<HubMessage>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                HubMessage::Connect(client_actor_ref, rpc_reply_port) => {
                    let new_user_alias = chat_server
                        .ask(
                            |rpc| chat_server::Msg::Connect(portal.clone(), client_actor_ref, rpc),
                            None,
                        )
                        .await
                        .unwrap();

                    // build a new proxy actor for the client
                    let portal_copy = portal.clone();
                    let (proxy, _) = chat_server
                        .clone()
                        .map(move |msg| chat_server::Msg::Public(portal_copy.clone(), msg))
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

async fn run() -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let cli = Cli::parse();

    // already start the actual chat server actor
    let chat_server = chat_server::start_chatserver_actor().await?;

    println!("Starting server, binding to: {}", cli.bind);

    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    // create the nexus. whenever a new portal is opened (aka a websocket-client connects),
    //  the callback will be invoked
    let nexus = start_nexus(Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    websocket::server::start_server(nexus, cli.bind).await?;

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        // whenever a client connects, create a new proxy actor for it, and publish it
        let hub_actor = spawn_hub(chat_server.clone(), msg.actor_ref.clone()).await?;

        // new connection: publish our main actor on it
        msg.actor_ref
            .publish_named_actor("hub".to_string(), hub_actor.clone())
            .await?;
    }

    Ok(())
}
