use futures::{
    future::Remote, stream::{SplitSink, SplitStream}, Sink, SinkExt, Stream, StreamExt
};
use log::{error, info};
use ractor::{
    actor, async_trait, concurrency::JoinHandle, Actor, ActorCell, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SupervisionEvent
};
use ractor_cluster_derive::RactorMessage;
use std::{collections::HashMap, marker::PhantomData, net::SocketAddr, pin::Pin};
use tungstenite::Message;
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

// Connection
// -------------------------------------------------------------------------------------------------------

// Messages for the connection actor
#[derive(RactorMessage)]
pub enum WSConnectionMessage {
    Text(String),
    Binary(Vec<u8>),
    Close,
}

// Connection actor state
struct WSConnection;
pub struct WSConnectionState {
    args: WSConnectionArgs,
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
        Ok(WSConnectionState { args })
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

    GetAllConnections(RpcReplyPort<Vec<ActorId>>),

    PublishNamedActor(String, ActorCell, RpcReplyPort<RemoteActorId>),
    GetNamedRemoteActor(String, RpcReplyPort<ActorCell>),

    PublishActor(ActorCell, RpcReplyPort<RemoteActorId>),
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
                let (actor_ref, handle) = ractor::Actor::spawn_linked(
                    None,
                    WSConnection,
                    WSConnectionArgs {
                        addr,
                        sender: ws_stream,
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
            },

            WSGatewayMessage::GetAllConnections(reply) => {
                let connection_ids: Vec<ActorId> = state
                    .connections
                    .keys()
                    .cloned()
                    .collect();
                reply.send(connection_ids)?;
            },

            WSGatewayMessage::PublishNamedActor(name, actor_cell, reply) => {
                // Publish the actor with the given name
                let remote_actor_id = RemoteActorId {
                    connection_id: uuid::Uuid::new_v4(),
                    id: actor_cell.get_id(),
                    addr: actor_cell.get_addr(),
                };
                reply.send(remote_actor_id)?;
            },
            WSGatewayMessage::GetNamedRemoteActor(name, reply) => {
                // Retrieve the actor with the given name
                if let Some(actor_cell) = state.connections.get(&name) {
                    reply.send(actor_cell.clone())?;
                } else {
                    reply.send(ActorCell::default())?;
                }
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
    pub id: ActorId,
    pub addr: SocketAddr,
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
        let serialized_msg = message.serialize();
        Ok(())
    }
}