use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::{
    Actor, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent, async_trait,
    concurrency::{Duration, JoinHandle},
};
use ractor_cluster_derive::RactorMessage;
use std::collections::HashMap;

use crate::{
    conduit::{ConduitMessage, ConduitSink, ConduitSource},
    portal::{
        ConduitID, LocalPortalId, OpaqueActorId, PortalActor, PortalActorArgs, PortalActorMessage,
        PortalConfig,
    },
};

// -------------------------------------------------------------------------------------------------------

// Nexus
// -------------------------------------------------------------------------------------------------------

// Messages for the nexus actor
#[derive(RactorMessage)]
pub enum NexusActorMessage {
    Connected(
        String,
        ConduitSink,
        RpcReplyPort<ActorRef<PortalActorMessage>>,
    ),
    GetAllPortals(RpcReplyPort<Vec<ActorRef<PortalActorMessage>>>),
}

// Nexus actor state
pub struct NexusActor;
pub struct NexusActorState {
    args: NexusActorArgs,
    portals: HashMap<ActorId, (String, ActorRef<PortalActorMessage>, JoinHandle<()>)>,
}

#[derive(RactorMessage)]
pub struct OnActorConnectedMessage {
    pub identifier: String,
    pub actor_ref: ActorRef<PortalActorMessage>,
}

pub struct NexusActorArgs {
    pub on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
}

// Nexus actor implementation
#[async_trait]
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
                info!("New WebSocket connection from: {}", identifier);

                // Create a new portal actor
                let (actor_ref, handle) = PortalActor::spawn_linked(
                    None,
                    PortalActor,
                    PortalActorArgs {
                        identifier: identifier.clone(),
                        sender: ws_stream,
                        local_id: LocalPortalId(rand::random()),
                        config: PortalConfig {
                            default_rpc_port_timeout: Duration::from_secs(120),
                        },
                    },
                    myself.get_cell(),
                )
                .await?;

                // Store the new portal
                state.portals.insert(
                    actor_ref.get_id(),
                    (identifier.clone(), actor_ref.clone(), handle),
                );

                if let Some(callback) = &state.args.on_client_connected {
                    callback.send_message(OnActorConnectedMessage {
                        identifier,
                        actor_ref: actor_ref.clone(),
                    })?;
                }

                // Reply with the portal actor reference
                reply.send(actor_ref)?;
            }

            NexusActorMessage::GetAllPortals(reply) => {
                let portals: Vec<_> = state.portals.values().map(|v| &v.1).cloned().collect();
                reply.send(portals)?;
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
                if let Some((addr, _, _)) = state.portals.remove(&actor.get_id()) {
                    info!(
                        "Portal to {} terminated: {:?}, last_state={:#?}",
                        addr, reason, last_state
                    );
                }
            }
            SupervisionEvent::ActorFailed(actor, err) => {
                info!("Actor failed: {:?} - {:?}", actor.get_id(), err);

                if let Some((addr, _, _)) = state.portals.remove(&actor.get_id()) {
                    info!(
                        "Portal to {} terminated because actor failed: {:?}",
                        addr, err
                    );
                }
            }
            _ => (),
        }

        Ok(())
    }
}

// Helper function to create and start the nexus actor
pub async fn start_nexus(
    on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
) -> Result<ActorRef<NexusActorMessage>, ractor::ActorProcessingErr> {

    let (nexus_ref, _handle) = NexusActor::spawn(
        Some(String::from("nexus")),
        NexusActor,
        NexusActorArgs {
            on_client_connected,
        },
    )
    .await?;

    Ok(nexus_ref)
}

pub async fn receive_loop(
    mut ws_receiver: ConduitSource,
    identifier: String,
    actor_ref: ActorRef<PortalActorMessage>,
) {
    // Process incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(msg) => match msg {
                ConduitMessage::Text(text) => {
                    if let Err(err) = actor_ref.cast(PortalActorMessage::Text(text.to_string())) {
                        error!("Error sending text message to actor: {}", err);
                        break;
                    }
                }
                ConduitMessage::Binary(data) => {
                    if let Err(err) = actor_ref.cast(PortalActorMessage::Binary(data.to_vec())) {
                        error!("Error sending binary message to actor: {}", err);
                        break;
                    }
                }
                ConduitMessage::Close(close_frame) => {
                    info!(
                        "Portal with {} closed because of reason: {:?}",
                        identifier, close_frame
                    );
                    break;
                }
                _ => {}
            },
            Err(e) => {
                error!("Error receiving message from {}: {}", e, identifier);
                break;
            }
        }
    }

    info!("Portal with {} closed", identifier);
    let _ = actor_ref.cast(PortalActorMessage::Close);
}

// ---------------------------------------------------------------------------------

// pub struct ProxyActor<T: Send + Sync + ractor::Message + 'static> {
//     _a: PhantomData<T>,
// }
// impl<T: Send + Sync + ractor::Message + 'static> Default for ProxyActor<T> {
//     fn default() -> Self {
//         Self::new()
//     }
// }

// impl<T: Send + Sync + ractor::Message + 'static> ProxyActor<T> {
//     pub fn new() -> Self {
//         ProxyActor { _a: PhantomData }
//     }
// }

#[derive(bincode::Encode, bincode::Decode, Debug, Clone, Copy)]
pub struct RemoteActorId {
    /// the portal key uniquely identifies the conduit, it is the same ID on both sides
    pub connection_key: ConduitID,
    /// this id identifies the side, it is different between the two portals of a single conduit
    pub side: LocalPortalId,
    /// the unique id of the actor
    pub id: OpaqueActorId,
}

// pub struct ProxyActorState {
//     pub args: ProxyActorArgs,
// }
// pub struct ProxyActorArgs {
//     remote_actor_id: ActorId,
// }

// #[async_trait]
// impl<T: Send + Sync + ractor::Message + 'static> Actor for ProxyActor<T> {
//     type Msg = T;
//     type State = ProxyActorState;
//     type Arguments = ProxyActorArgs;

//     async fn pre_start(
//         &self,
//         _myself: ActorRef<Self::Msg>,
//         args: Self::Arguments,
//     ) -> Result<Self::State, ActorProcessingErr> {
//         Ok(ProxyActorState { args })
//     }

//     async fn handle(
//         &self,
//         _myself: ActorRef<Self::Msg>,
//         message: Self::Msg,
//         _state: &mut Self::State,
//     ) -> Result<(), ActorProcessingErr> {
//         todo!();

//         Ok(())
//     }
// }
