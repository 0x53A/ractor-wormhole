use core::panic;
use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::{ActorRef, call_t};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{accept_async, tungstenite::protocol::Message};

use ractor_wormhole::gateway::{
    self, OnActorConnectedMessage, RawMessage, WSNexusMessage, start_nexus,
};

pub async fn start_server(
    bind: SocketAddr,
    on_client_connected: ActorRef<OnActorConnectedMessage>,
) -> Result<ActorRef<WSNexusMessage>, anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // Create a TCP listener
    let listener = TcpListener::bind(&bind).await?;
    info!("WebSocket server listening on: {}", bind);

    let nexus = start_nexus(Some(on_client_connected)).await.unwrap();

    // Accept connections
    let nexus_copy = nexus.clone();
    tokio::spawn(async move {
        while let Ok((stream, addr)) = listener.accept().await {
            info!("New connection from: {}", addr);
            // Handle each connection in a separate task
            tokio::spawn(handle_connection(stream, addr, nexus_copy.clone()));
        }
    });

    Ok(nexus)
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    actor_ref: ActorRef<WSNexusMessage>,
) {
    // Upgrade the TCP connection to a WebSocket connection
    let ws_stream = match accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(e) => {
            error!("Error during WebSocket handshake: {}", e);
            return;
        }
    };

    info!("WebSocket connection established with: {}", addr);

    let (ws_sender, ws_receiver) = ws_stream.split();

    let ws_receiver = ws_receiver.map(|element| match element {
        Ok(msg) => {
            let msg = match msg {
                Message::Text(text) => gateway::RawMessage::Text(text.to_string()),
                Message::Binary(bin) => gateway::RawMessage::Binary(bin.into()),
                Message::Close(_) => gateway::RawMessage::Close(None),
                _ => gateway::RawMessage::Other,
            };

            Ok(msg)
        }
        Err(e) => Err(gateway::RawError::from(e)),
    });

    let ws_sender = ws_sender.with(|element: RawMessage| async {
        let msg = match element {
            RawMessage::Text(text) => Message::text(text),
            RawMessage::Binary(bin) => Message::binary(bin),
            RawMessage::Close(_) => Message::Close(None),
            _ => panic!("Unsupported message type"),
        };
        Ok(msg)
    });

    let ws_sender: gateway::WebSocketSink = Box::pin(ws_sender);
    let ws_receiver: gateway::WebSocketSource = Box::pin(ws_receiver);

    let portal_identifier = format!("ws://{}", addr);
    let portal = call_t!(
        actor_ref,
        WSNexusMessage::Connected,
        100,
        portal_identifier.clone(),
        ws_sender
    );

    match portal {
        Ok(portal_actor) => {
            info!("Portal actor started for: {}", addr);
            gateway::receive_loop(ws_receiver, portal_identifier, portal_actor).await
        }
        Err(e) => error!("Error starting portal actor: {}", e),
    }
}
