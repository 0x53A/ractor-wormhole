//! Core FFI infrastructure: Runtime and Connection management.
//!
//! This module provides:
//! - `WormholeRuntime`: The main entry point, manages the Tokio runtime and Nexus
//! - `WormholeConnection`: A connection to a remote peer
//! - Actor creation methods for each actor type

use std::sync::{Arc, Mutex, RwLock};

use futures::StreamExt;
use ractor::ActorRef;
use tokio::sync::mpsc;

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::{start_nexus, NexusActorMessage, RemoteActorId},
    portal::{Portal, PortalActorMessage},
    util::FnActor,
};

use crate::callbacks::{ChatClientHandler, ChatServerHandler, FfiConduitMessage, HubHandler, TransportCallback};
use crate::error::WormholeError;
use crate::ffi_actors::{
    FfiChatClientActorLocal, FfiChatClientActorRef, FfiChatServerActorLocal, FfiChatServerActorRef,
    FfiHubActorLocal, FfiHubActorRef,
};
use crate::ffi_messages::FfiChatClientMessage;
use crate::ffi_rpc::{FfiAckReplyPort, FfiConnectReplyPort, FfiUserListReplyPort};
use crate::messages::{ChatClientMessage, ChatServerMessage, HubMessage};

// =============================================================================
// Remote Actor ID (FFI-safe)
// =============================================================================

/// FFI-safe representation of a remote actor ID.
#[derive(uniffi::Record, Clone, Debug)]
pub struct FfiRemoteActorId {
    pub connection_key: u128,
    pub side: u128,
    pub id: u128,
}

impl From<RemoteActorId> for FfiRemoteActorId {
    fn from(id: RemoteActorId) -> Self {
        FfiRemoteActorId {
            connection_key: id.connection_key.0,
            side: id.side.0,
            id: id.id.0,
        }
    }
}

impl From<FfiRemoteActorId> for RemoteActorId {
    fn from(id: FfiRemoteActorId) -> Self {
        RemoteActorId {
            connection_key: ractor_wormhole::portal::ConduitID(id.connection_key),
            side: ractor_wormhole::portal::LocalPortalId(id.side),
            id: ractor_wormhole::portal::OpaqueActorId(id.id),
        }
    }
}

// =============================================================================
// WormholeRuntime
// =============================================================================

/// The main runtime for ractor-wormhole.
#[derive(uniffi::Object)]
pub struct WormholeRuntime {
    pub(crate) runtime: tokio::runtime::Runtime,
    nexus: RwLock<Option<ActorRef<NexusActorMessage>>>,
}

impl WormholeRuntime {
    pub fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        self.runtime.block_on(f)
    }

    pub fn spawn<F>(&self, f: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.runtime.spawn(f);
    }

    pub(crate) fn get_nexus(&self) -> Option<ActorRef<NexusActorMessage>> {
        self.nexus.read().unwrap().clone()
    }
}

#[uniffi::export]
impl WormholeRuntime {
    /// Create a new WormholeRuntime.
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, WormholeError> {
        #[cfg(target_os = "android")]
        android_logger::init_once(
            android_logger::Config::default()
                .with_max_level(log::LevelFilter::Debug)
                .with_tag("RactorWormhole"),
        );

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .map_err(|e| WormholeError::InternalError {
                reason: e.to_string(),
            })?;

        let nexus = runtime.block_on(async {
            start_nexus(None, None)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })
        })?;

        Ok(Arc::new(WormholeRuntime {
            runtime,
            nexus: RwLock::new(Some(nexus)),
        }))
    }

    /// Shutdown the runtime.
    pub fn shutdown(&self) {
        let mut nexus = self.nexus.write().unwrap();
        if let Some(n) = nexus.take() {
            n.stop(Some("Shutdown requested".to_string()));
        }
    }

    // =========================================================================
    // Actor Creation Methods
    // =========================================================================

    /// Create a ChatClient actor.
    ///
    /// Messages are delivered via the handler's `receive` method.
    pub fn create_chat_client_actor(
        self: &Arc<Self>,
        handler: Box<dyn ChatClientHandler>,
    ) -> Result<Arc<FfiChatClientActorLocal>, WormholeError> {
        let handler: Arc<dyn ChatClientHandler> = Arc::from(handler);

        let actor_ref = self.runtime.block_on(async {
            let h = handler.clone();

            let (actor, _) = FnActor::<ChatClientMessage>::start_fn(async move |mut ctx| {
                while let Some(msg) = ctx.rx.recv().await {
                    // Convert internal message to FFI message and call handler
                    let ffi_msg: FfiChatClientMessage = msg.into();
                    h.receive(ffi_msg);
                }
            })
            .await
            .map_err(|e| WormholeError::InternalError {
                reason: e.to_string(),
            })?;

            Ok::<_, WormholeError>(actor)
        })?;

        Ok(Arc::new(FfiChatClientActorLocal {
            runtime: Arc::clone(self),
            actor_ref: RwLock::new(Some(actor_ref)),
        }))
    }

    /// Create a ChatServer actor.
    ///
    /// Messages are delivered via the handler's methods.
    pub fn create_chat_server_actor(
        self: &Arc<Self>,
        handler: Box<dyn ChatServerHandler>,
    ) -> Result<Arc<FfiChatServerActorLocal>, WormholeError> {
        let handler: Arc<dyn ChatServerHandler> = Arc::from(handler);

        let actor_ref = self.runtime.block_on(async {
            let h = handler.clone();

            let (actor, _) = FnActor::<ChatServerMessage>::start_fn(async move |mut ctx| {
                while let Some(msg) = ctx.rx.recv().await {
                    match msg {
                        ChatServerMessage::PostMessage(content, rpc) => {
                            let reply_port = Arc::new(FfiAckReplyPort::new(rpc));
                            h.receive_post_message(content.0, reply_port);
                        }
                        ChatServerMessage::ListUsers(rpc) => {
                            let reply_port = Arc::new(FfiUserListReplyPort::new(rpc));
                            h.receive_list_users(reply_port);
                        }
                    }
                }
            })
            .await
            .map_err(|e| WormholeError::InternalError {
                reason: e.to_string(),
            })?;

            Ok::<_, WormholeError>(actor)
        })?;

        Ok(Arc::new(FfiChatServerActorLocal {
            runtime: Arc::clone(self),
            actor_ref: RwLock::new(Some(actor_ref)),
        }))
    }

    /// Create a Hub actor.
    ///
    /// Connection requests are delivered via the handler's `receive_connect` method.
    pub fn create_hub_actor(
        self: &Arc<Self>,
        handler: Box<dyn HubHandler>,
    ) -> Result<Arc<FfiHubActorLocal>, WormholeError> {
        let handler: Arc<dyn HubHandler> = Arc::from(handler);
        let runtime_clone = Arc::clone(self);

        let actor_ref = self.runtime.block_on(async {
            let h = handler.clone();
            let rt = runtime_clone.clone();

            let (actor, _) = FnActor::<HubMessage>::start_fn(async move |mut ctx| {
                while let Some(msg) = ctx.rx.recv().await {
                    match msg {
                        HubMessage::Connect(client_ref, rpc) => {
                            // Wrap the client ref so Kotlin can send messages to it
                            let client_actor = Arc::new(FfiChatClientActorRef {
                                runtime: rt.clone(),
                                actor_ref: client_ref,
                            });
                            let reply_port = Arc::new(FfiConnectReplyPort::new(rpc));
                            h.receive_connect(client_actor, reply_port);
                        }
                    }
                }
            })
            .await
            .map_err(|e| WormholeError::InternalError {
                reason: e.to_string(),
            })?;

            Ok::<_, WormholeError>(actor)
        })?;

        Ok(Arc::new(FfiHubActorLocal {
            runtime: Arc::clone(self),
            actor_ref: RwLock::new(Some(actor_ref)),
        }))
    }

    /// Create a new connection using the provided transport callback.
    pub fn create_connection(
        self: &Arc<Self>,
        identifier: String,
        transport: Box<dyn TransportCallback>,
    ) -> Result<Arc<WormholeConnection>, WormholeError> {
        let nexus = self
            .get_nexus()
            .ok_or(WormholeError::RuntimeNotInitialized)?;

        let (tx_to_rust, rx_from_kotlin) = mpsc::unbounded_channel::<ConduitMessage>();
        let (tx_to_kotlin, mut rx_for_kotlin) = mpsc::unbounded_channel::<ConduitMessage>();

        let transport: Arc<dyn TransportCallback> = Arc::from(transport);

        let transport_clone = transport.clone();
        self.runtime.spawn(async move {
            while let Some(msg) = rx_for_kotlin.recv().await {
                transport_clone.send_message(msg.into());
            }
            transport_clone.close();
        });

        let sink = create_sink_from_sender(tx_to_kotlin);
        let source = create_source_from_receiver(rx_from_kotlin);

        let portal = self.runtime.block_on(async {
            ractor_wormhole::conduit::from_sink_source(nexus, identifier.clone(), sink, source)
                .await
                .map_err(|e| WormholeError::ConnectionFailed {
                    reason: e.to_string(),
                })
        })?;

        Ok(Arc::new(WormholeConnection {
            runtime: Arc::clone(self),
            portal: RwLock::new(Some(portal)),
            tx_to_rust: Mutex::new(Some(tx_to_rust)),
            identifier,
        }))
    }
}

// =============================================================================
// WormholeConnection
// =============================================================================

/// A connection to a remote peer.
#[derive(uniffi::Object)]
pub struct WormholeConnection {
    pub(crate) runtime: Arc<WormholeRuntime>,
    pub(crate) portal: RwLock<Option<ActorRef<PortalActorMessage>>>,
    tx_to_rust: Mutex<Option<mpsc::UnboundedSender<ConduitMessage>>>,
    identifier: String,
}

#[uniffi::export]
impl WormholeConnection {
    /// Feed a message received from the network into Rust.
    pub fn on_message_received(&self, message: FfiConduitMessage) -> Result<(), WormholeError> {
        let tx = self
            .tx_to_rust
            .lock()
            .unwrap()
            .clone()
            .ok_or(WormholeError::ConnectionFailed {
                reason: "Connection closed".into(),
            })?;

        tx.send(message.into())
            .map_err(|e| WormholeError::SendFailed {
                reason: e.to_string(),
            })
    }

    /// Wait for the handshake to complete.
    pub fn wait_for_handshake(&self, timeout_ms: u64) -> Result<(), WormholeError> {
        use ractor_wormhole::util::ActorRef_Ask;

        let portal = self.get_portal()?;

        self.runtime.block_on(async {
            portal
                .ask(
                    PortalActorMessage::WaitForHandshake,
                    Some(std::time::Duration::from_millis(timeout_ms)),
                )
                .await
                .map_err(|_| WormholeError::HandshakeTimeout)
        })
    }

    /// Get the connection identifier.
    pub fn get_identifier(&self) -> String {
        self.identifier.clone()
    }

    /// Disconnect and close the connection.
    pub fn disconnect(&self) {
        let _ = self.tx_to_rust.lock().unwrap().take();
        if let Some(portal) = self.portal.write().unwrap().take() {
            portal.stop(Some("Disconnected by client".into()));
        }
    }

    // =========================================================================
    // Publish Local Actors
    // =========================================================================

    /// Publish a Hub actor so remote peers can connect to it.
    pub fn publish_hub_actor(
        &self,
        name: String,
        actor: &Arc<FfiHubActorLocal>,
    ) -> Result<(), WormholeError> {
        let portal = self.get_portal()?;
        let actor_ref = actor.get_actor_ref().ok_or(WormholeError::ActorStopped)?;

        self.runtime.block_on(async {
            portal
                .publish_named_actor(name, actor_ref)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })
        })
    }

    /// Publish a ChatServer actor so remote peers can send messages.
    pub fn publish_chat_server_actor(
        &self,
        name: String,
        actor: &Arc<FfiChatServerActorLocal>,
    ) -> Result<(), WormholeError> {
        let portal = self.get_portal()?;
        let actor_ref = actor.get_actor_ref().ok_or(WormholeError::ActorStopped)?;

        self.runtime.block_on(async {
            portal
                .publish_named_actor(name, actor_ref)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })
        })
    }

    /// Publish a ChatClient actor so the server can send notifications.
    pub fn publish_chat_client_actor(
        &self,
        name: String,
        actor: &Arc<FfiChatClientActorLocal>,
    ) -> Result<(), WormholeError> {
        let portal = self.get_portal()?;
        let actor_ref = actor.get_actor_ref().ok_or(WormholeError::ActorStopped)?;

        self.runtime.block_on(async {
            portal
                .publish_named_actor(name, actor_ref)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })
        })
    }

    // =========================================================================
    // Query Remote Actors
    // =========================================================================

    /// Query for a remote Hub actor by name.
    pub fn get_remote_hub_actor(&self, name: String) -> Result<Arc<FfiHubActorRef>, WormholeError> {
        let portal = self.get_portal()?;

        self.runtime.block_on(async {
            let remote_id = self.query_named_actor_internal(&portal, &name).await?;

            let actor_ref: ActorRef<HubMessage> = portal
                .instantiate_proxy_for_remote_actor(remote_id)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })?;

            Ok(Arc::new(FfiHubActorRef {
                runtime: self.runtime.clone(),
                actor_ref,
            }))
        })
    }

    /// Query for a remote ChatServer actor by name.
    pub fn get_remote_chat_server_actor(
        &self,
        name: String,
    ) -> Result<Arc<FfiChatServerActorRef>, WormholeError> {
        let portal = self.get_portal()?;

        self.runtime.block_on(async {
            let remote_id = self.query_named_actor_internal(&portal, &name).await?;

            let actor_ref: ActorRef<ChatServerMessage> = portal
                .instantiate_proxy_for_remote_actor(remote_id)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })?;

            Ok(Arc::new(FfiChatServerActorRef {
                runtime: self.runtime.clone(),
                actor_ref,
            }))
        })
    }

    /// Query for a remote ChatClient actor by name.
    pub fn get_remote_chat_client_actor(
        &self,
        name: String,
    ) -> Result<Arc<FfiChatClientActorRef>, WormholeError> {
        let portal = self.get_portal()?;

        self.runtime.block_on(async {
            let remote_id = self.query_named_actor_internal(&portal, &name).await?;

            let actor_ref: ActorRef<ChatClientMessage> = portal
                .instantiate_proxy_for_remote_actor(remote_id)
                .await
                .map_err(|e| WormholeError::InternalError {
                    reason: e.to_string(),
                })?;

            Ok(Arc::new(FfiChatClientActorRef {
                runtime: self.runtime.clone(),
                actor_ref,
            }))
        })
    }
}

impl WormholeConnection {
    fn get_portal(&self) -> Result<ActorRef<PortalActorMessage>, WormholeError> {
        self.portal
            .read()
            .unwrap()
            .clone()
            .ok_or(WormholeError::ConnectionFailed {
                reason: "Connection closed".into(),
            })
    }

    async fn query_named_actor_internal(
        &self,
        portal: &ActorRef<PortalActorMessage>,
        name: &str,
    ) -> Result<RemoteActorId, WormholeError> {
        use ractor_wormhole::util::ActorRef_Ask;

        let result = portal
            .ask(
                |rpc| PortalActorMessage::QueryNamedRemoteActor(name.to_string(), rpc),
                None,
            )
            .await
            .map_err(|e| WormholeError::InternalError {
                reason: e.to_string(),
            })?;

        result.map_err(|_| WormholeError::ActorNotFound {
            name: name.to_string(),
        })
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

fn create_sink_from_sender(sender: mpsc::UnboundedSender<ConduitMessage>) -> ConduitSink {
    let sink = futures::sink::unfold(sender, |sender, msg: ConduitMessage| async move {
        sender
            .send(msg)
            .map_err(|e| anyhow::anyhow!("Send error: {}", e))?;
        Ok::<_, ConduitError>(sender)
    });

    Box::pin(sink)
}

fn create_source_from_receiver(receiver: mpsc::UnboundedReceiver<ConduitMessage>) -> ConduitSource {
    let stream =
        tokio_stream::wrappers::UnboundedReceiverStream::new(receiver).map(|msg| Ok(msg));

    Box::pin(stream)
}

// =============================================================================
// Custom u128 Type for UniFFI
// =============================================================================

uniffi::custom_type!(u128, String, {
    try_lift: |val| val.parse::<u128>().map_err(uniffi::deps::anyhow::Error::msg),
    lower: |obj| obj.to_string(),
    remote,
});
