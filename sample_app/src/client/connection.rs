use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::{ActorRef, call_t};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use url::Url;

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    gateway::{self, NexusActorMessage, start_nexus},
    portal::PortalActorMessage,
};

pub async fn establish_connection(
    server_url: String,
) -> Result<(ActorRef<NexusActorMessage>, ActorRef<PortalActorMessage>), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // Parse the URL
    let url = Url::parse(&server_url)?;
    info!("Connecting to WebSocket server at: {}", url);

    // Start the nexus actor
    let nexus = start_nexus(None).await.unwrap();

    // Connect to the server
    let portal = connect_to_server(url, nexus.clone()).await?;

    Ok((nexus, portal))
}

async fn connect_to_server(
    url: Url,
    nexus: ActorRef<NexusActorMessage>,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error> {
    // Connect to the WebSocket server
    let (ws_stream, _) = match connect_async(url.as_str()).await {
        Ok(conn) => {
            info!("WebSocket connection established to: {}", url);
            conn
        }
        Err(e) => {
            error!("Failed to connect to WebSocket server: {}", e);
            return Err(e.into());
        }
    };

    let addr = get_peer_addr(&ws_stream).unwrap();

    // Split the WebSocket stream
    let (ws_sender, ws_receiver) = ws_stream.split();

    let ws_receiver = ws_receiver.map(|element| match element {
        Ok(msg) => {
            let msg = match msg {
                Message::Text(text) => ConduitMessage::Text(text.to_string()),
                Message::Binary(bin) => ConduitMessage::Binary(bin.into()),
                Message::Close(_) => ConduitMessage::Close(None),
                _ => ConduitMessage::Other,
            };

            Ok(msg)
        }
        Err(e) => Err(ConduitError::from(e)),
    });

    let ws_sender = ws_sender.with(|element: ConduitMessage| async {
        let msg = match element {
            ConduitMessage::Text(text) => Message::text(text),
            ConduitMessage::Binary(bin) => Message::binary(bin),
            ConduitMessage::Close(_) => Message::Close(None),
            _ => panic!("Unsupported message type"),
        };
        Ok(msg)
    });

    let ws_sender: ConduitSink = Box::pin(ws_sender);
    let ws_receiver: ConduitSource = Box::pin(ws_receiver);

    // Register the portal with the nexus actor
    let portal_identifier = format!("ws://{}", addr);
    let portal = call_t!(
        nexus,
        NexusActorMessage::Connected,
        100,
        portal_identifier,
        ws_sender
    );

    match portal {
        Ok(portal_actor) => {
            info!("Portal actor started for: {}", addr);

            let portal_actor_copy = portal_actor.clone();
            let portal_identifier = format!("ws://{}", addr);
            tokio::spawn(async move {
                gateway::receive_loop(ws_receiver, portal_identifier, portal_actor_copy).await
            });

            Ok(portal_actor)
        }
        Err(e) => {
            error!("Error starting portal actor: {}", e);
            Err(e.into())
        }
    }
}

fn get_peer_addr(ws_stream: &WebSocketStream<MaybeTlsStream<TcpStream>>) -> Option<SocketAddr> {
    // Access the inner MaybeTlsStream
    let maybe_tls_stream = ws_stream.get_ref();

    match maybe_tls_stream {
        MaybeTlsStream::Plain(tcp_stream) => {
            // If it's a plain TCP stream
            tcp_stream.peer_addr().ok()
        }
        #[cfg(feature = "tokio-tungstenite/native-tls")]
        MaybeTlsStream::NativeTls(tls_stream) => {
            // If it's a TLS stream
            tls_stream.get_ref().peer_addr().ok()
        }
        // Handle other variants based on what's available in your tokio-tungstenite version
        _ => None,
    }
}
