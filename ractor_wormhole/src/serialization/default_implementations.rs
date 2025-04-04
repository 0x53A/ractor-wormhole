use ractor::{ActorRef, RpcReplyPort, async_trait};

use super::*;

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl<T: ContextSerializable + ractor::Message + Send + Sync + 'static + std::fmt::Debug>
    ContextSerializable for ActorRef<T>
{
    async fn serialize(self, ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        ctx.serialize_actor_ref(&self).await
    }

    async fn deserialize(
        ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        ctx.deserialize_actor_ref(data).await
    }
}

#[async_trait]
impl<T: ContextSerializable + Send + Sync + 'static + std::fmt::Debug> ContextSerializable
    for RpcReplyPort<T>
{
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
// -------------------------------------------------------------------------------------------------------

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

// -------------------------------------------------------------------------------------------------------

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

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextSerializable for u32 {
    async fn serialize(self, _ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(4);
        buffer.extend_from_slice(&self.to_le_bytes());
        Ok(buffer)
    }

    async fn deserialize(
        _ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        assert_eq!(data.len(), 4);
        Ok(u32::from_le_bytes(data[0..4].try_into()?))
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextSerializable for String {
    async fn serialize(self, _ctx: &ActorSerializationContext) -> SerializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len());
        let length = self.len() as u64;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(self.as_bytes());
        Ok(buffer)
    }

    async fn deserialize(
        _ctx: &ActorSerializationContext,
        data: &[u8],
    ) -> SerializationResult<Self> {
        let length = u64::from_le_bytes(data[0..8].try_into()?) as usize;
        let string_data = &data[8..8 + length];
        Ok(String::from_utf8(string_data.to_vec())?)
    }
}
