use futures::{SinkExt, StreamExt, future};
use log::{error, info};
use ractor::{ActorRef, call_t};
use tokio_tungstenite::{
    connect_async, tungstenite::client::IntoClientRequest, tungstenite::protocol::Message,
};

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
};

use crate::conduit;

pub async fn connect_to_server<R>(
    nexus: ActorRef<NexusActorMessage>,
    request: R,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error>
where
    R: IntoClientRequest + Unpin,
{
    let r = request.into_client_request()?;
    let uri = r.uri().clone();
    info!("Connecting to WebSocket server at: {}", uri);

    // Connect to the WebSocket server
    let (ws_stream, _) = match connect_async(r).await {
        Ok(conn) => {
            info!("WebSocket connection established to: {}", uri);
            conn
        }
        Err(e) => {
            error!("Failed to connect to WebSocket server at {}: {}", uri, e);
            return Err(e.into());
        }
    };

    // Split the WebSocket stream
    let (tx, rx) = ws_stream.split();

    // map the tungstenite messages to ConduitMessage
    let rx = rx.filter_map(|element| {
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
    let rx: ConduitSource = Box::pin(rx);

    // map the ConduitMessage to tungstenite messages
    let tx = tx.with(|element: ConduitMessage| async {
        let msg = match element {
            ConduitMessage::Text(text) => Message::text(text),
            ConduitMessage::Binary(bin) => Message::binary(bin),
            ConduitMessage::Close(_) => Message::Close(None),
        };
        Ok(msg)
    });
    let tx: ConduitSink = Box::pin(tx);

    // Register the portal with the nexus actor
    let portal_identifier = uri.to_string();
    let portal = call_t!(
        nexus,
        NexusActorMessage::Connected,
        100,
        portal_identifier.clone(),
        tx
    )?;

    info!("Portal actor started for: {}", uri);

    let portal_actor_copy = portal.clone();
    tokio::spawn(
        async move { conduit::receive_loop(rx, portal_identifier, portal_actor_copy).await },
    );

    Ok(portal)
}
