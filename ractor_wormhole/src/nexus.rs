#[cfg(feature = "async-trait")]
use async_trait::async_trait;
use log::info;
use ractor::{
    Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent,
    concurrency::{Duration, JoinHandle},
};
use std::collections::HashMap;

use crate::{
    conduit::ConduitSink,
    portal::{
        BoxedRematerializer, ConduitID, LocalPortalId, OpaqueActorId, PortalActor, PortalActorArgs,
        PortalActorMessage, PortalConfig,
    },
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
pub struct NexusActorState {
    args: NexusActorArgs,
    portals: HashMap<ActorId, (String, ActorRef<PortalActorMessage>, JoinHandle<()>)>,
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
                info!("New WebSocket connection from: {identifier}");

                // Create a new portal actor
                let (actor_ref, handle) = Actor::spawn_linked(
                    None,
                    PortalActor,
                    PortalActorArgs {
                        identifier: identifier.clone(),
                        sender: ws_stream,
                        local_id: LocalPortalId(rand::random()),
                        config: PortalConfig {
                            default_rpc_port_timeout: Duration::from_secs(120),
                        },
                        parent: myself.clone(),
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

            NexusActorMessage::PublishNamedActor(name, actor_ref, receiver) => {
                let existing = state.named_actors.iter().find(|(s, _)| **s == name);

                if let Some((_, (existing_actor, _))) = existing {
                    if existing_actor.get_id() != actor_ref.get_id() {
                        info!(
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
                    Some(_) => info!("Actor with name {name} already existed and was overwritten"),
                    None => info!("Actor with name {name} published"),
                }
            }
            NexusActorMessage::QueryNamedActor(name, reply) => {
                let result = state.named_actors.get(&name).cloned();

                reply.send(result)?;
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
                    info!("Portal to {addr} terminated: {reason:?}, last_state={last_state:#?}");
                }
            }
            SupervisionEvent::ActorFailed(actor, err) => {
                info!("Actor failed: {:?} - {:?}", actor.get_id(), err);

                if let Some((addr, _, _)) = state.portals.remove(&actor.get_id()) {
                    info!("Portal to {addr} terminated because actor failed: {err:?}");
                }
            }
            _ => (),
        }

        Ok(())
    }
}

// Helper function to create and start the nexus actor
pub async fn start_nexus(
    name: Option<String>,
    on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
) -> Result<ActorRef<NexusActorMessage>, ractor::ActorProcessingErr> {
    let (nexus_ref, _handle) = NexusActor::spawn(
        Some(name.unwrap_or(String::from("nexus"))),
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
