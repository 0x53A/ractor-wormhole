#![feature(fn_traits)]
#![feature(try_find)]
#![feature(let_chains)]

mod ui;

use clap::Parser;
use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::{
    conduit::websocket,
    nexus::start_nexus,
    portal::{NexusResult, Portal, PortalActorMessage},
    util::{ActorRef_Ask, FnActor},
};
use shared::ChatClientMessage;
use ui::{UIMsg, spawn_ui_actor};

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

pub async fn start_chatclient_actor(
    ui: ActorRef<UIMsg>,
) -> NexusResult<ActorRef<ChatClientMessage>> {
    let (actor_ref, _) = FnActor::<ChatClientMessage>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                ChatClientMessage::MessageReceived(user_alias, chat_msg) => {
                    ui.send_message(UIMsg::AddChatMessage(user_alias, chat_msg))
                        .unwrap();
                }
                ChatClientMessage::UserConnected(user_alias) => {
                    ui.send_message(UIMsg::UserConnected(user_alias)).unwrap();
                }
                ChatClientMessage::Disconnect => {
                    ui.send_message(UIMsg::Disconnected).unwrap();
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

    color_eyre::install().map_err(|err| anyhow::anyhow!(err))?;
    let terminal = ratatui::init();

    let ui = spawn_ui_actor(terminal).await;

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

    let local_chat_client = start_chatclient_actor(ui.clone()).await?;
    let (user_alias, remote_chat_server) = remote_hub
        .ask(
            |rpc| shared::HubMessage::Connect(local_chat_client, rpc),
            None,
        )
        .await?;

    ui.send_message(UIMsg::Connected(
        user_alias.clone(),
        remote_chat_server.clone(),
    ))?;

    // wait for ui to exit
    ui.wait(None).await?;

    ratatui::restore();

    Ok(())
}
