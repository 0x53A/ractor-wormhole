use futures::{Sink, SinkExt, Stream, StreamExt};
use log::{error, info};
use ractor::{
    Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent,
    async_trait,
    concurrency::{Duration, JoinHandle},
};
use ractor_cluster_derive::RactorMessage;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, pin::Pin};

use crate::{
    serialization::{ContextTransmaterializable, GetReceiver, TransmaterializationContext},
    util::{ActorRef_Ask, FnActor},
};

use crate::serialization::internal_serializations::SimpleByteTransmaterializable;

// -------------------------------------------------------------------------------------------------------

pub enum RawMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<String>),
    Other,
}

pub type RawError = anyhow::Error;

pub type WebSocketSink = Pin<Box<dyn Sink<RawMessage, Error = RawError> + Send>>;
pub type WebSocketSource = Pin<Box<dyn Stream<Item = Result<RawMessage, RawError>> + Send>>;

// -------------------------------------------------------------------------------------------------------

/// The **local** portal identifier
#[derive(
    RactorMessage,
    Debug,
    bincode::Encode,
    bincode::Decode,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    Copy,
)]
pub struct LocalPortalId(pub u128);

/// A shared identifier for the portal. The id is the same on both sides of the portal.
/// It is created by both sides generating a random uuid and then xoring both.
#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode, Copy)]
pub struct PortalKey(pub u128);

impl Display for PortalKey {
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

pub type CrossNexusMessageId = u64;

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
pub enum CrossNexusMessage {
    RequestActorByName(CrossNexusMessageId, String),
    // RequestActorById(CrossNexusMessageId, OpaqueActorId),
    ResponseActorByName(
        CrossNexusMessageId,
        Result<RemoteActorId, ActorRequestError>,
    ),
    ResponseActorById(
        CrossNexusMessageId,
        Result<RemoteActorId, ActorRequestError>,
    ),

    SendMessage(RemoteActorId, Box<[u8]>),
}

// Portal
// -------------------------------------------------------------------------------------------------------

pub type NexusResult<T> = Result<T, anyhow::Error>;

#[async_trait]
pub trait MsgReceiver {
    async fn receive(
        &self,
        actor: ActorCell,
        data: &[u8],
        ctx: TransmaterializationContext,
    ) -> NexusResult<()>;
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
pub enum WSPortalMessage {
    // data received from websocket
    Text(String),
    Binary(Vec<u8>),
    Close,

    SerializeMessage(RemoteActorId, TransmitMessageF),
    TransmitMessage(RemoteActorId, Vec<u8>),

    /// publish a local actor under a known name, making it available to the remote side of the portal.
    /// On the remote side, it can be looked up by name.
    PublishNamedActor(
        String,
        ActorCell,
        Box<dyn MsgReceiver + Send>,
        Option<RpcReplyPort<RemoteActorId>>,
    ),

    /// publish a local actor, making it available to the remote side of the portal.
    /// It is published under a random id, which would need to be passed to the remote side through some kind of existing channel.
    PublishActor(
        ActorCell,
        Box<dyn MsgReceiver + Send>,
        RpcReplyPort<RemoteActorId>,
    ),

    /// looks up an actor by name on the **remote** side of the portal. Returns None if no actor was registered under that name.
    QueryNamedRemoteActor(String, RpcReplyPort<NexusResult<RemoteActorId>>),
}

impl ractor::Message for WSPortalMessage {}

#[async_trait]
pub trait UserFriendlyPortal {
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
    ) -> NexusResult<RemoteActorId>;
}

#[async_trait]
impl UserFriendlyPortal for ActorRef<WSPortalMessage> {
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

                info!("Proxy actor started {}", type_str);

                while let Some(msg) = ctx.rx.recv().await {
                    info!("Proxy actor received msg: {:#?} [{}]", msg, type_str);
                    let remote_id = remote_actor_id;

                    let f: TransmitMessageF = Box::new(move |ctx| {
                        info!("f inside Proxy Actor was called: {:#?} [{}]", msg, type_str);
                        // Create a regular closure that returns a boxed and pinned future
                        Box::pin(async move {
                            let bytes: Vec<u8> =
                                crate::serialization::ContextTransmaterializable::immaterialize(
                                    msg, &ctx,
                                )
                                .await?;
                            Ok(bytes)
                        })
                    });

                    if let Err(err) =
                        portal_ref.send_message(WSPortalMessage::SerializeMessage(remote_id, f))
                    {
                        error!("Failed to send message to portal: {}", err);
                    }

                    info!(
                        "Proxy actor sent WSPortalMessage::TransmitMessage to portal: {:#?} [{}]",
                        remote_id, type_str
                    );
                }
            })
            .await?;

        Ok(proxy_actor_ref)
    }

    async fn publish_named_actor<T: ContextTransmaterializable + ractor::Message + Send + Sync>(
        &self,
        name: String,
        actor_ref: ActorRef<T>,
    ) -> NexusResult<RemoteActorId> {
        let receiver = actor_ref.get_receiver();

        let response = self
            .ask(
                |rpc| {
                    WSPortalMessage::PublishNamedActor(
                        name,
                        actor_ref.get_cell(),
                        receiver,
                        Some(rpc),
                    )
                },
                None,
            )
            .await?;

        Ok(response)
    }
}

// Portal actor
struct WSPortal;

enum ChannelState {
    Opening {
        self_introduction: Introduction,
    },
    Open {
        // self_introduction: Introduction,
        // remote_introduction: Introduction,
        channel_id: PortalKey,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Introduction {
    pub channel_id_contribution: uuid::Bytes,
    pub version: String,
    pub info_text: String,
    pub this_side_id: LocalPortalId,
}

pub struct WSPortalState {
    args: WSPortalArgs,
    channel_state: ChannelState,
    published_actors: HashMap<OpaqueActorId, (ActorCell, Box<dyn MsgReceiver + Send>)>,
    named_actors: HashMap<String, OpaqueActorId>,

    next_request_id: u64,
    open_requests: HashMap<CrossNexusMessageId, RpcReplyPort<NexusResult<RemoteActorId>>>,
}

pub struct WSPortalArgs {
    identifier: String,
    sender: WebSocketSink,
    local_id: LocalPortalId,
    config: PortalConfig,
}

fn xor_arrays(a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
    let mut result = [0u8; 16];
    for i in 0..16 {
        result[i] = a[i] ^ b[i];
    }
    result
}

// Portal actor implementation
#[async_trait]
impl Actor for WSPortal {
    type Msg = WSPortalMessage;
    type State = WSPortalState;
    type Arguments = WSPortalArgs;

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

        args.sender.send(RawMessage::Text(text)).await?;
        args.sender.flush().await?;

        Ok(WSPortalState {
            args,
            channel_state: ChannelState::Opening {
                self_introduction: introduction,
            },
            published_actors: HashMap::new(),
            named_actors: HashMap::new(),
            open_requests: HashMap::new(),
            next_request_id: 1,
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            WSPortalMessage::Text(text) => {
                info!(
                    "Received text message from {}: {}",
                    state.args.identifier, text
                );

                match &state.channel_state {
                    ChannelState::Opening { self_introduction } => {
                        let remote_introduction: Introduction = serde_json::from_str(&text)?;
                        info!(
                            "Received introduction from {}: {:?}",
                            state.args.identifier, remote_introduction
                        );
                        let channel_id = PortalKey(u128::from_le_bytes(xor_arrays(
                            self_introduction.channel_id_contribution,
                            remote_introduction.channel_id_contribution,
                        )));

                        state.channel_state = ChannelState::Open {
                            // self_introduction: self_introduction.clone(),
                            // remote_introduction,
                            channel_id,
                        };
                    }
                    ChannelState::Open { .. } => {
                        panic!("Received text message after handshake: {}", text);
                    }
                }
            }
            WSPortalMessage::Binary(data) => {
                info!(
                    "Received binary message from {}: {} bytes",
                    state.args.identifier,
                    data.len()
                );

                match &state.channel_state {
                    ChannelState::Opening { .. } => {
                        panic!(
                            "Received binary message before handshake: {} bytes",
                            data.len()
                        );
                    }
                    ChannelState::Open { channel_id, .. } => {
                        let msg = CrossNexusMessage::rematerialize(&data)?;
                        info!("Received message from {}: {:?}", state.args.identifier, msg);

                        match msg {
                            CrossNexusMessage::RequestActorByName(id, name) => {
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
                                    None => Err(ActorRequestError::ActorNotFound),
                                };

                                // Send response back
                                let response_msg =
                                    CrossNexusMessage::ResponseActorByName(id, response);
                                let data = bincode::encode_to_vec(
                                    response_msg,
                                    bincode::config::standard(),
                                )?;
                                state.args.sender.send(RawMessage::Binary(data)).await?;
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
                            CrossNexusMessage::ResponseActorByName(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: NexusResult<RemoteActorId> =
                                        response.map_err(|err| err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {}", id);
                                }
                            }

                            CrossNexusMessage::ResponseActorById(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: NexusResult<RemoteActorId> =
                                        response.map_err(|err| err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {}", id);
                                }
                            }

                            CrossNexusMessage::SendMessage(target_id, data) => {
                                // Find the local actor from the remote target
                                if let Some((local_actor_cell, receiver)) =
                                    state.published_actors.get(&target_id.id)
                                {
                                    receiver
                                        .receive(
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
                        }
                    }
                }
            }
            WSPortalMessage::Close => {
                info!("Closing portal to {}", state.args.identifier);
                myself.stop(Some("Portal closed".into()));
            }

            WSPortalMessage::PublishNamedActor(name, actor_cell, receiver, reply) => {
                let ChannelState::Open { channel_id, .. } = &state.channel_state else {
                    error!("PublishNamedActor called before handshake");
                    return Ok(());
                };

                let existing = state
                    .published_actors
                    .iter()
                    .find(|(_k, (v, _))| v.get_id() == actor_cell.get_id());

                let opaque_actor_id = match existing {
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
                        state
                            .published_actors
                            .insert(new_id, (actor_cell, receiver));
                        new_id
                    }
                };

                // note: this overrides an already published actor of the same name.
                match state.named_actors.insert(name.clone(), opaque_actor_id) {
                    Some(_) => info!(
                        "Actor with name {} already existed and was overwritten",
                        name
                    ),
                    None => info!("Actor with name {} published", name),
                }

                let remote_actor_id = RemoteActorId {
                    connection_key: *channel_id,
                    side: state.args.local_id,
                    id: opaque_actor_id,
                };

                if let Some(rpc) = reply {
                    rpc.send(remote_actor_id)?;
                }
            }

            WSPortalMessage::PublishActor(actor_cell, receiver, rpc) => {
                let ChannelState::Open { channel_id, .. } = &state.channel_state else {
                    error!("PublishActor called before handshake");
                    return Ok(());
                };

                let existing = state
                    .published_actors
                    .iter()
                    .find(|(_k, (v, _))| v.get_id() == actor_cell.get_id());

                let opaque_actor_id = match existing {
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
                        state
                            .published_actors
                            .insert(new_id, (actor_cell, receiver));
                        new_id
                    }
                };

                let remote_actor_id = RemoteActorId {
                    connection_key: *channel_id,
                    side: state.args.local_id,
                    id: opaque_actor_id,
                };

                rpc.send(remote_actor_id)?;
            }

            WSPortalMessage::QueryNamedRemoteActor(name, reply) => {
                let ChannelState::Open { .. } = &state.channel_state else {
                    error!("QueryNamedRemoteActor called before handshake");
                    return Ok(());
                };

                let request_id = state.next_request_id;
                state.next_request_id += 1;

                state.open_requests.insert(request_id, reply);

                let request = CrossNexusMessage::RequestActorByName(request_id, name);
                let bytes = request.immaterialize()?;
                state.args.sender.send(RawMessage::Binary(bytes)).await?;
                state.args.sender.flush().await?;
            }

            WSPortalMessage::SerializeMessage(target, msg_f) => {
                let ChannelState::Open { .. } = &state.channel_state else {
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
                tokio::spawn(async move {
                    let bytes = msg_f(TransmaterializationContext {
                        connection: myself_copy.clone(),
                        default_rpc_port_timeout,
                    })
                    .await
                    .unwrap(); // todo: fix unwrap
                    // info!("Serialized! Now sending ...");

                    let _ = myself_copy
                        .send_message(WSPortalMessage::TransmitMessage(target_copy, bytes));
                });
            }

            WSPortalMessage::TransmitMessage(target, bytes) => {
                let ChannelState::Open { .. } = &state.channel_state else {
                    error!("TransmitMessage called before handshake");
                    return Ok(());
                };

                let request = CrossNexusMessage::SendMessage(target, bytes.into_boxed_slice());
                let bytes = request.immaterialize()?;
                state.args.sender.send(RawMessage::Binary(bytes)).await?;
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

// Nexus
// -------------------------------------------------------------------------------------------------------

// Messages for the nexus actor
#[derive(RactorMessage)]
pub enum WSNexusMessage {
    Connected(
        String,
        WebSocketSink,
        RpcReplyPort<ActorRef<WSPortalMessage>>,
    ),
    GetAllPortals(RpcReplyPort<Vec<ActorRef<WSPortalMessage>>>),
}

// Nexus actor state
pub struct WSNexus;
pub struct WSNexusState {
    args: WSNexusArgs,
    portals: HashMap<ActorId, (String, ActorRef<WSPortalMessage>, JoinHandle<()>)>,
}

#[derive(RactorMessage)]
pub struct OnActorConnectedMessage {
    pub identifier: String,
    pub actor_ref: ActorRef<WSPortalMessage>,
}

pub struct WSNexusArgs {
    pub on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
}

// Nexus actor implementation
#[async_trait]
impl Actor for WSNexus {
    type Msg = WSNexusMessage;
    type State = WSNexusState;
    type Arguments = WSNexusArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("WebSocket nexus actor started");
        Ok(WSNexusState {
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
            WSNexusMessage::Connected(identifier, ws_stream, reply) => {
                info!("New WebSocket connection from: {}", identifier);

                // Create a new portal actor
                let (actor_ref, handle) = ractor::Actor::spawn_linked(
                    None,
                    WSPortal,
                    WSPortalArgs {
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

            WSNexusMessage::GetAllPortals(reply) => {
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
) -> Result<ActorRef<WSNexusMessage>, ractor::ActorProcessingErr> {
    let (nexus_ref, handle) = ractor::Actor::spawn(
        Some(String::from("nexus")),
        WSNexus,
        WSNexusArgs {
            on_client_connected,
        },
    )
    .await?;

    // question: do I need to detach?
    let _ = handle;

    Ok(nexus_ref)
}

pub async fn receive_loop(
    mut ws_receiver: WebSocketSource,
    identifier: String,
    actor_ref: ActorRef<WSPortalMessage>,
) {
    // Process incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(msg) => match msg {
                RawMessage::Text(text) => {
                    if let Err(err) = actor_ref.cast(WSPortalMessage::Text(text.to_string())) {
                        error!("Error sending text message to actor: {}", err);
                        break;
                    }
                }
                RawMessage::Binary(data) => {
                    if let Err(err) = actor_ref.cast(WSPortalMessage::Binary(data.to_vec())) {
                        error!("Error sending binary message to actor: {}", err);
                        break;
                    }
                }
                RawMessage::Close(close_frame) => {
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
    let _ = actor_ref.cast(WSPortalMessage::Close);
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
    pub connection_key: PortalKey,
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
