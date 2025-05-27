#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

mod app;

use std::{sync::mpsc::Receiver, time::Duration};

#[cfg(not(target_arch = "wasm32"))]
#[cfg(target_arch = "wasm32")]
use tokio_with_wasm::alias as tokio;

use anyhow::anyhow;
use app::{UiUpdate, start_client_handler_actor};
use futures::SinkExt;
use log::info;
use ractor::ActorRef;
use ractor_wormhole::{
    nexus::{NexusActorMessage, start_nexus},
    portal::{Portal, PortalActorMessage},
    util::ActorRef_Ask,
};

pub async fn init(
    url: String,
    request_repaint_tx: tokio::sync::mpsc::Sender<()>,
) -> Result<
    (
        ActorRef<NexusActorMessage>,
        ActorRef<PortalActorMessage>,
        Receiver<UiUpdate>,
    ),
    anyhow::Error,
> {
    let nexus = start_nexus(None, None).await.unwrap();

    #[cfg(target_arch = "wasm32")]
    let portal = ractor_wormhole::conduit::websocket::client::ewebsock::connect_to_server(
        nexus.clone(),
        url,
    )
    .await
    .unwrap();

    #[cfg(not(target_arch = "wasm32"))]
    let portal = ractor_wormhole::conduit::websocket::client::tokio_tungstenite::connect_to_server(
        nexus.clone(),
        url,
    )
    .await
    .unwrap();

    let (ui_update_tx, ui_update_rx) = std::sync::mpsc::channel();

    let ui_update_tx_copy = ui_update_tx.clone();

    portal
        .wait_for_opened(Duration::from_secs(5))
        .await
        .unwrap();
    info!("WebSocket connection opened");

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

    let local_chat_client =
        start_client_handler_actor(ui_update_tx_copy, request_repaint_tx).await?;

    let (user_alias, remote_chat_server) = remote_hub
        .ask(
            |rpc| shared::HubMessage::Connect(local_chat_client, rpc),
            None,
        )
        .await?;

    ui_update_tx.send(UiUpdate::Connected(
        user_alias.clone(),
        remote_chat_server.clone(),
    ))?;

    Ok((nexus, portal, ui_update_rx))
}

#[cfg(not(target_arch = "wasm32"))]
use clap::Parser;

#[cfg(not(target_arch = "wasm32"))]
#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Server URL to connect to
    #[arg(long)]
    url: String,
}

#[cfg(not(target_arch = "wasm32"))]
async fn inner_main(rt: &tokio::runtime::Runtime) -> anyhow::Result<()> {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let cli = Cli::parse();

    color_eyre::install().map_err(|err| anyhow::anyhow!(err))?;

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([400.0, 300.0])
            .with_min_inner_size([300.0, 220.0])
            .with_icon(
                // NOTE: Adding an icon is optional
                eframe::icon_data::from_png_bytes(&include_bytes!("../assets/icon-256.png")[..])
                    .expect("Failed to load icon"),
            ),
        ..Default::default()
    };

    let (request_repaint_tx, mut request_repaint_rx) = tokio::sync::mpsc::channel(1000);

    let (nexus, portal, ui_rcv) = init(cli.url, request_repaint_tx).await.unwrap();

    eframe::run_native(
        "eframe template",
        native_options,
        Box::new(|cc| {
            let eguictx = cc.egui_ctx.clone();
            tokio::task::spawn(async move {
                while let Some(()) = request_repaint_rx.recv().await {
                    eguictx.request_repaint();
                }
            });

            Ok(Box::new(app::TemplateApp::new(cc, portal, ui_rcv)))
        }),
    )
    .map_err(|e| anyhow!("{e}"))?;

    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn inner_main() -> eframe::Result {
    use eframe::wasm_bindgen::JsCast as _;
    use ractor_wormhole::portal;

    // Redirect `log` message to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let (request_repaint_tx, mut request_repaint_rx) = tokio::sync::mpsc::channel(1000);
        let (nexus, portal, ui_rcv) = init(".".to_string(), request_repaint_tx).await.unwrap();

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| {
                    let eguictx = cc.egui_ctx.clone();
                    wasm_bindgen_futures::spawn_local(async move {
                        while let Some(()) = request_repaint_rx.recv().await {
                            eguictx.request_repaint();
                        }
                    });

                    Ok(Box::new(app::TemplateApp::new(cc, portal, ui_rcv)))
                }),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(_) => {
                    loading_text.remove();
                }
                Err(e) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {e:?}");
                }
            }
        }
    });

    Ok(())
}

// When compiling natively:
#[cfg(not(target_arch = "wasm32"))]
fn main() -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed building the Runtime");
    rt.block_on(async { inner_main(&rt).await })
}

// When compiling to web using trunk:
#[cfg(target_arch = "wasm32")]
fn main() -> eframe::Result {
    inner_main()
}
