use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::{ActorRef, call_t};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpStream, time};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use url::Url;

use ractor_wormhole::gateway::{self, RawMessage, WSGatewayMessage, start_gateway};

pub async fn run(server_url: String) -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // Parse the URL
    let url = Url::parse(&server_url)?;
    info!("Connecting to WebSocket server at: {}", url);

    // Start the gateway actor
    let gateway = start_gateway(None).await.unwrap();

    // Connect to the server
    connect_to_server(url, gateway.clone()).await?;

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}

async fn connect_to_server(
    url: Url,
    gateway: ActorRef<WSGatewayMessage>,
) -> Result<(), Box<dyn std::error::Error>> {
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

    // Register the connection with the gateway actor
    let connection = call_t!(gateway, WSGatewayMessage::Connected, 100, addr, ws_sender);

    match connection {
        Ok(connection_actor) => {
            info!("Connection actor started for: {}", addr);

            gateway::receive_loop(ws_receiver, addr, connection_actor).await
        }
        Err(e) => error!("Error starting connection actor: {}", e),
    }

    Ok(())
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
