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

#[cfg(feature = "ractor_cluster")]
impl ractor::Message for ConduitMessage {}

/// Deadline for each connection setup stage; established connections are unaffected.
pub const CONNECTION_SETUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
/// Maximum simultaneous startup operations per listener or Nexus.
pub const MAX_PENDING_CONNECTIONS: usize = 64;

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
    loop {
        let message =
            match futures::future::select(Box::pin(actor_ref.wait(None)), receiver.next()).await {
                futures::future::Either::Left(_) => break,
                futures::future::Either::Right((message, _)) => message,
            };
        let Some(msg) = message else { break };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::ActorRef_Ask;

    struct NotifyDrop(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for NotifyDrop {
        fn drop(&mut self) {
            if let Some(tx) = self.0.take() {
                let _ = tx.send(());
            }
        }
    }

    #[tokio::test]
    async fn stopping_portal_drops_idle_source() {
        let nexus = crate::nexus::start_nexus(None, None).await.unwrap();
        use futures::SinkExt;
        let sink = Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let portal = nexus
            .ask(
                |reply| NexusActorMessage::Connected("idle".into(), sink, reply),
                None,
            )
            .await
            .unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let source = futures::stream::unfold(NotifyDrop(Some(tx)), |state| async move {
            futures::future::pending::<()>().await;
            Some((Ok(ConduitMessage::Close(None)), state))
        });
        let receive = tokio::spawn(receive_loop(
            Box::pin(source),
            "idle".into(),
            portal.clone(),
        ));
        portal.stop(None);
        portal.wait(None).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), rx)
            .await
            .unwrap()
            .unwrap();
        receive.await.unwrap();
        nexus.stop(None);
    }

    /// Regression: ractor < 0.16.4 kept only a single "already stopped" permit,
    /// so the second `wait()` on a stopped actor never returned.
    #[tokio::test]
    async fn repeated_wait_on_stopped_actor_returns() {
        let (ctx, handle) = crate::util::FnActor::<u32>::start().await.unwrap();
        ctx.actor_ref.stop(None);
        handle.await.unwrap();
        for _ in 0..3 {
            tokio::time::timeout(std::time::Duration::from_secs(1), ctx.actor_ref.wait(None))
                .await
                .expect("wait() on a stopped actor must return")
                .unwrap();
        }
    }

    /// Regression: a receive loop that starts after its portal stopped must not
    /// consume the stop notification needed by later `wait()` callers.
    #[tokio::test]
    async fn receive_loop_on_stopped_portal_does_not_block_later_waiters() {
        let nexus = crate::nexus::start_nexus(None, None).await.unwrap();
        use futures::SinkExt;
        let sink = Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let portal = nexus
            .ask(
                |reply| NexusActorMessage::Connected("stopped".into(), sink, reply),
                None,
            )
            .await
            .unwrap();
        portal
            .stop_and_wait(None, Some(std::time::Duration::from_secs(1)))
            .await
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            receive_loop(
                Box::pin(futures::stream::pending()),
                "stopped".into(),
                portal.clone(),
            ),
        )
        .await
        .expect("receive loop must exit for a stopped portal");
        tokio::time::timeout(std::time::Duration::from_secs(1), portal.wait(None))
            .await
            .expect("later wait() must return")
            .unwrap();
        nexus.stop(None);
    }
}
