use futures::{SinkExt, StreamExt, future};
use log::{error, info};
use ractor::{ActorRef, call_t};
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async, tungstenite::protocol::Message,
};
use tungstenite::client::IntoClientRequest;
use url::Url;


use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::{self, NexusActorMessage, start_nexus},
    portal::PortalActorMessage,
};

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
    let (ws_sender, ws_receiver) = ws_stream.split();

    // map the tungstenite messages to ConduitMessage
    let ws_receiver = ws_receiver.filter_map(|element| {
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
            Err(e) => future::ready(Some(Err(e)))
        }
        
    });

    // map the ConduitMessage to tungstenite messages
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
    let portal_identifier = uri.to_string();
    let portal = call_t!(
        nexus,
        NexusActorMessage::Connected,
        100,
        portal_identifier.clone(),
        ws_sender
    );

    match portal {
        Ok(portal_actor) => {
            info!("Portal actor started for: {}", uri);

            let portal_actor_copy = portal_actor.clone();
            tokio::spawn(async move {
                nexus::receive_loop(ws_receiver, portal_identifier, portal_actor_copy).await
            });

            Ok(portal_actor)
        }
        Err(e) => {
            error!("Error starting portal actor: {}", e);
            Err(e.into())
        }
    }
}
