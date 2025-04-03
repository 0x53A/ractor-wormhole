use std::marker::PhantomData;

use ractor::{async_trait, concurrency::Duration, Actor, ActorProcessingErr, ActorRef, RpcReplyPort};

use crate::gateway::{ConnectionId, WSConnectionMessage};

pub struct ActorSerializationContext {
    connection_id: ConnectionId,
    connection: ActorRef<WSConnectionMessage>,
}

impl ActorSerializationContext {

    pub fn serialize_replychannel<T>(&self, rpc: RpcReplyPort<T>) -> Vec<u8> {
        // when serializing a RpcReplyPort, we create an actor here in this local system. The second RpcReplyPort that will be created on the other side, will send a message to this actor.
        // When the message is received, the actor will forward it to the original RpcReplyPort and stop itself.


        todo!()
    }

    pub fn serialize_actor_ref<T>(&self, actor_ref: ActorRef<T>) -> Vec<u8>  {
        todo!()
    }

    pub fn deserialize_replychannel<T: Send + Sync + 'static>(&self) -> RpcReplyPort<T> {

        let timeout: Option<Duration> = todo!(); // todo
        let remote_actor_ref : RemoteActorId = todo!();

        let (tx, rx) = tokio::sync::oneshot::channel::<T>();

        tokio::spawn(async {
            let result = rx.await;
            match result {
                Ok(msg) => {
                    self.connection.send_message(WSConnectionMessage::TransmitMessage(remote_actor_ref, ))
                }
                Err(_) => {
                    // we don't **need** to do anything, but we should still notify the remote actor so it can kill itself.
                }
            }
        });

        let port : RpcReplyPort<T> = if let Some(t) = timeout {
            (tx, t).into()
        } else {
            tx.into()
        };

        port
    }
    pub fn deserialize_actor_ref<T>(&self) -> ActorRef<T> {
        todo!()
    }

}

/// Handles (de)serialization of messages to bytes.
/// All serialization schemes must be platform independent, strings will be serialized in utf8, all numbers are little endian.
pub trait ContextSerializable {
    fn serialize(self, ctx: &ActorSerializationContext) -> Vec<u8>;
    fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> Self where Self: Sized;
}

// -----------------------------------------


impl<T> ContextSerializable for ActorRef<T> {
    fn serialize(self, ctx: &ActorSerializationContext) -> Vec<u8> {
        ctx.serialize_actor_ref(self)
    }

    fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> Self {
        ctx.deserialize_actor_ref()
    }
}

impl<T> ContextSerializable for RpcReplyPort<T> {
    fn serialize(self, ctx: &ActorSerializationContext) -> Vec<u8> {
        ctx.serialize_replychannel(self)
    }

    fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> Self {
        ctx.deserialize_replychannel()
    }
}
// -----------------------------------------


/// the serialization scheme for a Vec is: header: length:u64 + n * [element_size:u64 + element_bytes]
impl<T: ContextSerializable> ContextSerializable for Vec<T> {
    fn serialize(self, ctx: &ActorSerializationContext) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(8 + self.len() * 8);
        let count = self.len() as u64;
        buffer.extend_from_slice(&count.to_le_bytes());        

        for element in self {
            let element_bytes = element.serialize(ctx);
            let length: u64 = element_bytes.len() as u64;
            buffer.extend_from_slice(&length.to_le_bytes());
            buffer.extend_from_slice(&element_bytes);
        }

        buffer
    }

    fn deserialize(ctx: &ActorSerializationContext, data: &[u8]) -> Self {
        let mut offset = 0;

        let count = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
        offset += 8;

        let mut buffer = Vec::with_capacity(count);

        while offset < data.len() {
            let length = u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap()) as usize;
            offset += 8;
            let element_data = &data[offset..offset + length];
            buffer.push(T::deserialize(ctx, element_data));
            offset += length;
        }

        buffer
    }
}

impl ContextSerializable for Vec<u8> {
    fn serialize(self, _ctx: &ActorSerializationContext) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(8 + self.len());
        let length = self.len() as u64;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(&self);
        buffer
    }

    fn deserialize(_ctx: &ActorSerializationContext, data: &[u8]) -> Self {
        let length = u64::from_le_bytes(data[0..8].try_into().unwrap()) as usize;
        data[8..8 + length].to_vec()
    }
}



// -----------------------------------------

pub struct RpcProxyMsg<T> {
    pub data: T
}

pub struct RpcProxyActor<T: Send + Sync + 'static> {
    _a: PhantomData<T>,
}
impl<T: Send + Sync + ractor::Message + 'static> RpcProxyActor<T> {
    pub fn new() -> Self {
        RpcProxyActor {
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
impl<T: Send + Sync + ractor::Message + 'static> Actor for RpcProxyActor<T> {
    type Msg = T;
    type State = ProxyActorState;
    type Arguments = ProxyActorArgs;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {

    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {

    }
}