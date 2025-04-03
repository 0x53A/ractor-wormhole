use std::marker::PhantomData;

use ractor::{
    Actor, ActorProcessingErr, ActorRef, RpcReplyPort, async_trait, concurrency::Duration,
};

use crate::{
    gateway::{ConnectionId, RemoteActorId, WSConnectionMessage},
    util::ActorRef_Ask,
};

type SerializationResult<T> = Result<T, Box<dyn std::error::Error>>;

pub struct ActorSerializationContext {
    connection_id: ConnectionId,
    connection: ActorRef<WSConnectionMessage>,
    /// which timeout to use if the RpcReplyPort doesn't have a timeout set
    default_rpc_port_timeout: Duration,
}

impl ActorSerializationContext {
    pub async fn serialize_replychannel<T: Send + Sync + 'static>(
        &self,
        rpc: RpcReplyPort<T>,
    ) -> SerializationResult<Vec<u8>> {
        // when serializing a RpcReplyPort, we create an actor here in this local system. The second RpcReplyPort that will be created on the other side, will send a message to this actor.
        // When the message is received, the actor will forward it to the original RpcReplyPort and stop itself.

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

        let published_id = self
            .connection
            .ask(
                |rpc| WSConnectionMessage::PublishActor(local_actor.get_cell(), rpc),
                Some(timeout),
            )
            .await?;

        let serialized = todo!(); //published_id.ser

        Ok(serialized)
    }

    pub async fn serialize_actor_ref<T>(
        &self,
        actor_ref: ActorRef<T>,
    ) -> SerializationResult<Vec<u8>> {
        todo!()
    }

    pub async fn deserialize_replychannel<T: Send + Sync + 'static>(
        &self,
        buffer: &[u8],
    ) -> SerializationResult<RpcReplyPort<T>> {
        let timeout: Option<Duration> = todo!(); // todo
        let remote_actor_ref: RemoteActorId = todo!();

        let (tx, rx) = tokio::sync::oneshot::channel::<T>();

        todo!();
        // tokio::spawn(async {
        //     let result = rx.await;
        //     match result {
        //         Ok(msg) => self
        //             .connection
        //             .send_message(WSConnectionMessage::TransmitMessage(remote_actor_ref)),
        //         Err(_) => {
        //             // we don't **need** to do anything, but we should still notify the remote actor so it can kill itself.
        //         }
        //     }
        // });

        let port: RpcReplyPort<T> = if let Some(t) = timeout {
            (tx, t).into()
        } else {
            tx.into()
        };

        Ok(port)
    }
    pub async fn deserialize_actor_ref<T>(
        &self,
        buffer: &[u8],
    ) -> SerializationResult<ActorRef<T>> {
        todo!()
    }
}

/// Handles (de)serialization of messages to bytes.
/// All serialization schemes must be platform independent, strings will be serialized in utf8, all numbers are little endian.
#[async_trait]
pub trait ContextSerializable {
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>>;
    async fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized;
}

// -----------------------------------------

#[async_trait]
impl<T> ContextSerializable for ActorRef<T> {
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        ctx.serialize_actor_ref(self).await
    }

    async fn deserialize(
        ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        ctx.deserialize_actor_ref(data).await
    }
}

#[async_trait]
impl<T: Send + Sync + 'static> ContextSerializable for RpcReplyPort<T> {
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        ctx.serialize_replychannel(self).await
    }

    async fn deserialize(
        ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        ctx.deserialize_replychannel(data).await
    }
}
// -----------------------------------------

/// the serialization scheme for a Vec is: header: length:u64 + n * [element_size:u64 + element_bytes]
#[async_trait]
impl<T: ContextSerializable + Send + Sync + 'static> ContextSerializable for Vec<T> {
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len() * 8);
        let count = self.len() as u64;
        buffer.extend_from_slice(&count.to_le_bytes());

        for element in self {
            let element_bytes = element.serialize(ctx).await?;
            let length: u64 = element_bytes.len() as u64;
            buffer.extend_from_slice(&length.to_le_bytes());
            buffer.extend_from_slice(&element_bytes);
        }

        Ok(buffer)
    }

    async fn deserialize(
        ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        let mut offset = 0;

        let count = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;

        let mut buffer = Vec::with_capacity(count);

        while offset < data.len() {
            let length = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
            offset += 8;
            let element_data = &data[offset..offset + length];
            buffer.push(T::deserialize(ctx, element_data).await?);
            offset += length;
        }

        Ok(buffer)
    }
}

#[async_trait]
impl ContextSerializable for Vec<u8> {
    async fn serialize(self, _ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len());
        let length = self.len() as u64;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(&self);
        Ok(buffer)
    }

    async fn deserialize(
        _ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        let length = u64::from_le_bytes(data[0..8].try_into()?) as usize;
        Ok(data[8..8 + length].to_vec())
    }
}

// -----------------------------------------

//#[derive(RactorMessage)]
pub struct RpcProxyMsg<T: Send + Sync + 'static> {
    pub data: T,
}

impl<T: Send + Sync + 'static> ractor::Message for RpcProxyMsg<T> {}

pub struct RpcProxyActor<T: Send + Sync + 'static> {
    _a: PhantomData<T>,
}
impl<T: Send + Sync + 'static> Default for RpcProxyActor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Send + Sync + 'static> RpcProxyActor<T> {
    pub fn new() -> Self {
        RpcProxyActor { _a: PhantomData }
    }
}

pub struct RpcProxyActorState<T> {
    rpc_reply_port: Option<RpcReplyPort<T>>,
}
pub struct RpcProxyActorArgs<T> {
    pub rpc_reply_port: RpcReplyPort<T>,
}

#[async_trait]
impl<T: Send + Sync + 'static> Actor for RpcProxyActor<T> {
    type Msg = RpcProxyMsg<T>;
    type State = RpcProxyActorState<T>;
    type Arguments = RpcProxyActorArgs<T>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Starting the RpcProxyActor actor");
        Ok(RpcProxyActorState {
            rpc_reply_port: Some(args.rpc_reply_port),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::info!("RpcProxyActor handle message...");
        if let Some(rrp) = state.rpc_reply_port.take() {
            if let Err(err) = rrp.send(message.data) {
                tracing::error!("Failed to send message to RpcReplyPort: {}", err);
            } else {
                tracing::info!("Message sent to RpcReplyPort successfully");
            }
        }
        myself.stop(None);
        Ok(())
    }
}
