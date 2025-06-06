mod default_impl_tupl;
mod default_implementations;
pub mod internal_serializations;
mod rpc_proxy;
mod util;

use internal_serializations::SimpleByteTransmaterializable;
pub use rpc_proxy::*;

// -------------------------------------------------------------------------------------------------------

use crate::{
    nexus::RemoteActorId,
    portal::{BoxedRematerializer, MsgRematerializer, NexusResult, Portal, PortalActorMessage},
    util::ActorRef_Ask,
};
use async_trait::async_trait;
use ractor::{Actor, ActorRef, RpcReplyPort, concurrency::Duration};
use util::require_buffer_size;

// -------------------------------------------------------------------------------------------------------

pub type TransmaterializationError = anyhow::Error;

pub type TransmaterializationResult<T> = Result<T, TransmaterializationError>;

// -------------------------------------------------------------------------------------------------------

/// this represents an RpcReplyPort. A new Actor of type `RpcProxyActor` is created.
#[derive(bincode::Encode, bincode::Decode)]
struct ProxiedRpcReplyPort {
    pub timeout_ms: Option<u128>,
    pub remote_actor_id: RemoteActorId,
}

impl SimpleByteTransmaterializable for ProxiedRpcReplyPort {
    fn immaterialize(&self) -> TransmaterializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn rematerialize(data: &[u8]) -> TransmaterializationResult<Self>
    where
        Self: Sized,
    {
        let (rpc, consumed): (ProxiedRpcReplyPort, _) =
            bincode::decode_from_slice(data, bincode::config::standard())?;
        require_buffer_size(data, consumed)?;
        Ok(rpc)
    }
}

// -------------------------------------------------------------------------------------------------------

pub struct TransmaterializationContext {
    pub connection: ActorRef<PortalActorMessage>,
    /// which timeout to use if the RpcReplyPort doesn't have a timeout set
    pub default_rpc_port_timeout: Duration,
}

pub trait GetRematerializer {
    fn get_rematerializer() -> BoxedRematerializer;
}

/// implementation of trait 'MsgReceiver'
pub struct MsgRematerializerImpl<TMessage> {
    _marker: std::marker::PhantomData<TMessage>,
}

impl<TMessage> Default for MsgRematerializerImpl<TMessage> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<TMessage: ContextTransmaterializable + ractor::Message + Sync> MsgRematerializer
    for MsgRematerializerImpl<TMessage>
{
    async fn rematerialize(
        &self,
        actor: ractor::ActorCell,
        data: &[u8],
        ctx: TransmaterializationContext,
    ) -> NexusResult<()> {
        let msg = <TMessage as ContextTransmaterializable>::rematerialize(&ctx, data).await?;
        let actor_ref = ActorRef::<TMessage>::from(actor);
        actor_ref.send_message(msg)?;
        Ok(())
    }

    fn clone_boxed(&self) -> BoxedRematerializer {
        Box::new(MsgRematerializerImpl::<TMessage>::default())
    }
}

impl<TMessage: ContextTransmaterializable + ractor::Message + Sync> GetRematerializer for TMessage {
    fn get_rematerializer() -> BoxedRematerializer {
        Box::new(MsgRematerializerImpl::<TMessage>::default())
    }
}

impl TransmaterializationContext {
    pub async fn immaterialize_replychannel<
        T: ContextTransmaterializable + Send + Sync + 'static,
    >(
        &self,
        rpc: RpcReplyPort<T>,
    ) -> TransmaterializationResult<Vec<u8>> {
        /*
                When sending a Message with an RpcReplyPort through the portal to a remote actor ...
                ─────────────────────────────────────────────────────────────────────────────────────


            ┌──────────────────────────────┐        () - ()      ┌─────┐
            │Msg(data, reply: RpcReplyPort)│    ==> ()   () ==>  │Actor│
            └──────────────────────────────┘        () - ()      └─────┘

                ─────────────────────────────────────────────────────────────────────────────────────

                            1)proxy actor is created on sending site   3)the RpcReplyPort is reconstructed on the other side
                            2)addr is serialized into the message         from the tx side of a channel, and a task awaits the rx

                                                                        channel(rx, tx)
                                                                                /    \
        ┌────────────┐       ┌─────────────┐     ==> () - () ==>     ┌─────────┐     ┌────────────┐
        │RpcReplyPort│ <==   │RpcProxyActor│         ()   ()         │task:    │ <== │RpcReplyPort│
        └────────────┘       └─────────────┘     <== () - () <==     │ rx.await│     └────────────┘
                                                                     └─────────┘

                    5) the RpcProxyActor receives the data        4) when the RpcReplyPort is triggered, the task reads the value
                        and triggers the RpcReplyPort                 and sends it as a RpcProxyActorMsg through the portal
        */

        let timeout = rpc.get_timeout().unwrap_or(self.default_rpc_port_timeout);

        let (local_actor, _handle) = RpcProxyActor::spawn_linked(
            None,
            RpcProxyActor::<T>::new(),
            RpcProxyActorArgs {
                rpc_reply_port: rpc,
            },
            self.connection.get_cell(),
        )
        .await?;

        ractor::time::kill_after(timeout, local_actor.get_cell());

        let receiver = RpcProxyMsg::<T>::get_rematerializer();

        let published_id = self
            .connection
            .ask(
                |rpc| PortalActorMessage::PublishActor(local_actor.get_cell(), receiver, Some(rpc)),
                None,
            )
            .await?;

        let structured = ProxiedRpcReplyPort {
            timeout_ms: Some(timeout.as_millis()),
            remote_actor_id: published_id,
        };
        let serialized = structured.immaterialize()?;

        Ok(serialized)
    }

    pub async fn immaterialize_actor_ref<T: ContextTransmaterializable + ractor::Message + Sync>(
        &self,
        actor_ref: &ActorRef<T>,
    ) -> TransmaterializationResult<Vec<u8>> {
        let receiver = T::get_rematerializer();

        let published_id = self
            .connection
            .ask(
                |rpc| PortalActorMessage::PublishActor(actor_ref.get_cell(), receiver, Some(rpc)),
                None,
            )
            .await?;

        let serialized = published_id.immaterialize()?;

        Ok(serialized)
    }

    pub async fn rematerialize_replychannel<
        T: ContextTransmaterializable + Send + Sync + 'static + std::fmt::Debug,
    >(
        &self,
        buffer: &[u8],
    ) -> TransmaterializationResult<RpcReplyPort<T>> {
        // deserialize from bytes
        let structured: ProxiedRpcReplyPort = ProxiedRpcReplyPort::rematerialize(buffer)?;

        let timeout: Option<Duration> = structured
            .timeout_ms
            .map(|ms| ractor::concurrency::Duration::from_millis(ms as u64));
        let remote_actor_ref: RemoteActorId = structured.remote_actor_id;

        let actor_cell = self
            .connection
            .instantiate_proxy_for_remote_actor(remote_actor_ref)
            .await?;

        let actor_ref = actor_cell;

        let rpc_port = rpc_reply_port_from_actor_ref(actor_ref, timeout);

        Ok(rpc_port)
    }

    pub async fn rematerialize_actor_ref<
        T: ContextTransmaterializable + ractor::Message + Send + Sync + 'static + std::fmt::Debug,
    >(
        &self,
        buffer: &[u8],
    ) -> TransmaterializationResult<ActorRef<T>> {
        let remote_actor_id = RemoteActorId::rematerialize(buffer)?;

        let actor_cell = self
            .connection
            .instantiate_proxy_for_remote_actor(remote_actor_id)
            .await?;

        let actor_ref = actor_cell;

        Ok(actor_ref)
    }
}

// -------------------------------------------------------------------------------------------------------

/// Handles (de)serialization of messages to bytes.
/// All serialization schemes must be platform independent.
#[async_trait]
pub trait ContextTransmaterializable {
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>>;
    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self>
    where
        Self: Sized;
}

// -------------------------------------------------------------------------------------------------------

pub mod transmaterialization_proxies {

    pub use ::anyhow::anyhow;
    pub use ::async_trait::async_trait;

    #[cfg(any(feature = "serde", feature = "bincode"))]
    use super::*;

    #[cfg(feature = "serde")]
    pub mod serde_proxy {

        use super::*;

        pub fn immaterialize<T: serde::Serialize>(data: T) -> TransmaterializationResult<Vec<u8>> {
            let json = serde_json::to_vec(&data)?;
            Ok(json)
        }

        pub fn rematerialize<T: serde::de::DeserializeOwned>(
            data: &[u8],
        ) -> TransmaterializationResult<T> {
            let deserialized = serde_json::from_slice(data)?;
            Ok(deserialized)
        }
    }

    #[cfg(feature = "bincode")]
    pub mod bincode_proxy {

        use super::*;

        pub fn immaterialize<T: bincode::Encode>(data: T) -> TransmaterializationResult<Vec<u8>> {
            let json = bincode::encode_to_vec(data, bincode::config::standard())?;
            Ok(json)
        }

        pub fn rematerialize<T: bincode::Decode<()>>(data: &[u8]) -> TransmaterializationResult<T> {
            let (deserialized, consumed) =
                bincode::decode_from_slice::<T, _>(data, bincode::config::standard())?;
            assert!(consumed == data.len());
            Ok(deserialized)
        }
    }
}
