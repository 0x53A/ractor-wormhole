use futures::{SinkExt, StreamExt, future};
use log::{error, info};
use ractor::ActorRef;
use tokio_tungstenite::{
    WebSocketStream, connect_async, tungstenite::client::IntoClientRequest,
    tungstenite::protocol::Message,
};

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
};

use crate::{conduit, util::ActorRef_Ask};

pub async fn connect_to_server<R>(
    nexus: ActorRef<NexusActorMessage>,
    request: R,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error>
where
    R: IntoClientRequest + Unpin,
{
    let r = request.into_client_request()?;
    let uri = r.uri().clone();
    info!("Connecting to WebSocket server at: {uri}");

    // Connect to the WebSocket server
    let (ws_stream, _) = match connect_async(r).await {
        Ok(conn) => {
            info!("WebSocket connection established to: {uri}");
            conn
        }
        Err(e) => {
            error!("Failed to connect to WebSocket server at {uri}: {e}");
            return Err(e.into());
        }
    };

    // Split the WebSocket stream
    let (tx, rx) = ws_stream.split();

    // map the tungstenite messages to ConduitMessage
    let rx = map_ws_to_conduit(rx);

    // map the ConduitMessage to tungstenite messages
    let tx = map_conduit_to_ws(tx);

    // Register the portal with the nexus actor
    let portal_identifier = uri.to_string();
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), tx, rpc),
            None,
        )
        .await?;

    info!("Portal actor started for: {uri}");

    let portal_actor_copy = portal.clone();
    ractor::concurrency::spawn(async move {
        conduit::receive_loop(rx, portal_identifier, portal_actor_copy).await
    });

    Ok(portal)
}

// ---------------------------------------------------------------------------------

/// Maps a WebSocket stream to a ConduitSource by transforming WebSocket Messages to ConduitMessages
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
                    Message::Close(Some(reason)) => Some(ConduitMessage::Close(Some(format!(
                        "Close code: {:?}, reason: {}",
                        reason.code, reason.reason
                    )))),
                    Message::Close(None) => Some(ConduitMessage::Close(None)),
                    _unhandled => None,
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

/// Maps a WebSocket sink to a ConduitSink by transforming ConduitMessages to WebSocket Messages
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
