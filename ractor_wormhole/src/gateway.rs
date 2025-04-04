use bincode::de;
use futures::{future::BoxFuture, Sink, SinkExt, Stream, StreamExt};
use log::{error, info};
use ractor::{
    async_trait, concurrency::{Duration, JoinHandle}, Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent
};
use ractor_cluster_derive::RactorMessage;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, marker::PhantomData, net::SocketAddr, pin::Pin};

use crate::{serialization::{ActorSerializationContext, ContextSerializable}, util::FnActor};

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

/// The **local** connection identifier
#[derive(RactorMessage, Debug, bincode::Encode, bincode::Decode, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalConnectionId(pub u128);

/// A shared identifier for the connection. The id is the same on both sides of the connection.
/// It is created by both sides generating a random uuid and then xoring both.
#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode)]
pub struct ConnectionKey(pub u128);

impl Display for ConnectionKey {
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


/// Hide the internal actor id behind a uuid; only the connection has the mapping between uuid and real actor id
#[derive(Clone, Debug, PartialEq, Eq, Hash, bincode::Encode, bincode::Decode)]
pub struct OpaqueActorId(pub u128);

impl Display for OpaqueActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// -------------------------------------------------------------------------------------------------------

pub type CrossGatewayMessageId = u64;

#[derive(Debug, bincode::Encode, bincode::Decode)]
pub enum ActorRequestError {
    ActorNotFound,
    TransmissionError
}

impl Into<anyhow::Error> for ActorRequestError {
    fn into(self) -> anyhow::Error {
        match self {
            ActorRequestError::ActorNotFound => anyhow::anyhow!("Actor not found"),
            ActorRequestError::TransmissionError => anyhow::anyhow!("Transmission error"),
        }
    }
}

/// these are the raw messages that are actually sent over the wire.
/// Note that these are sent **after** the initial handshake.
/// (the initial handshake is a json serialized `Introduction`)
#[derive(Debug, bincode::Encode, bincode::Decode)]
pub enum CrossGatewayMessage {
    RequestActorByName(CrossGatewayMessageId, String),
    RequestActorById(CrossGatewayMessageId, OpaqueActorId),

    ResponseActorByName(
        CrossGatewayMessageId,
        Result<RemoteActorId, ActorRequestError>,
    ),
    ResponseActorById(CrossGatewayMessageId, Result<RemoteActorId, ActorRequestError>),

    SendMessage(RemoteActorId, Box<[u8]>),
}

// Connection
// -------------------------------------------------------------------------------------------------------

pub type GatewayResult<T> = Result<T, anyhow::Error>;

#[async_trait]
pub trait MsgReceiver {
    async fn receive(&self, actor: ActorCell, data: &[u8], ctx: ActorSerializationContext) -> GatewayResult<()>;
}

type TransmitMessageF = Box<(dyn FnOnce(ActorSerializationContext) -> Pin<Box<(dyn futures::Future<Output = GatewayResult<Vec<u8>>> + std::marker::Send + 'static)>> + std::marker::Send + 'static)>;

// Messages for the connection actor
pub enum WSConnectionMessage {
    // data received from websocket
    Text(String),
    Binary(Vec<u8>),
    Close,

    TransmitMessage(RemoteActorId, TransmitMessageF),

    /// publish a local actor under a known name, making it available to the remote side of the connection.
    /// On the remote side, it can be looked up by name.
    PublishNamedActor(String, ActorCell, Box<dyn MsgReceiver + Send>, Option<RpcReplyPort<RemoteActorId>>),

    /// publish a local actor, making it available to the remote side of the connection.
    /// It is published under a random id, which would need to be passed to the remote side through some kind of existing channel.
    PublishActor(ActorCell, Box<dyn MsgReceiver + Send>, RpcReplyPort<RemoteActorId>),

    /// looks up an actor by name on the **remote** side of the connection. Returns None if no actor was registered under that name.
    QueryNamedRemoteActor(String, RpcReplyPort<GatewayResult<RemoteActorId>>),
}

impl ractor::Message for WSConnectionMessage { }


pub trait UserFriendlyConnection {
    async fn instantiate_proxy_for_remote_actor<T: ContextSerializable + ractor::Message + Send + Sync>(&self, remote_actor_id: RemoteActorId) -> GatewayResult<ActorRef<T>>;
}

impl UserFriendlyConnection for ActorRef<WSConnectionMessage> {
    async fn instantiate_proxy_for_remote_actor<T: ContextSerializable + ractor::Message + Send + Sync>(&self, remote_actor_id: RemoteActorId) -> GatewayResult<ActorRef<T>> {
        let connection_ref = self.clone();
        
        let (proxy_actor_ref, _handle) =
            FnActor::<T>::start_fn_linked(self.get_cell(), async move |mut ctx| {
                while let Some(msg) = ctx.rx.recv().await {
                    let msg_clone = msg;
                    let remote_id = remote_actor_id.clone();
                    
                    let f: TransmitMessageF = Box::new(move |ctx| {
                        // Create a regular closure that returns a boxed and pinned future
                        Box::pin(async move {
                            let bytes: Vec<u8> = crate::serialization::ContextSerializable::serialize(msg_clone, &ctx).await?;
                            Ok(bytes)
                        })
                    });
                    
                    let _ = connection_ref.send_message(WSConnectionMessage::TransmitMessage(remote_id, f));
                }
            }).await?;

        Ok(proxy_actor_ref)
    }
}

// Connection actor state
struct WSConnection;

enum ChannelState {
    Opening {
        self_introduction: Introduction,
    },
    Open {
        self_introduction: Introduction,
        remote_introduction: Introduction,
        channel_id: ConnectionKey,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Introduction {
    pub channel_id_contribution: uuid::Bytes,
    pub version: String,
    pub info_text: String,
    pub this_side_id: LocalConnectionId
}

pub struct WSConnectionState {
    args: WSConnectionArgs,
    channel_state: ChannelState,
    published_actors: HashMap<OpaqueActorId, (ActorCell, Box<dyn MsgReceiver + Send>)>,
    named_actors: HashMap<String, OpaqueActorId>,

    next_request_id: u64,
    open_requests: HashMap<CrossGatewayMessageId, RpcReplyPort<GatewayResult<RemoteActorId>>>,
}

pub struct WSConnectionArgs {
    addr: SocketAddr,
    sender: WebSocketSink,
    local_id: LocalConnectionId,
    config: PortalConfig
}

fn xor_arrays(a: [u8; 16], b: [u8; 16]) -> [u8; 16] {
    let mut result = [0u8; 16];
    for i in 0..16 {
        result[i] = a[i] ^ b[i];
    }
    result
}

// Connection actor implementation
#[async_trait]
impl Actor for WSConnection {
    type Msg = WSConnectionMessage;
    type State = WSConnectionState;
    type Arguments = WSConnectionArgs;

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
            this_side_id: args.local_id.clone(),
        };
        let text = serde_json::to_string_pretty(&introduction)?;

        args.sender.send(RawMessage::Text(text)).await?;
        args.sender.flush().await?;

        Ok(WSConnectionState {
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
            WSConnectionMessage::Text(text) => {
                info!("Received text message from {}: {}", state.args.addr, text);

                match &state.channel_state {
                    ChannelState::Opening { self_introduction } => {
                        let remote_introduction: Introduction = serde_json::from_str(&text)?;
                        info!(
                            "Received introduction from {}: {:?}",
                            state.args.addr, remote_introduction
                        );
                        let channel_id = ConnectionKey(u128::from_le_bytes(xor_arrays(
                            self_introduction.channel_id_contribution,
                            remote_introduction.channel_id_contribution,
                        )));

                        state.channel_state = ChannelState::Open {
                            self_introduction: self_introduction.clone(),
                            remote_introduction,
                            channel_id: channel_id.clone(),
                        };
                    }
                    ChannelState::Open { .. } => {
                        panic!("Received text message after handshake: {}", text);
                    }
                }
            }
            WSConnectionMessage::Binary(data) => {
                info!(
                    "Received binary message from {}: {} bytes",
                    state.args.addr,
                    data.len()
                );

                match &state.channel_state {
                    ChannelState::Opening { .. } => {
                        panic!("Received binary message before handshake: {} bytes", data.len());
                    }
                    ChannelState::Open { remote_introduction, channel_id, .. } => {
                        let (msg, consumed) = bincode::decode_from_slice::<CrossGatewayMessage, _>(&data, bincode::config::standard())                            ?;
                        assert!(consumed == data.len(), "Consumed {} bytes, but {} bytes were sent", consumed, data.len());
                        info!("Received message from {}: {:?}", state.args.addr, msg);

                        match msg {
                            CrossGatewayMessage::RequestActorByName(id, name) => {

                                // Look up the named actor in our local registry
                                let opaque_id = state.named_actors.get(&name).cloned();
                                
                                let response = match opaque_id {
                                    Some(opaque_id) => {
                                        // Construct a RemoteActorId for the actor
                                        let remote_id = RemoteActorId {
                                            connection_key: channel_id.clone(),
                                            side: state.args.local_id.clone(), 
                                            id: opaque_id.clone(),
                                        };
                                        
                                        Ok(remote_id)
                                    }
                                    None => Err(ActorRequestError::ActorNotFound),
                                };
                                
                                // Send response back
                                let response_msg = CrossGatewayMessage::ResponseActorByName(id, response);
                                let data = bincode::encode_to_vec(response_msg, bincode::config::standard())?;
                                state.args.sender.send(RawMessage::Binary(data)).await?;
                            }
                            
                            CrossGatewayMessage::RequestActorById(id, opaque_id) => {
                                // Look up the actor in our registry
                                let actor_id = state.published_actors.get(&opaque_id);
                                
                                let response = match actor_id {
                                    Some(actor_id) => {
                                        // Construct a RemoteActorId for the actor
                                        let remote_id = RemoteActorId {
                                            connection_key:  channel_id.clone(),
                                            side: state.args.local_id.clone(),
                                            id: opaque_id.clone(),
                                        };
                                        
                                        Ok(remote_id)
                                    }
                                    None => Err(ActorRequestError::ActorNotFound),
                                };
                                
                                // Send response back
                                let response_msg = CrossGatewayMessage::ResponseActorById(id, response);
                                let data = bincode::encode_to_vec(response_msg, bincode::config::standard())?;
                                state.args.sender.send(RawMessage::Binary(data)).await?;
                            }
                            
                            CrossGatewayMessage::ResponseActorByName(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: GatewayResult<RemoteActorId> = response.map_err(|err|err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {}", id);
                                }
                            }
                            
                            CrossGatewayMessage::ResponseActorById(id, response) => {
                                // Handle response to our earlier request
                                if let Some(reply_port) = state.open_requests.remove(&id) {
                                    let mapped: GatewayResult<RemoteActorId> = response.map_err(|err|err.into());
                                    reply_port.send(mapped)?;
                                } else {
                                    error!("Received response for unknown request ID: {}", id);
                                }
                            }
                            
                            CrossGatewayMessage::SendMessage(target_id, data) => {
                                // Find the local actor from the remote target
                                if let Some((local_actor_cell, receiver)) = state.published_actors.get(&target_id.id) {
                                    receiver.receive(
                                        local_actor_cell.clone(),
                                        &data,
                                        ActorSerializationContext {
                                            connection_key: target_id.connection_key.clone(),
                                            connection: myself.clone(),
                                            default_rpc_port_timeout: state.args.config.default_rpc_port_timeout,
                                            local_connection_side: state.args.local_id.clone(),
                                            remote_connection_side: remote_introduction.this_side_id.clone(),
                                        },
                                    ).await?;
                                } else {
                                    error!("Remote actor ID {} not found in published actors", target_id.id);
                                }
                            }
                        }
                    }
                }
            }
            WSConnectionMessage::Close => {
                info!("Closing connection to {}", state.args.addr);
                myself.stop(Some("Connection closed".into()));
            }

            WSConnectionMessage::PublishNamedActor(name, actor_cell, receiver, reply) => {
                let ChannelState::Open { channel_id, remote_introduction, .. } = &state.channel_state else {
                    // todo: actually handle error state
                    return Ok(());
                };

                let existing = state
                    .published_actors
                    .iter()
                    .find(|(k, (v, _))| v.get_id() == actor_cell.get_id());
                
                let opaque_actor_id = match existing {
                    Some((k, v)) => {
                        info!(
                            "Actor with id {} was already published under {}",
                            actor_cell.get_id(),
                            k.clone()
                        );
                        k.clone()
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
                            .insert(new_id.clone(), (actor_cell, receiver));
                        new_id
                    }
                };

                // note: this overrides an already published actor of the same name.
                match state
                    .named_actors
                    .insert(name.clone(), opaque_actor_id.clone())
                {
                    Some(_) => info!(
                        "Actor with name {} already existed and was overwritten",
                        name
                    ),
                    None => info!("Actor with name {} published", name),
                }

                let remote_actor_id = RemoteActorId {
                    connection_key: channel_id.clone(),
                    side: remote_introduction.this_side_id.clone(),
                    id: opaque_actor_id,
                };

                if let Some(rpc) = reply {
                    rpc.send(remote_actor_id)?;
                }
            }

            WSConnectionMessage::PublishActor(actor_cell, receiver, rpc) => {
                let ChannelState::Open { channel_id, .. } = &state.channel_state else {
                    // todo: actually handle error state
                    return Ok(());
                };


                todo!()
            }

            WSConnectionMessage::QueryNamedRemoteActor(name, reply) => {
                let ChannelState::Open { channel_id, .. } = &state.channel_state else {
                    // todo: actually handle error state
                    return Ok(());
                };

                let request_id = state.next_request_id;
                state.next_request_id += 1;
                
                state.open_requests.insert(request_id, reply);
                
                let request = CrossGatewayMessage::RequestActorByName(request_id, name);
                let bytes = bincode::encode_to_vec(request, bincode::config::standard())?;
                state.args.sender.send(RawMessage::Binary(bytes)).await?;
            }

            WSConnectionMessage::TransmitMessage(target, msg_f) => {
                let ChannelState::Open { channel_id, .. } = &state.channel_state else {
                    // todo: actually handle error state
                    return Ok(());
                };

                let bytes = msg_f(ActorSerializationContext {
                    connection_key: channel_id.clone(),
                    connection: myself.clone(),
                    default_rpc_port_timeout: state.args.config.default_rpc_port_timeout,
                    local_connection_side: state.args.local_id.clone(),
                    remote_connection_side: target.side.clone(),
                }).await?;

                let request = CrossGatewayMessage::SendMessage(target, bytes.into_boxed_slice());
                let bytes = bincode::encode_to_vec(request, bincode::config::standard())?;
                state.args.sender.send(RawMessage::Binary(bytes)).await?;
            }
        }
        Ok(())
    }
}

// Gateway
// -------------------------------------------------------------------------------------------------------

// Messages for the gateway actor
#[derive(RactorMessage)]
pub enum WSGatewayMessage {
    Connected(
        SocketAddr,
        WebSocketSink,
        RpcReplyPort<ActorRef<WSConnectionMessage>>,
    ),

    GetAllConnections(RpcReplyPort<Vec<ActorRef<WSConnectionMessage>>>),
}

// Gateway actor state
pub struct WSGateway;
pub struct WSGatewayState {
    args: WSGatewayArgs,
    connections: HashMap<ActorId, (SocketAddr, ActorRef<WSConnectionMessage>, JoinHandle<()>)>,
}

#[derive(RactorMessage)]
pub struct OnActorConnectedMessage {
    pub addr: SocketAddr,
    pub actor_ref: ActorRef<WSConnectionMessage>,
}

pub struct WSGatewayArgs {
    pub on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
}

// Gateway actor implementation
#[async_trait]
impl Actor for WSGateway {
    type Msg = WSGatewayMessage;
    type State = WSGatewayState;
    type Arguments = WSGatewayArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        info!("WebSocket gateway actor started");
        Ok(WSGatewayState {
            args,
            connections: HashMap::new(),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            WSGatewayMessage::Connected(addr, ws_stream, reply) => {
                info!("New WebSocket connection from: {}", addr);

                // Create a new connection actor
                let (actor_ref, handle) = ractor::Actor::spawn_linked(
                    None,
                    WSConnection,
                    WSConnectionArgs {
                        addr,
                        sender: ws_stream,
                        local_id: LocalConnectionId(rand::random()),
                        config: PortalConfig {
                            default_rpc_port_timeout: Duration::from_secs(120),
                        },
                    },
                    myself.get_cell(),
                )
                .await?;

                // Store the new connection
                state
                    .connections
                    .insert(actor_ref.get_id(), (addr, actor_ref.clone(), handle));

                // Reply with the connection actor reference
                reply.send(actor_ref)?;
            }

            WSGatewayMessage::GetAllConnections(reply) => {
                let connections: Vec<_> =
                    state.connections.values().map(|v| &v.1).cloned().collect();
                reply.send(connections)?;
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
            SupervisionEvent::ActorTerminated(actor, _last_state, reason) => {
                if let Some((addr, _, _)) = state.connections.remove(&actor.get_id()) {
                    info!("Connection to {} terminated: {:?}", addr, reason);
                }
            }
            SupervisionEvent::ActorFailed(actor, err) => {
                info!("Actor failed: {:?} - {:?}", actor.get_id(), err);

                if let Some((addr, _, _)) = state.connections.remove(&actor.get_id()) {
                    info!("Connection to {} terminated: {:?}", addr, err);
                }
            }
            _ => (),
        }

        Ok(())
    }
}

// Helper function to create and start the gateway actor
pub async fn start_gateway(
    on_client_connected: Option<ActorRef<OnActorConnectedMessage>>,
) -> Result<ActorRef<WSGatewayMessage>, ractor::ActorProcessingErr> {
    let (gateway_ref, handle) = ractor::Actor::spawn(
        Some(String::from("ws-gateway")),
        WSGateway,
        WSGatewayArgs {
            on_client_connected,
        },
    )
    .await?;

    // question: do I need to detach?
    let _ = handle;

    Ok(gateway_ref)
}

pub async fn receive_loop(
    mut ws_receiver: WebSocketSource,
    addr: SocketAddr,
    actor_ref: ActorRef<WSConnectionMessage>,
) {
    // Process incoming messages
    while let Some(msg) = ws_receiver.next().await {
        match msg {
            Ok(msg) => match msg {
                RawMessage::Text(text) => {
                    if let Err(err) = actor_ref.cast(WSConnectionMessage::Text(text.to_string())) {
                        error!("Error sending text message to actor: {}", err);
                        break;
                    }
                }
                RawMessage::Binary(data) => {
                    if let Err(err) = actor_ref.cast(WSConnectionMessage::Binary(data.to_vec())) {
                        error!("Error sending binary message to actor: {}", err);
                        break;
                    }
                }
                RawMessage::Close(close_frame) => {
                    info!(
                        "Connection with {} closed because of reason: {:?}",
                        addr, close_frame
                    );
                    break;
                }
                _ => {}
            },
            Err(e) => {
                error!("Error receiving message from {}: {}", e, addr);
                break;
            }
        }
    }

    info!("Connection with {} closed", addr);
    let _ = actor_ref.cast(WSConnectionMessage::Close);
}

// ---------------------------------------------------------------------------------

pub struct ProxyActor<T: Send + Sync + ractor::Message + 'static> {
    _a: PhantomData<T>,
}
impl<T: Send + Sync + ractor::Message + 'static> Default for ProxyActor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync + ractor::Message + 'static> ProxyActor<T> {
    pub fn new() -> Self {
        ProxyActor { _a: PhantomData }
    }
}

#[derive(bincode::Encode, bincode::Decode, Debug, Clone)]
pub struct RemoteActorId {
    /// the connection key uniquely identifies the connection, it is the same ID on both sides
    pub connection_key: ConnectionKey,
    /// this id identifies the side, it is different between the two sides of a single connection
    pub side: LocalConnectionId,
    /// the unique id of the actor
    pub id: OpaqueActorId,
}

pub struct ProxyActorState {
    pub args: ProxyActorArgs,
}
pub struct ProxyActorArgs {
    remote_actor_id: ActorId,
}

#[async_trait]
impl<T: Send + Sync + ractor::Message + 'static> Actor for ProxyActor<T> {
    type Msg = T;
    type State = ProxyActorState;
    type Arguments = ProxyActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(ProxyActorState { args })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        todo!();

        Ok(())
    }
}
