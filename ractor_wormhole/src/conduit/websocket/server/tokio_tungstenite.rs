//mod server_http;

use futures::{SinkExt, StreamExt, future};
use log::{error, info};
use ractor::ActorRef;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
};

use crate::conduit;

/// starts a pure websocket server (using tokio_tungstenite) on the specific bind address.
pub async fn start_server(
    nexus: ActorRef<NexusActorMessage>,
    bind: SocketAddr,
) -> Result<(), anyhow::Error> {
    // Create a TCP listener
    let listener = TcpListener::bind(&bind).await?;
    info!("WebSocket server listening on: {bind}");

    // Accept connections
    let nexus_copy = nexus.clone();
    tokio::spawn(async move {
        while let Ok((stream, addr)) = listener.accept().await {
            info!("New connection from: {addr}");
            let _ = handle_connection(stream, addr, nexus_copy.clone()).await;
        }
    });

    Ok(())
}

pub async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    nexus: ActorRef<NexusActorMessage>,
) {
    // Upgrade the TCP connection to a WebSocket connection
    let ws_stream = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws_stream) => ws_stream,
        Err(e) => {
            error!("Error during WebSocket handshake: {e}");
            return;
        }
    };

    info!("WebSocket connection established with: {addr}");

    let (tx, rx) = ws_stream.split();

    let rx = map_ws_to_conduit(rx);
    let tx = map_conduit_to_ws(tx);

    let portal_identifier = format!("ws://{addr}");
    if let Err(err) = conduit::from_sink_source(nexus, portal_identifier, tx, rx).await {
        error!("Error creating portal: {err}");
    }
}

// ---------------------------------------------------------------------------------

pub fn map_conduit_to_ws<S>(
    sink: futures::stream::SplitSink<WebSocketStream<S>, Message>,
) -> ConduitSink
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let sink = sink.with(|element: ConduitMessage| async {
        let msg = match element {
            ConduitMessage::Text(text) => Message::text(text),
            ConduitMessage::Binary(bin) => Message::binary(bin),
            ConduitMessage::Close(_) => Message::Close(None),
        };
        Ok(msg)
    });
    Box::pin(sink)
}

pub fn map_ws_to_conduit<T>(
    source: futures::stream::SplitStream<WebSocketStream<T>>,
) -> ConduitSource
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let source = source.filter_map(|element| {
        let output = match element {
            Ok(msg) => {
                let msg = match msg {
                    Message::Text(text) => Some(ConduitMessage::Text(text.to_string())),
                    Message::Binary(bin) => Some(ConduitMessage::Binary(bin.into())),
                    Message::Close(_) => Some(ConduitMessage::Close(None)),
                    _ => None,
                };

                Ok(msg)
            }
            Err(e) => Err(ConduitError::from(e)),
        };

        match output {
            Ok(Some(msg)) => future::ready(Some(Ok(msg))),
            Ok(None) => future::ready(None),
            Err(e) => future::ready(Some(Err(e))),
        }
    });
    Box::pin(source)
}
