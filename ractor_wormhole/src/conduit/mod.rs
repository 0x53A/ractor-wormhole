#[cfg(any(
    feature = "websocket_client",
    feature = "websocket_client_wasm",
    feature = "websocket_server"
))]
pub mod websocket;

#[cfg(target_os = "linux")]
#[cfg(any(feature = "unix_socket_client", feature = "unix_socket_server"))]
pub mod unix_socket;

#[cfg(target_os = "linux")]
#[cfg(feature = "ssh_client")]
pub mod ssh;

use futures::{Sink, Stream, StreamExt};
use ractor::ActorRef;
use std::pin::Pin;

use log::{debug, error};

use crate::{
    nexus::{self, NexusActorMessage},
    portal::PortalActorMessage,
    util::ActorRef_Ask,
};

// -------------------------------------------------------------------------------------------------------

/// The messages that flow through a conduit (transport).
///
/// The protocol has exactly two phases: first each side sends a single [Handshake](ConduitMessage::Handshake),
/// after that all actor communication is [Content](ConduitMessage::Content).
///
/// How these are represented on the wire is up to the transport, e.g. the WebSocket
/// transport maps them to text and binary frames, the byte-stream transports (unix socket, ssh)
/// use a type tag + length prefix.
pub enum ConduitMessage {
    /// the initial handshake message (a json-serialized `Introduction`).
    /// It is sent exactly once, by each side, at the start of the connection.
    Handshake(String),
    /// a content message (bincode-serialized `CrossPortalMessage`), carrying the actual actor communication
    Content(Vec<u8>),
    Close(Option<String>),
}

pub type ConduitError = anyhow::Error;

/// the sink, from the point of view of the Conduit; that is, the 'tx' end of a websocket for example.
/// The conduit writes messages into it.
pub type ConduitSink = Pin<Box<dyn Sink<ConduitMessage, Error = ConduitError> + Send>>;
/// the source, from the point of view of the Conduit; that is, the 'rx' end of a websocket for example.
/// the Conduit (asynchronously) reads messages from it.
pub type ConduitSource = Pin<Box<dyn Stream<Item = Result<ConduitMessage, ConduitError>> + Send>>;

// -------------------------------------------------------------------------------------------------------

pub async fn receive_loop(
    mut receiver: ConduitSource,
    identifier: String,
    actor_ref: ActorRef<PortalActorMessage>,
) {
    // Process incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(msg) => match msg {
                ConduitMessage::Handshake(text) => {
                    if let Err(err) =
                        actor_ref.cast(PortalActorMessage::Handshake(text.to_string()))
                    {
                        error!("Error sending handshake message to actor: {err}");
                        break;
                    }
                }
                ConduitMessage::Content(data) => {
                    if let Err(err) = actor_ref.cast(PortalActorMessage::Content(data.to_vec())) {
                        error!("Error sending content message to actor: {err}");
                        break;
                    }
                }
                ConduitMessage::Close(close_frame) => {
                    debug!("Portal with {identifier} closed because of reason: {close_frame:?}");
                    break;
                }
            },
            Err(e) => {
                error!("Error receiving message from {e}: {identifier}");
                break;
            }
        }
    }

    debug!("Portal with {identifier} closed");
    let _ = actor_ref.cast(PortalActorMessage::Close);
}

pub async fn from_sink_source(
    nexus: ActorRef<nexus::NexusActorMessage>,
    portal_identifier: String,
    sink: ConduitSink,
    source: ConduitSource,
) -> Result<ActorRef<PortalActorMessage>, ConduitError> {
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), sink, rpc),
            None,
        )
        .await;

    match portal {
        Ok(portal_actor) => {
            debug!("Portal actor started for: {portal_identifier}");
            let portal_actor_copy = portal_actor.clone();
            ractor::concurrency::spawn(async move {
                receive_loop(source, portal_identifier, portal_actor_copy).await;
            });
            Ok(portal_actor)
        }
        Err(e) => {
            error!("Error starting portal actor: {e}");
            Err(e)?
        }
    }
}
