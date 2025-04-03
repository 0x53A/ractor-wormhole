use futures::{
    future::Remote, stream::{SplitSink, SplitStream}, Sink, SinkExt, Stream, StreamExt
};
use log::{error, info};
use ractor::{
    actor, async_trait, concurrency::JoinHandle, message::BoxedMessage, Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent
};
use ractor_cluster_derive::RactorMessage;
use std::{any::Any, collections::HashMap, fmt::Display, marker::PhantomData, net::SocketAddr, pin::Pin};
use tungstenite::Message;

use crate::serialization::ContextSerializable;
// use tokio::net::TcpStream;
// use tokio_tungstenite::{WebSocketStream, tungstenite::protocol::Message};

pub enum RawMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<String>),
    Other,
}

pub type RawError = Box<dyn std::error::Error + Send + Sync>;

pub type WebSocketSink = Pin<Box<dyn Sink<RawMessage, Error = RawError> + Send>>;
pub type WebSocketSource = Pin<Box<dyn Stream<Item = Result<RawMessage, RawError>> + Send>>;


// -------------------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ConnectionId(pub uuid::Uuid);

impl Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}


// -------------------------------------------------------------------------------------------------------


/// Hide the internal actor id behind a uuid; only the connection has the mapping between uuid and real actor id
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OpaqueActorId(pub uuid::Uuid);

impl Display for OpaqueActorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// -------------------------------------------------------------------------------------------------------

pub enum CrossGatewayMessage {

}


// Connection
// -------------------------------------------------------------------------------------------------------

// Messages for the connection actor
#[derive(RactorMessage)]
pub enum WSConnectionMessage {
    // data received from websocket
    Text(String),
    Binary(Vec<u8>),
    Close,

    TransmitMessage(RemoteActorId, Box<dyn ContextSerializable + Send>),

    /// publish a local actor under a known name, making it available to the remote side of the connection.
    /// On the remote side, it can be looked up by name.
    PublishNamedActor(String, ActorCell, RpcReplyPort<RemoteActorId>),

    /// publish a local actor, making it available to the remote side of the connection.
    /// It is published under a random id, which would need to be passed to the remote side through some kind of existing channel.
    PublishActor(ActorCell, RpcReplyPort<RemoteActorId>),

    /// looks up an actor by name on the **remote** side of the connection. Returns None if no actor was registered under that name.
    QueryNamedRemoteActor(String, RpcReplyPort<Option<RemoteActorId>>),

    /// instantiate (a proxy of) the remote actor into the local system
    GetRemoteActorById(RemoteActorId, RpcReplyPort<Option<ActorCell>>),
}

// Connection actor state
struct WSConnection;
pub struct WSConnectionState {
    args: WSConnectionArgs,
    published_actors: HashMap<OpaqueActorId, ActorId>,
    named_actors: HashMap<String, OpaqueActorId>,
}

pub struct WSConnectionArgs {
    connection_id: uuid::Uuid,
    addr: SocketAddr,
    sender: WebSocketSink,
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
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(WSConnectionState { args, published_actors: HashMap::new(), named_actors: HashMap::new() })
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
            }
            WSConnectionMessage::Binary(data) => {
                info!(
                    "Received binary message from {}: {} bytes",
                    state.args.addr,
                    data.len()
                );
            }
            WSConnectionMessage::Close => {
                info!("Closing connection to {}", state.args.addr);
                myself.stop(Some("Connection closed".into()));
            },



            WSConnectionMessage::PublishNamedActor(name, actor_cell, reply) => {

                let opaque_actor_id =
                match state.published_actors.iter().try_find(|(k, v)| Some(**v == actor_cell.get_id())) {
                    Some(Some((k, v))) => {
                        info!("Actor with id {} was already published under {}", actor_cell.get_id(), k.clone());
                        k.clone()
                },
                    _ => {
                        let new_id = OpaqueActorId(uuid::Uuid::new_v4());
                        info!("Actor with id {} published as {}", state.args.connection_id, new_id.0);
                        state.published_actors.insert(new_id.clone(), actor_cell.get_id());
                        new_id
                    }
                };

                // note: this overrides an already published actor of the same name.
                match state.named_actors.insert(name, opaque_actor_id) {
                    Some(_) => info!("Actor with name {} already existed and was overwritten", name),
                    None => info!("Actor with name {} published", name),
                }
                
                let remote_actor_id = RemoteActorId {
                    connection_id: state.args.connection_id,
                    id: opaque_actor_id
                };
                reply.send(remote_actor_id)?;
            },

            WSConnectionMessage::PublishActor(_, _) => {

            },

            WSConnectionMessage::QueryNamedRemoteActor(name, reply) => {
                
                // need to send a message to the other side of the gateway
                
                let bytes: Vec<u8> = todo!();
              
              
                state.args.sender.send(RawMessage::Binary(bytes)).await?;
            },

            WSConnectionMessage::TransmitMessage(target, msg, _) => {

            },
            
            WSConnectionMessage::GetRemoteActorById(_, _) => {
                // need to send a message to the other side of the gateway
                let bytes: Vec<u8> = todo!();
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
    connections: HashMap<ActorId, (uuid::Uuid, SocketAddr, ActorRef<WSConnectionMessage>, JoinHandle<()>)>,
}

pub struct OnActorConnectedMessage {
    addr: SocketAddr,
    actor_ref: ActorRef<WSConnectionMessage>,
}

pub struct WSGatewayArgs {
    on_client_connected: Option<ActorRef<OnActorConnectedMessage>>
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
                let connection_id = uuid::Uuid::new_v4();
                let (actor_ref, handle) = ractor::Actor::spawn_linked(
                    None,
                    WSConnection,
                    WSConnectionArgs {
                        addr,
                        connection_id,
                        sender: ws_stream,
                    },
                    myself.get_cell(),
                )
                .await?;

                // Store the new connection
                state
                    .connections
                    .insert(actor_ref.get_id(), (connection_id, addr, actor_ref.clone(), handle));

                // Reply with the connection actor reference
                reply.send(actor_ref)?;
            },

            WSGatewayMessage::GetAllConnections(reply) => {
                let connection_ids: Vec<ActorId> = state
                    .connections
                    .keys()
                    .cloned()
                    .collect();
                reply.send(connection_ids)?;
            },
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
pub async fn start_gateway(on_client_connected: Option<ActorRef<OnActorConnectedMessage>>) -> Result<ActorRef<WSGatewayMessage>, ractor::ActorProcessingErr> {
    let (gateway_ref, handle) =
        ractor::Actor::spawn(Some(String::from("ws-gateway")), WSGateway, WSGatewayArgs { on_client_connected }).await?;

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
impl<T: Send + Sync + ractor::Message + 'static> ProxyActor<T> {
    pub fn new() -> Self {
        ProxyActor {
            _a: PhantomData,
        }
    }
}

pub struct RemoteActorId {
    pub connection_id: uuid::Uuid,
    pub id: OpaqueActorId,
}

pub struct ProxyActorState {
    pub args: ProxyActorArgs,
}
pub struct ProxyActorArgs {
    remote_actor_id: ActorId
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
        let serialized_msg = message.box_message();
        Ok(())
    }
}