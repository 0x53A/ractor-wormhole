#![feature(fn_traits)]
#![feature(try_find)]
#![feature(never_type)]

mod alias_gen;
mod chat_server;
mod http_server;
mod hub;

use clap::Parser;
use std::net::SocketAddr;

use anyhow::anyhow;
use ractor_wormhole::{nexus::start_nexus, portal::Portal, util::FnActor};

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

async fn run() -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let cli = Cli::parse();

    // already start the actual chat server actor
    let chat_server = chat_server::start_chatserver_actor().await?;

    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    // create the nexus. whenever a new portal is opened (aka a websocket-client connects),
    //  the callback will be invoked
    let nexus = start_nexus(Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    // Start the HTTP server
    println!("Starting server, binding to: {}", cli.bind);
    let nexus_clone = nexus.clone();
    tokio::spawn(async move {
        http_server::http_server_fn(nexus_clone, cli.bind)
            .await
            .unwrap();
    });

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        // whenever a client connects, create a new proxy actor for it, and publish it
        let hub_actor = hub::spawn_hub(chat_server.clone(), msg.actor_ref.clone()).await?;

        // new connection: publish our main actor on it
        msg.actor_ref
            .publish_named_actor("hub".to_string(), hub_actor.clone())
            .await?;
    }

    Ok(())
}
