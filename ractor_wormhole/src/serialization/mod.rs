mod default_implementations;
mod rpc_proxy;

pub use rpc_proxy::*;

// -------------------------------------------------------------------------------------------------------

use ractor::{actor::actor_ref, async_trait, concurrency::Duration, Actor, ActorRef, RpcReplyPort};

use crate::{
    gateway::{ConnectionKey, GatewayResult, LocalConnectionId, MsgReceiver, RemoteActorId, UserFriendlyConnection, WSConnectionMessage},
    util::ActorRef_Ask,
};

// -------------------------------------------------------------------------------------------------------

pub type SerializationError = anyhow::Error;

pub type SerializationResult<T> = Result<T, SerializationError>;

// -------------------------------------------------------------------------------------------------------

#[derive(bincode::Encode, bincode::Decode)]
pub struct SerializedRpcReplyPort {
    pub timeout_ms: Option<u128>,
    pub remote_actor_id: RemoteActorId,
}

// -------------------------------------------------------------------------------------------------------

pub struct ActorSerializationContext {
    pub connection_key: ConnectionKey,
    pub local_connection_side: LocalConnectionId,
    pub remote_connection_side: LocalConnectionId,
    pub connection: ActorRef<WSConnectionMessage>,
    /// which timeout to use if the RpcReplyPort doesn't have a timeout set
    pub default_rpc_port_timeout: Duration,
}

pub trait GetReceiver {
    fn get_receiver(&self) -> Box<dyn MsgReceiver + Send>;
}

pub struct Receiver<TMessage> {
    _marker: std::marker::PhantomData<TMessage>,
}

impl<TMessage> Default for Receiver<TMessage> {
    fn default() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<TMessage: ContextSerializable + ractor::Message + Sync> MsgReceiver for Receiver<TMessage> {
    async fn receive(&self, actor: ractor::ActorCell, data: &[u8], ctx: ActorSerializationContext) -> GatewayResult<()> {
        let msg = <TMessage as ContextSerializable>::deserialize(&ctx, data).await?;
        let actor_ref = ActorRef::<TMessage>::from(actor);
        actor_ref.send_message(msg)?;
        Ok(())
    }
}

impl<TMessage: ContextSerializable + ractor::Message + Sync> GetReceiver for ActorRef<TMessage> {
    fn get_receiver(&self) -> Box<dyn MsgReceiver + Send> {
        Box::new(Receiver::<TMessage>::default())
    }
}

impl ActorSerializationContext {
    pub async fn serialize_replychannel<T: ContextSerializable + Send + Sync + 'static>(
        &self,
        rpc: RpcReplyPort<T>,
    ) -> SerializationResult<Vec<u8>> {
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

        let (local_actor, _handle) = Actor::spawn_linked(
            None,
            RpcProxyActor::<T>::new(),
            RpcProxyActorArgs {
                rpc_reply_port: rpc,
            },
            self.connection.get_cell(),
        )
        .await?;

        ractor::time::kill_after(timeout, local_actor.get_cell());

        let receiver = local_actor.get_receiver();

        let published_id = self
            .connection
            .ask(
                |rpc| WSConnectionMessage::PublishActor(local_actor.get_cell(), receiver, rpc),
                None,
            )
            .await?;

        let structured = SerializedRpcReplyPort {
            timeout_ms: Some(timeout.as_millis()),
            remote_actor_id: published_id,
        };
        let serialized = bincode::encode_to_vec(&structured, bincode::config::standard())?;

        Ok(serialized)
    }

    pub async fn serialize_actor_ref<T: ContextSerializable + ractor::Message + Sync>(
        &self,
        actor_ref: &ActorRef<T>,
    ) -> SerializationResult<Vec<u8>> {

        let receiver = actor_ref.get_receiver();

        let published_id = self
            .connection
            .ask(
                |rpc| WSConnectionMessage::PublishActor(actor_ref.get_cell(), receiver, rpc),
                None,
            )
            .await?;

        let serialized = bincode::encode_to_vec(&published_id, bincode::config::standard())?;

        Ok(serialized)
    }

    pub async fn deserialize_replychannel<T: ContextSerializable + Send + Sync + 'static>(
        &self,
        buffer: &[u8],
    ) -> SerializationResult<RpcReplyPort<T>> {
        // deserialize from bytes
        let (structured, consumed): (SerializedRpcReplyPort, _) =
            bincode::decode_from_slice(buffer, bincode::config::standard())?;
        assert!(consumed == buffer.len());

        let timeout: Option<Duration> = structured
            .timeout_ms
            .map(|ms| ractor::concurrency::Duration::from_millis(ms as u64));
        let remote_actor_ref: RemoteActorId = structured.remote_actor_id;

        let actor_cell = self
            .connection.instantiate_proxy_for_remote_actor(remote_actor_ref).await?;

        let actor_ref = ActorRef::<RpcProxyMsg<T>>::from(actor_cell);

        let rpc_port = rpc_reply_port_from_actor_ref(actor_ref, timeout);

        Ok(rpc_port)
    }

    pub async fn deserialize_actor_ref<T: ContextSerializable + ractor::Message + Send + Sync + 'static>(
        &self,
        buffer: &[u8],
    ) -> SerializationResult<ActorRef<T>> {
        let (remote_actor_id, consumed): (RemoteActorId, _) =
            bincode::decode_from_slice(buffer, bincode::config::standard())?;
        assert!(consumed == buffer.len());

        let actor_cell = self
            .connection
            .instantiate_proxy_for_remote_actor(remote_actor_id).await?;

        let actor_ref = ActorRef::<T>::from(actor_cell);

        Ok(actor_ref)
    }
}

// -------------------------------------------------------------------------------------------------------

/// Handles (de)serialization of messages to bytes.
/// All serialization schemes must be platform independent.
#[async_trait]
pub trait ContextSerializable {
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>>;
    async fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized;
}

// -------------------------------------------------------------------------------------------------------

pub mod serialization_proxies {

    pub use ::ractor::async_trait;

    use super::*;

    #[cfg(feature = "serde")]
    pub mod serde_proxy {

        use super::*;

        pub fn serialize<T: serde::Serialize>(data: T) -> SerializationResult<Vec<u8>> {
            let json = serde_json::to_vec(&data)?;
            Ok(json)
        }

        pub fn deserialize<T: serde::de::DeserializeOwned>(data: &[u8]) -> SerializationResult<T> {
            let deserialized = serde_json::from_slice(data)?;
            Ok(deserialized)
        }
    }

    #[cfg(feature = "bincode")]
    pub mod bincode_proxy {

        use super::*;

        pub fn serialize<T: bincode::Encode>(data: T) -> SerializationResult<Vec<u8>> {
            let json = bincode::encode_to_vec(data, bincode::config::standard())?;
            Ok(json)
        }

        pub fn deserialize<T: bincode::Decode<()>>(data: &[u8]) -> SerializationResult<T> {
            let (deserialized, consumed) =
                bincode::decode_from_slice::<T, _>(data, bincode::config::standard())?;
            assert!(consumed == data.len());
            Ok(deserialized)
        }
    }
}
