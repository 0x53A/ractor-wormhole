use async_trait::async_trait;
use futures::SinkExt;
use log::{error, info};
use ractor::{
    Actor, ActorCell, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent, actor,
    concurrency::Duration,
};
use std::{collections::HashMap, fmt::Display, pin::Pin};

use crate::{
    conduit::{ConduitMessage, ConduitSink},
    nexus::{NexusActorMessage, RemoteActorId},
    transmaterialization::{
        ContextTransmaterializable, GetRematerializer, TransmaterializationContext,
    },
    util::{ActorRef_Ask, FnActor},
};

use crate::transmaterialization::internal_serializations::SimpleByteTransmaterializable;

// -------------------------------------------------------------------------------------------------------

// note: the introduction is json serialized
/// The **local** portal identifier
#[derive(
    Debug,
    bincode::Encode,
    bincode::Decode,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    Copy,
)]
pub struct LocalPortalId(pub u128);

/// A shared identifier for the Conduit. The id is the same on both portals of the conduit.
/// It is created by both sides generating a random uuid and then xoring both.
#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode, Copy)]
pub struct ConduitID(pub u128);

impl Display for ConduitID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// -------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode)]
pub struct PortalConfig {
    pub default_rpc_port_timeout: Duration,
}

// -------------------------------------------------------------------------------------------------------

/// Hide the internal actor id behind a uuid; only the portal has the mapping between uuid and real actor id
#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode, Copy)]
pub struct OpaqueActorId(pub u128);

impl Display for OpaqueActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// -------------------------------------------------------------------------------------------------------

pub type CrossPortalMessageId = u64;

#[derive(Debug, bincode::Encode, bincode::Decode)]
pub enum ActorRequestError {
    ActorNotFound,
    TransmissionError,
}

impl From<ActorRequestError> for anyhow::Error {
    fn from(val: ActorRequestError) -> Self {
        match val {
            ActorRequestError::ActorNotFound => anyhow::anyhow!("Actor not found"),
            ActorRequestError::TransmissionError => anyhow::anyhow!("Transmission error"),
        }
    }
}

/// these are the raw messages that are actually sent over the wire.
/// Note that these are sent **after** the initial handshake.
/// (the initial handshake is a json serialized `Introduction`)
#[derive(Debug, bincode::Encode, bincode::Decode)]
pub enum CrossPortalMessage {
    RequestActorByName(CrossPortalMessageId, String),
    // RequestActorById(CrossNexusMessageId, OpaqueActorId),
    ResponseActorByName(
        CrossPortalMessageId,
        Result<RemoteActorId, ActorRequestError>,
    ),
    ResponseActorById(
        CrossPortalMessageId,
        Result<RemoteActorId, ActorRequestError>,
    ),

    SendMessage(RemoteActorId, Box<[u8]>),

    ActorExited(RemoteActorId),
}

// Portal
// -------------------------------------------------------------------------------------------------------

pub type NexusResult<T> = Result<T, anyhow::Error>;

/// helper object that wraps an Actor Message Type, rematerializes a message and then forwards it to the actor.
/// That's neccessary because the ActorRef is strongly typed, but we lose the generic type information when storing the actor cell.
#[async_trait]
pub trait MsgRematerializer {
    async fn rematerialize(
        &self,
        actor: ActorCell,
        data: &[u8],
        ctx: TransmaterializationContext,
    ) -> NexusResult<()>;

    fn clone_boxed(&self) -> Box<dyn MsgRematerializer + Send>;
}

pub type BoxedRematerializer = Box<dyn MsgRematerializer + Send>;

impl Clone for BoxedRematerializer {
    fn clone(&self) -> Self {
        self.clone_boxed()
    }
}

type TransmitMessageF = Box<
    (
        dyn FnOnce(
                TransmaterializationContext,
            ) -> Pin<
                Box<
                    (
                        dyn futures::Future<Output = NexusResult<Vec<u8>>>
                            + std::marker::Send
                            + 'static
                    ),
                >,
            > + std::marker::Send
            + 'static
    ),
>;

// Messages for the portal actor
pub enum PortalActorMessage {
    // data received from websocket
    Text(String),
    Binary(Vec<u8>),
    Close,

    ImmaterializeMessage(RemoteActorId, TransmitMessageF),
    TransmitMessage(RemoteActorId, Vec<u8>),

    /// publish a local actor under a known name, making it available to the remote side of the portal.
    /// On the remote side, it can be looked up by name.
    PublishNamedActor(
        String,
        ActorCell,
        BoxedRematerializer,
        Option<RpcReplyPort<Option<RemoteActorId>>>,
    ),

    /// publish a local actor, making it available to the remote side of the portal.
    /// It is published under a random id, which would need to be passed to the remote side through some kind of existing channel.
    /// Note: if you include a RpcReplyPort, you must wait until the handshake is finished and the portals are fully connected.
    ///        if you do NOT include a RpcReplyPort, you can register the actor immediatly.
    PublishActor(
        ActorCell,
        BoxedRematerializer,
        Option<RpcReplyPort<RemoteActorId>>,
    ),

    RegisterProxyForRemoteActor(RemoteActorId, ActorCell),

    /// looks up an actor by name on the **remote** side of the portal. Returns None if no actor was registered under that name.
    QueryNamedRemoteActor(String, RpcReplyPort<NexusResult<RemoteActorId>>),

    /// waits until the portal is fully opened
    WaitForHandshake(RpcReplyPort<()>),

    LocalActorExited(ractor::ActorId),
}

#[cfg(feature = "ractor_cluster")]
impl ractor::Message for PortalActorMessage {}

/// wrap the `ActorRef<PortalActorMessage>` in a more user-friendly interface
#[async_trait]
pub trait Portal {
    async fn instantiate_proxy_for_remote_actor<
        T: ContextTransmaterializable + ractor::Message + Send + Sync + std::fmt::Debug,
    >(
        &self,
        remote_actor_id: RemoteActorId,
    ) -> NexusResult<ActorRef<T>>;

    async fn publish_named_actor<T: ContextTransmaterializable + ractor::Message + Send + Sync>(
        &self,
        name: String,
        actor_ref: ActorRef<T>,
    ) -> NexusResult<()>;

    async fn wait_for_opened(&self, timeout: Duration) -> NexusResult<()>;
}

#[async_trait]
impl Portal for ActorRef<PortalActorMessage> {
    async fn instantiate_proxy_for_remote_actor<
        T: ContextTransmaterializable + ractor::Message + Send + Sync + std::fmt::Debug,
    >(
        &self,
        remote_actor_id: RemoteActorId,
    ) -> NexusResult<ActorRef<T>> {
        let portal_ref = self.clone();

        let (proxy_actor_ref, _handle) =
            FnActor::<T>::start_fn_linked(self.get_cell(), async move |mut ctx| {
                let type_str = std::any::type_name::<T>();

                info!("Proxy actor started {type_str}");

                while let Some(msg) = ctx.rx.recv().await {
                    info!("Proxy actor received msg: {msg:#?} [{type_str}]");
                    let remote_id = remote_actor_id;

                    let f: TransmitMessageF = Box::new(move |ctx| {
                        info!("f inside Proxy Actor was called: {msg:#?} [{type_str}]");
                        // Create a regular closure that returns a boxed and pinned future
                        Box::pin(async move {
                            let bytes: Vec<u8> =
                                crate::transmaterialization::ContextTransmaterializable::immaterialize(
                                    msg, &ctx,
                                )
                                .await?;
                            Ok(bytes)
                        })
                    });

                    if let Err(err) =
                        portal_ref.send_message(PortalActorMessage::ImmaterializeMessage(remote_id, f))
                    {
                        error!("Failed to send message to portal: {err}");
                    }

                    info!(
                        "Proxy actor sent WSPortalMessage::TransmitMessage to portal: {remote_id:#?} [{type_str}]"
                    );
                }
            })
            .await?;

        self.send_message(PortalActorMessage::RegisterProxyForRemoteActor(
            remote_actor_id,
            proxy_actor_ref.get_cell(),
        ))?;
        Ok(proxy_actor_ref)
    }

    async fn publish_named_actor<T: ContextTransmaterializable + ractor::Message + Send + Sync>(
        &self,
        name: String,
        actor_ref: ActorRef<T>,
    ) -> NexusResult<()> {
        let receiver = T::get_rematerializer();

        let response = self.send_message(PortalActorMessage::PublishNamedActor(
            name,
            actor_ref.get_cell(),
            receiver,
            None,
        ))?;

        Ok(response)
    }

    async fn wait_for_opened(&self, timeout: Duration) -> NexusResult<()> {
        let response = self
            .ask(PortalActorMessage::WaitForHandshake, Some(timeout))
            .await?;

        Ok(response)
    }
}

// Portal actor
pub struct PortalActor;

pub enum PortalConduitState {
    Opening {
        self_introduction: Introduction,
    },
    Open {
        // self_introduction: Introduction,
        // remote_introduction: Introduction,
        channel_id: ConduitID,
    },
}

// note: the introduction is json serialized
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Introduction {
    pub channel_id_contribution: uuid::Bytes,
    pub version: String,
    pub info_text: String,
    pub this_side_id: LocalPortalId,
}

pub struct PortalActorState {
    args: PortalActorArgs,
    channel_state: PortalConduitState,
    published_actors: HashMap<OpaqueActorId, (ActorCell, BoxedRematerializer)>,
    proxies_for_remote_actors: HashMap<RemoteActorId, ActorCell>,
    named_actors: HashMap<String, OpaqueActorId>,

    next_request_id: u64,
    open_requests: HashMap<CrossPortalMessageId, RpcReplyPort<NexusResult<RemoteActorId>>>,

    waiting_for_handshake: Vec<RpcReplyPort<()>>,
}

pub struct PortalActorArgs {
    pub identifier: String,
    pub sender: ConduitSink,
    pub local_id: LocalPortalId,
    pub config: PortalConfig,
    pub parent: ActorRef<NexusActorMessage>,
}

fn xor_arrays(a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
    let mut result = [0u8; 16];
    for i in 0..16 {
        result[i] = a[i] ^ b[i];
    }
    result
}

impl PortalActor {
    fn publish_actor(
        &self,
        myself: ActorRef<PortalActorMessage>,
        published_actors: &mut HashMap<OpaqueActorId, (ActorCell, BoxedRematerializer)>,
        actor_cell: ActorCell,
        receiver: BoxedRematerializer,
    ) -> OpaqueActorId {
        let existing = published_actors
            .iter()
            .find(|(_k, (v, _))| v.get_id() == actor_cell.get_id());

        match existing {
            Some((k, _v)) => {
                info!(
                    "Actor with id {} was already published under {}",
                    actor_cell.get_id(),
                    k.clone()
                );
                *k
            }
            None => {
                let new_id = OpaqueActorId(uuid::Uuid::new_v4().to_u128_le());
                info!(
                    "Actor with id {} published as {}",
                    actor_cell.get_id(),
                    new_id.0
                );
                published_actors.insert(new_id, (actor_cell.clone(), receiver));

                ractor::concurrency::spawn(async move {
                    // wait for the actor to exit
                    let _ = actor_cell.wait(None).await;
                    // notify the remote side that the actor has exited
                    let _ = myself
                        .send_message(PortalActorMessage::LocalActorExited(actor_cell.get_id()));
                });

                new_id
            }
        }
    }
}

// Portal actor implementation
#[cfg_attr(feature = "async-trait", async_trait)]
impl Actor for PortalActor {
    type Msg = PortalActorMessage;
    type State = PortalActorState;
    type Arguments = PortalActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        mut args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        const MOTIVATIONAL_MESSAGES: [&str; 5] = [
            "Lookin' good!",
            "Beep Boop",
            "It's a beautiful day",
            "Did you know: Koalas have fingerprints so similar to humans that they've occasionally confused crime scene investigators.",
            "Gentoo penguins propose to their mates with a pebble.",
        ];

        let msg = MOTIVATIONAL_MESSAGES[rand::random_range(0..MOTIVATIONAL_MESSAGES.len())];

        let introduction = Introduction {
            channel_id_contribution: uuid::Uuid::new_v4().to_bytes_le(),
            version: "0.1".to_string(),
            info_text: msg.to_string(),
            this_side_id: args.local_id,
        };
        let text = serde_json::to_string_pretty(&introduction)?;

        args.sender.send(ConduitMessage::Text(text)).await?;
        args.sender.flush().await?;

        Ok(PortalActorState {
            args,
            channel_state: PortalConduitState::Opening {
                self_introduction: introduction,
            },
            published_actors: HashMap::new(),
            proxies_for_remote_actors: HashMap::new(),
            named_actors: HashMap::new(),
            open_requests: HashMap::new(),
            next_request_id: 1,
            waiting_for_handshake: Vec::new(),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            PortalActorMessage::Text(text) => {
                info!(
                    "Received text message from {}: {}",
                    state.args.identifier, text
                );

                match &state.channel_state {
                    PortalConduitState::Opening { self_introduction } => {
                        let remote_introduction: Introduction = serde_json::from_str(&text)?;
                        info!(
                            "Received introduction from {}: {:?}",
                            state.args.identifier, remote_introduction
                        );
                        let channel_id = ConduitID(u128::from_le_bytes(xor_arrays(
                            self_introduction.channel_id_contribution,
                            remote_introduction.channel_id_contribution,
                        )));

                        info!("Handshake complete, channel_id: {channel_id}");

                        state.channel_state = PortalConduitState::Open {
                            // self_introduction: self_introduction.clone(),
                            // remote_introduction,
                            channel_id,
                        };

                        for x in state.waiting_for_handshake.drain(..) {
                            let _ = x.send(());
                        }
                    }
                    PortalConduitState::Open { .. } => {
                        panic!("Received text message after handshake: {text}");
                    }
                }
            }
            PortalActorMessage::Binary(data) => {
                info!(
                    "Received binary message from {}: {} bytes",
                    state.args.identifier,
                    data.len()
                );

                match &state.channel_state {
                    PortalConduitState::Opening { .. } => {
                        panic!(
                            "Received binary message before handshake: {} bytes",
                            data.len()
                        );
                    }
                    PortalConduitState::Open { channel_id, .. } => {
                        let msg = CrossPortalMessage::rematerialize(&data)?;
                        info!("Received message from {}: {:?}", state.args.identifier, msg);

                        match msg {
                            CrossPortalMessage::RequestActorByName(id, name) => {
                                // Look up the named actor in our local registry
                                let opaque_id = state.named_actors.get(&name).cloned();

                                let response = match opaque_id {
                                    Some(opaque_id) => {
                                        // Construct a RemoteActorId for the actor
                                        let remote_id = RemoteActorId {
                                            connection_key: *channel_id,
                                            side: state.args.local_id,
                                            id: opaque_id,
                                        };

                                        Ok(remote_id)
                                    }
                                    None => {
                                        // we didn't find it in the portal registry, but not all is lost, it could be in the nexus registry.
                                        let nexus_actor = state.args.parent.clone();
                                        let actor_published_on_nexus = nexus_actor
                                            .ask(
                                                |rpc| NexusActorMessage::QueryNamedActor(name, rpc),
                                                None,
                                            )
                                            .await?;

                                        if let Some((actor_cell, receiver)) =
                                            actor_published_on_nexus
                                        {
                                            let opaque_actor_id = self.publish_actor(
                                                myself,
                                                &mut state.published_actors,
                                                actor_cell,
                                                receiver,
                                            );

                                            // Construct a RemoteActorId for the actor
                                            let remote_id = RemoteActorId {
                                                connection_key: *channel_id,
                                                side: state.args.local_id,
                                                id: opaque_actor_id,
                                            };

                                            Ok(remote_id)
                                        } else {
                                            Err(ActorRequestError::ActorNotFound)
                                        }
                                    }
                                };

                                // Send response back
                                let response_msg =
                                    CrossPortalMessage::ResponseActorByName(id, response);
                                let data = bincode::encode_to_vec(
                                    response_msg,
                                    bincode::config::standard(),
                                )?;
                                state.args.sender.send(ConduitMessage::Binary(data)).await?;
                                state.args.sender.flush().await?;
                            }

                            // CrossNexusMessage::RequestActorById(id, opaque_id) => {
                            //     // Look up the actor in our registry
                            //     let published_actor = state.published_actors.get(&opaque_id);

                            //     let response = match published_actor {
                            //         Some(actor_id) => {
                            //             // Construct a RemoteActorId for the actor
                            //             let remote_id = RemoteActorId {
                            //                 connection_key: *channel_id,
                            //                 side: state.args.local_id,
                            //                 id: opaque_id,
                            //             };

                            //             Ok(remote_id)
                            //         }
                            //         None => Err(ActorRequestError::ActorNotFound),
                            //     };

                            //     // Send response back
                            //     let response_msg =
                            //         CrossNexusMessage::ResponseActorById(id, response);
                            //     let data = bincode::encode_to_vec(
                            //         response_msg,
                            //         bincode::config::standard(),
                            //     )?;
                            //     state.args.sender.send(RawMessage::Binary(data)).await?;
                            //     state.args.sender.flush().await?;
                            // }
                            CrossPortalMessage::ResponseActorByName(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: NexusResult<RemoteActorId> =
                                        response.map_err(|err| err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {id}");
                                }
                            }

                            CrossPortalMessage::ResponseActorById(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: NexusResult<RemoteActorId> =
                                        response.map_err(|err| err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {id}");
                                }
                            }

                            CrossPortalMessage::SendMessage(target_id, data) => {
                                // Find the local actor from the remote target
                                if let Some((local_actor_cell, receiver)) =
                                    state.published_actors.get(&target_id.id)
                                {
                                    receiver
                                        .rematerialize(
                                            local_actor_cell.clone(),
                                            &data,
                                            TransmaterializationContext {
                                                connection: myself.clone(),
                                                default_rpc_port_timeout: state
                                                    .args
                                                    .config
                                                    .default_rpc_port_timeout,
                                            },
                                        )
                                        .await?;
                                } else {
                                    error!(
                                        "Remote actor ID {} not found in published actors",
                                        target_id.id
                                    );
                                }
                            }

                            CrossPortalMessage::ActorExited(remote_actor_id) => {
                                let Some(actor_cell) =
                                    state.proxies_for_remote_actors.remove(&remote_actor_id)
                                else {
                                    error!(
                                        "Received ActorExited for unknown remote actor: {remote_actor_id:?}"
                                    );
                                    return Ok(());
                                };

                                actor_cell.stop(Some("Proxy for remote actor is being shutdown because the real actor exited".into()));
                            }
                        }
                    }
                }
            }
            PortalActorMessage::Close => {
                info!("Closing portal to {}", state.args.identifier);
                myself.stop(Some("Portal closed".into()));
            }

            PortalActorMessage::WaitForHandshake(reply) => {
                match &state.channel_state {
                    PortalConduitState::Opening { .. } => {
                        state.waiting_for_handshake.push(reply);
                    }
                    PortalConduitState::Open { .. } => {
                        reply.send(())?;
                    }
                }
                return Ok(());
            }

            PortalActorMessage::PublishNamedActor(name, actor_cell, receiver, reply) => {
                if reply.is_some()
                    && !matches!(state.channel_state, PortalConduitState::Open { .. })
                {
                    error!("PublishNamedActor with rpc=Some called before handshake");
                    //return Ok(());
                }

                let opaque_actor_id =
                    self.publish_actor(myself, &mut state.published_actors, actor_cell, receiver);

                // note: this overrides an already published actor of the same name.
                match state.named_actors.insert(name.clone(), opaque_actor_id) {
                    Some(_) => info!("Actor with name {name} already existed and was overwritten"),
                    None => info!("Actor with name {name} published"),
                }

                if let Some(rpc) = reply {
                    if let PortalConduitState::Open { channel_id } = &state.channel_state {
                        let remote_actor_id = RemoteActorId {
                            connection_key: *channel_id,
                            side: state.args.local_id,
                            id: opaque_actor_id,
                        };
                        rpc.send(Some(remote_actor_id))?;
                    } else {
                        rpc.send(None)?;
                    };
                }
            }

            PortalActorMessage::PublishActor(actor_cell, receiver, rpc) => {
                if rpc.is_some() && !matches!(state.channel_state, PortalConduitState::Open { .. })
                {
                    error!("PublishActor with rpc=Some called before handshake");
                    return Ok(());
                }

                let opaque_actor_id =
                    self.publish_actor(myself, &mut state.published_actors, actor_cell, receiver);

                if let Some(rpc) = rpc {
                    let PortalConduitState::Open { channel_id, .. } = &state.channel_state else {
                        error!("PublishActor called before handshake");
                        return Ok(());
                    };

                    let remote_actor_id = RemoteActorId {
                        connection_key: *channel_id,
                        side: state.args.local_id,
                        id: opaque_actor_id,
                    };

                    rpc.send(remote_actor_id)?;
                }
            }

            PortalActorMessage::QueryNamedRemoteActor(name, reply) => {
                let PortalConduitState::Open { .. } = &state.channel_state else {
                    error!("QueryNamedRemoteActor called before handshake");
                    return Ok(());
                };

                let request_id = state.next_request_id;
                state.next_request_id += 1;

                state.open_requests.insert(request_id, reply);

                let request = CrossPortalMessage::RequestActorByName(request_id, name);
                let bytes = request.immaterialize()?;
                state
                    .args
                    .sender
                    .send(ConduitMessage::Binary(bytes))
                    .await?;
                state.args.sender.flush().await?;
            }

            PortalActorMessage::ImmaterializeMessage(target, msg_f) => {
                let PortalConduitState::Open { .. } = &state.channel_state else {
                    error!("TransmitMessage called before handshake");
                    return Ok(());
                };

                // serializing can call into the actor, so we need to release the message pump, otherwise it deadlocks

                // info!(
                //     "Transmitting message to {}, but first serializing ...",
                //     target.id
                // );

                let myself_copy = myself.clone();
                let default_rpc_port_timeout = state.args.config.default_rpc_port_timeout;
                let target_copy = target;
                ractor::concurrency::spawn(async move {
                    let bytes = msg_f(TransmaterializationContext {
                        connection: myself_copy.clone(),
                        default_rpc_port_timeout,
                    })
                    .await
                    .unwrap(); // todo: fix unwrap
                    // info!("Serialized! Now sending ...");

                    let _ = myself_copy
                        .send_message(PortalActorMessage::TransmitMessage(target_copy, bytes));
                });
            }

            PortalActorMessage::TransmitMessage(target, bytes) => {
                let PortalConduitState::Open { .. } = &state.channel_state else {
                    error!("TransmitMessage called before handshake");
                    return Ok(());
                };

                let request = CrossPortalMessage::SendMessage(target, bytes.into_boxed_slice());
                let bytes = request.immaterialize()?;
                state
                    .args
                    .sender
                    .send(ConduitMessage::Binary(bytes))
                    .await?;
                state.args.sender.flush().await?;
            }

            PortalActorMessage::RegisterProxyForRemoteActor(remote_actor_id, actor_cell) => {
                let PortalConduitState::Open { .. } = &state.channel_state else {
                    error!("RegisterProxyForRemoteActor called before handshake");
                    return Ok(());
                };

                state
                    .proxies_for_remote_actors
                    .insert(remote_actor_id, actor_cell);
            }

            PortalActorMessage::LocalActorExited(actor_id) => {
                let PortalConduitState::Open { channel_id, .. } = &state.channel_state else {
                    error!("LocalActorExited called before handshake");
                    return Ok(());
                };

                let Some(entry) = state
                    .published_actors
                    .iter_mut()
                    .find(|(_k, (v, _))| v.get_id() == actor_id)
                else {
                    error!("LocalActorExited called for unknown actor: {actor_id}");
                    return Ok(());
                };

                let msg = CrossPortalMessage::ActorExited(RemoteActorId {
                    connection_key: *channel_id,
                    side: state.args.local_id,
                    id: *entry.0,
                });

                let bytes = msg.immaterialize()?;
                state
                    .args
                    .sender
                    .send(ConduitMessage::Binary(bytes))
                    .await?;
                state.args.sender.flush().await?;
            }
        }
        Ok(())
    }

    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        event: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match event {
            SupervisionEvent::ActorTerminated(actor, last_state, reason) => {
                info!(
                    "Actor {} terminated: {:?}, last_state={:#?}",
                    actor.get_id(),
                    reason,
                    last_state
                );
            }
            SupervisionEvent::ActorFailed(actor, err) => {
                info!("Actor {} failed: {:?}", actor.get_id(), err);
            }
            _ => {}
        }

        Ok(())
    }
}
