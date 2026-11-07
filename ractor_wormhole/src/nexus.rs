use async_trait::async_trait;
use log::{debug, info, warn};
use ractor::{
    Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent,
    concurrency::Duration,
};
use std::collections::HashMap;

use crate::{
    WormholeResult,
    conduit::{CONNECTION_SETUP_TIMEOUT, ConduitSink, MAX_PENDING_CONNECTIONS},
    portal::{
        BoxedRematerializer, ConduitID, LocalPortalId, OpaqueActorId, PortalActor, PortalActorArgs,
        PortalActorMessage, PortalConfig,
    },
    transmaterialization::{ContextTransmaterializable, GetRematerializer},
};

// -------------------------------------------------------------------------------------------------------

// Nexus
// -------------------------------------------------------------------------------------------------------

// Messages for the nexus actor
#[cfg_attr(
    feature = "ractor_cluster",
    derive(ractor_cluster_derive::RactorMessage)
)]
pub enum NexusActorMessage {
    Connected(
        String,
        ConduitSink,
        RpcReplyPort<ActorRef<PortalActorMessage>>,
    ),
    #[doc(hidden)]
    ConnectionReady(
        String,
        Result<StartingPortal, String>,
        RpcReplyPort<ActorRef<PortalActorMessage>>,
    ),
    GetAllPortals(RpcReplyPort<Vec<ActorRef<PortalActorMessage>>>),

    /// publish a local actor under a known name, making it available to the remote side of all portals.
    /// On the remote side, it can be looked up by name.
    PublishNamedActor(String, ActorCell, BoxedRematerializer),

    QueryNamedActor(
        String,
        RpcReplyPort<Option<(ActorCell, BoxedRematerializer)>>,
    ),
}

// Nexus actor state
pub struct NexusActor;

/// Owns an uncommitted portal, including while its completion is in a mailbox.
#[doc(hidden)]
pub struct StartingPortal(Option<ActorRef<PortalActorMessage>>);
impl Drop for StartingPortal {
    fn drop(&mut self) {
        if let Some(portal) = self.0.take() {
            portal.stop(Some("Connection setup abandoned".into()));
        }
    }
}
pub struct NexusActorState {
    args: NexusActorArgs,
    portals: HashMap<ActorId, (String, ActorRef<PortalActorMessage>)>,
    pending_connections: usize,
    named_actors: HashMap<String, (ActorCell, BoxedRematerializer)>,
}

#[cfg_attr(
    feature = "ractor_cluster",
    derive(ractor_cluster_derive::RactorMessage)
)]
pub struct OnActorConnectedMessage {
    pub identifier: String,
    pub actor_ref: ActorRef<PortalActorMessage>,
}

pub struct NexusActorArgs {
    pub on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
}

// Nexus actor implementation
#[cfg_attr(feature = "async-trait", async_trait)]
impl Actor for NexusActor {
    type Msg = NexusActorMessage;
    type State = NexusActorState;
    type Arguments = NexusActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("WebSocket nexus actor started");
        Ok(NexusActorState {
            args,
            portals: HashMap::new(),
            pending_connections: 0,
            named_actors: HashMap::new(),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            NexusActorMessage::Connected(identifier, ws_stream, reply) => {
                if reply.is_closed() || state.pending_connections >= MAX_PENDING_CONNECTIONS {
                    return Ok(());
                }
                state.pending_connections += 1;
                // Portal pre_start writes to the transport. Keep that I/O out of
                // the Nexus mailbox so an unresponsive peer cannot stall others.
                ractor::concurrency::spawn(async move {
                    let parent = myself.clone();
                    let portal_identifier = identifier.clone();
                    let startup = crate::util::with_timeout(CONNECTION_SETUP_TIMEOUT, async move {
                        let (portal, _) = Actor::spawn_linked(
                            None,
                            PortalActor,
                            PortalActorArgs {
                                identifier: portal_identifier,
                                sender: ws_stream,
                                local_id: LocalPortalId(rand::random()),
                                config: PortalConfig {
                                    default_rpc_port_timeout: Duration::from_secs(120),
                                },
                                parent: parent.clone(),
                            },
                            parent.get_cell(),
                        )
                        .await?;
                        Ok::<_, ractor::SpawnErr>(StartingPortal(Some(portal)))
                    });
                    let result = match futures::future::select(
                        Box::pin(startup),
                        Box::pin(myself.wait(None)),
                    )
                    .await
                    {
                        futures::future::Either::Left((result, _)) => result
                            .map_err(|err| err.to_string())
                            .and_then(|result| result.map_err(|err| err.to_string())),
                        futures::future::Either::Right(_) => return,
                    };
                    let _ = myself.send_message(NexusActorMessage::ConnectionReady(
                        identifier, result, reply,
                    ));
                });
            }
            NexusActorMessage::ConnectionReady(identifier, result, reply) => {
                state.pending_connections -= 1;
                let mut started = match result {
                    Ok(started) => started,
                    Err(err) => {
                        warn!("Failed to start portal to {identifier}: {err}");
                        return Ok(());
                    }
                };
                let actor_ref = started.0.as_ref().unwrap().clone();
                if reply.is_closed()
                    || matches!(
                        actor_ref.get_status(),
                        ractor::ActorStatus::Stopped | ractor::ActorStatus::Stopping
                    )
                {
                    return Ok(());
                }
                // A configured callback is required. Reject an already unavailable
                // callback before handing the portal to the transport.
                if state
                    .args
                    .on_client_connected
                    .as_ref()
                    .is_some_and(|callback| {
                        matches!(
                            callback.get_status(),
                            ractor::ActorStatus::Stopped | ractor::ActorStatus::Stopping
                        )
                    })
                {
                    warn!("Connection callback is unavailable");
                    return Ok(());
                }
                if reply.send(actor_ref.clone()).is_err() {
                    return Ok(());
                }
                // Only announce connections whose transport accepted the reply.
                if let Some(callback) = &state.args.on_client_connected {
                    if let Err(err) = callback.send_message(OnActorConnectedMessage {
                        identifier: identifier.clone(),
                        actor_ref: actor_ref.clone(),
                    }) {
                        warn!("Failed to notify connection callback: {err}");
                        return Ok(()); // StartingPortal stops this connection.
                    }
                }
                state
                    .portals
                    .insert(actor_ref.get_id(), (identifier, actor_ref));
                started.0.take();
            }

            NexusActorMessage::GetAllPortals(reply) => {
                let portals: Vec<_> = state.portals.values().map(|v| &v.1).cloned().collect();
                let _ = reply.send(portals);
            }

            NexusActorMessage::PublishNamedActor(name, actor_ref, receiver) => {
                let existing = state.named_actors.iter().find(|(s, _)| **s == name);

                if let Some((_, (existing_actor, _))) = existing {
                    if existing_actor.get_id() != actor_ref.get_id() {
                        warn!(
                            "Actor with id {} already published under name '{}', overwriting with {}''",
                            existing_actor.get_id(),
                            name,
                            actor_ref.get_id()
                        );
                    } else {
                        return Ok(());
                    }
                }

                match state
                    .named_actors
                    .insert(name.clone(), (actor_ref, receiver))
                {
                    Some(_) => warn!("Actor with name {name} already existed and was overwritten"),
                    None => debug!("Actor with name {name} published"),
                }
            }
            NexusActorMessage::QueryNamedActor(name, reply) => {
                let result = state.named_actors.get(&name).cloned();

                let _ = reply.send(result);
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        event: SupervisionEvent,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match &event {
            SupervisionEvent::ActorTerminated(actor, last_state, reason) => {
                if let Some((addr, _)) = state.portals.remove(&actor.get_id()) {
                    debug!("Portal to {addr} terminated: {reason:?}, last_state={last_state:#?}");
                }
            }
            SupervisionEvent::ActorFailed(actor, err) => {
                warn!("Actor failed: {:?} - {:?}", actor.get_id(), err);

                if let Some((addr, _)) = state.portals.remove(&actor.get_id()) {
                    warn!("Portal to {addr} terminated because actor failed: {err:?}");
                }
            }
            _ => (),
        }

        Ok(())
    }
}

/// wrap the `ActorRef<NexusActorMessage>` in a more user-friendly interface
#[async_trait]
pub trait Nexus {
    /// publish a local actor under a known name, making it available to the remote side
    /// of **all** portals of this nexus, current and future.
    /// On the remote side, it can be looked up by name.
    async fn publish_named_actor<T: ContextTransmaterializable + ractor::Message + Send + Sync>(
        &self,
        name: String,
        actor_ref: ActorRef<T>,
    ) -> WormholeResult<()>;
}

#[async_trait]
impl Nexus for ActorRef<NexusActorMessage> {
    async fn publish_named_actor<T: ContextTransmaterializable + ractor::Message + Send + Sync>(
        &self,
        name: String,
        actor_ref: ActorRef<T>,
    ) -> WormholeResult<()> {
        let rematerializer = T::get_rematerializer();

        self.send_message(NexusActorMessage::PublishNamedActor(
            name,
            actor_ref.get_cell(),
            rematerializer,
        ))?;

        Ok(())
    }
}

// Helper function to create and start the nexus actor
pub async fn start_nexus(
    name: Option<String>,
    on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
) -> Result<ActorRef<NexusActorMessage>, ractor::ActorProcessingErr> {
    let (nexus_ref, _handle) = NexusActor::spawn(
        name,
        NexusActor,
        NexusActorArgs {
            on_client_connected,
        },
    )
    .await?;

    Ok(nexus_ref)
}

// ---------------------------------------------------------------------------------

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RemoteActorId {
    /// the portal key uniquely identifies the conduit, it is the same ID on both sides
    pub connection_key: ConduitID,
    /// this id identifies the side, it is different between the two portals of a single conduit
    pub side: LocalPortalId,
    /// the unique id of the actor
    pub id: OpaqueActorId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        conduit::ConduitMessage,
        util::{ActorRef_Ask, FnActor},
    };
    use futures::SinkExt;
    use tokio::sync::oneshot;

    fn gated_sink() -> (ConduitSink, oneshot::Sender<()>, oneshot::Receiver<()>) {
        let (release, gate) = oneshot::channel();
        let (entered, started) = oneshot::channel();
        let sink = futures::sink::unfold(
            Some((entered, gate)),
            |gate, _: ConduitMessage| async move {
                if let Some((entered, gate)) = gate {
                    let _ = entered.send(());
                    let _ = gate.await;
                }
                Ok::<_, anyhow::Error>(None)
            },
        );
        (Box::pin(sink), release, started)
    }

    #[tokio::test]
    async fn pending_startup_does_not_block_nexus_and_cancellation_is_not_announced() {
        let (mut callback, callback_handle) =
            FnActor::<OnActorConnectedMessage>::start().await.unwrap();
        let nexus = start_nexus(None, Some(callback.actor_ref.clone()))
            .await
            .unwrap();
        let (sink, release, started) = gated_sink();
        let request = nexus.ask(
            |reply| NexusActorMessage::Connected("gated".into(), sink, reply),
            None,
        );
        started.await.unwrap();
        let portals = nexus
            .ask(
                NexusActorMessage::GetAllPortals,
                Some(Duration::from_secs(1)),
            )
            .await
            .unwrap();
        assert!(portals.is_empty());
        drop(request);
        release.send(()).unwrap();
        // A marker connection establishes that the Nexus can complete startup
        // while the cancelled connection is discarded.
        let sink = Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        let portal = nexus
            .ask(
                |reply| NexusActorMessage::Connected("marker".into(), sink, reply),
                None,
            )
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(1), callback.rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(event.identifier, "marker");
        portal.stop(None);
        nexus.stop(None);
        nexus.wait(None).await.unwrap();
        callback.actor_ref.stop(None);
        callback_handle.await.unwrap();
        assert!(callback.rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn unavailable_required_callback_rejects_connection_without_stopping_nexus() {
        let (callback, handle) = FnActor::<OnActorConnectedMessage>::start().await.unwrap();
        callback.actor_ref.stop(None);
        handle.await.unwrap();
        let nexus = start_nexus(None, Some(callback.actor_ref)).await.unwrap();
        let sink = Box::pin(futures::sink::drain().sink_map_err(|never| match never {}));
        assert!(
            nexus
                .ask(
                    |reply| NexusActorMessage::Connected("rejected".into(), sink, reply),
                    Some(Duration::from_secs(1))
                )
                .await
                .is_err()
        );
        assert!(
            nexus
                .ask(
                    NexusActorMessage::GetAllPortals,
                    Some(Duration::from_secs(1))
                )
                .await
                .unwrap()
                .is_empty()
        );
        nexus.stop(None);
    }

    #[tokio::test]
    async fn nexus_shutdown_cancels_startup_io() {
        let nexus = start_nexus(None, None).await.unwrap();
        let (sink, mut release, started) = gated_sink();
        let request = nexus.ask(
            |reply| NexusActorMessage::Connected("shutdown".into(), sink, reply),
            None,
        );
        started.await.unwrap();
        nexus.stop(None);
        nexus.wait(None).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), release.closed())
            .await
            .unwrap();
        assert!(request.await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn startup_deadline_releases_pending_request() {
        let nexus = start_nexus(None, None).await.unwrap();
        let (sink, _release, started) = gated_sink();
        let request = nexus.ask(
            |reply| NexusActorMessage::Connected("gated".into(), sink, reply),
            None,
        );
        started.await.unwrap();
        tokio::time::advance(CONNECTION_SETUP_TIMEOUT + Duration::from_secs(1)).await;
        assert!(request.await.is_err());
        assert!(
            nexus
                .ask(NexusActorMessage::GetAllPortals, None)
                .await
                .unwrap()
                .is_empty()
        );
        nexus.stop(None);
    }
}
