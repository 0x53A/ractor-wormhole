#![feature(fn_traits)]
#![feature(try_find)]

use clap::Parser;
use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::{
    conduit::websocket,
    nexus::start_nexus,
    portal::{NexusResult, Portal, PortalActorMessage},
    util::{ActorRef_Ask, FnActor},
};
use shared::ChatClientMessage;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Server URL to connect to
    #[arg(long)]
    url: String,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {:#}", e); // Pretty format with all causes
        std::process::exit(1);
    }
}

pub async fn start_chatclient_actor() -> NexusResult<ActorRef<ChatClientMessage>> {
    let (actor_ref, _) = FnActor::<ChatClientMessage>::start_fn(async |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                ChatClientMessage::MessageReceived(user_alias, chat_msg) => {
                    println!(
                        "Message from {}: {}",
                        user_alias.to_string(),
                        chat_msg.to_string()
                    );
                }
                ChatClientMessage::UserConnected(user_alias) => {
                    println!("User connected: {}", user_alias.to_string());
                }
            }
        }
    })
    .await?;

    Ok(actor_ref)
}

async fn run() -> Result<(), anyhow::Error> {
    // // Initialize logger
    // env_logger::init_from_env(
    //     env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    // );

    let cli = Cli::parse();

    // Start the nexus actor
    let nexus = start_nexus(None).await.unwrap();

    // connect to the server
    let portal = websocket::client::connect_to_server(nexus, cli.url).await?;

    // wait for the portal to be ready (handshake)
    tokio::time::sleep(Duration::from_secs(1)).await;

    // the server has published a named actor
    let remote_hub_address = portal
        .ask(
            |rpc| PortalActorMessage::QueryNamedRemoteActor("hub".to_string(), rpc),
            None,
        )
        .await??;

    let remote_hub: ActorRef<shared::HubMessage> = portal
        .instantiate_proxy_for_remote_actor(remote_hub_address)
        .await?;

    let local_chat_client = start_chatclient_actor().await?;
    let (user_alias, remote_chat_server) = remote_hub
        .ask(
            |rpc| shared::HubMessage::Connect(local_chat_client, rpc),
            None,
        )
        .await?;

    println!("Connected to server as: {}", user_alias.to_string());

    loop {
        let mut line_buf = String::new();
        let _ = std::io::stdin().read_line(&mut line_buf)?;
        let () = remote_chat_server
            .ask(
                |rpc| {
                    shared::ChatServerMessage::PostMessage(
                        shared::ChatMessage(line_buf.clone()),
                        rpc,
                    )
                },
                None,
            )
            .await?;
    }

    Ok(())
}
