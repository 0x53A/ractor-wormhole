use async_trait::async_trait;
use ractor::{ActorRef, RpcReplyPort};

use super::{
    util::{require_buffer_size, require_min_buffer_size},
    *,
};

use static_assertions::{const_assert, const_assert_eq};

// -------------------------------------------------------------------------------------------------------

// This file contains the library provided implementations for ContextTransmaterializable.
//
// Notes:
//   * All buffers passed should be exactly the size of the data.
//   * For dynamically sized data, where the length is not known in advance,
//      we need to validate at the end of the deserialization routine that all data has been consumed.
//   * We serialize numbers as little endian, strings as utf-8.

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl<T: ContextTransmaterializable + ractor::Message + Send + Sync + 'static + std::fmt::Debug>
    ContextTransmaterializable for ActorRef<T>
{
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        ctx.immaterialize_actor_ref(&self).await
    }

    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        ctx.rematerialize_actor_ref(data).await
    }
}

#[async_trait]
impl<T: ContextTransmaterializable + Send + Sync + 'static + std::fmt::Debug>
    ContextTransmaterializable for RpcReplyPort<T>
{
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        ctx.immaterialize_replychannel(self).await
    }

    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        ctx.rematerialize_replychannel(data).await
    }
}
// -------------------------------------------------------------------------------------------------------

/// the serialization scheme for a Vec is: header: length:u64 + n * [element_size:u64 + element_bytes]
#[async_trait]
impl<T: ContextTransmaterializable + Send + Sync + 'static> ContextTransmaterializable for Vec<T> {
    default async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len() * 8);
        let count = self.len() as u64;
        buffer.extend_from_slice(&count.to_le_bytes());

        for element in self {
            let element_bytes = element.immaterialize(ctx).await?;
            let length: u64 = element_bytes.len() as u64;
            buffer.extend_from_slice(&length.to_le_bytes());
            buffer.extend_from_slice(&element_bytes);
        }

        Ok(buffer)
    }

    default async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        let mut offset = 0;
        require_min_buffer_size(data, 8)?;
        let count = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;

        let mut buffer = Vec::with_capacity(count);

        while offset < data.len() {
            require_min_buffer_size(data, offset + 8)?;
            let length = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
            offset += 8;
            require_min_buffer_size(data, offset + length)?;
            let element_data = &data[offset..offset + length];
            buffer.push(T::rematerialize(ctx, element_data).await?);
            offset += length;
        }

        require_buffer_size(data, offset)?;
        Ok(buffer)
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextTransmaterializable for Vec<u8> {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len());
        let length = self.len() as u64;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(&self);
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        let length = u64::from_le_bytes(data[0..8].try_into()?) as usize;
        require_buffer_size(data, 8 + length)?;
        Ok(data[8..8 + length].to_vec())
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextTransmaterializable for String {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8 + self.len());
        let length = self.len() as u64;
        buffer.extend_from_slice(&length.to_le_bytes());
        buffer.extend_from_slice(self.as_bytes());
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        let length = u64::from_le_bytes(data[0..8].try_into()?) as usize;
        require_buffer_size(data, 8 + length)?;
        let string_data = &data[8..8 + length];
        Ok(String::from_utf8(string_data.to_vec())?)
    }
}

// -------------------------------------------------------------------------------------------------------

/// Macro to implement ContextTransmaterializable for numeric types
macro_rules! impl_context_transmaterializable_for_le_bytes {
    ($type:ty, $size:expr) => {
        #[async_trait]
        impl ContextTransmaterializable for $type {
            default async fn immaterialize(
                self,
                _ctx: &TransmaterializationContext,
            ) -> TransmaterializationResult<Vec<u8>> {
                let mut buffer = Vec::with_capacity($size);
                buffer.extend_from_slice(&self.to_le_bytes());
                Ok(buffer)
            }

            default async fn rematerialize(
                _ctx: &TransmaterializationContext,
                data: &[u8],
            ) -> TransmaterializationResult<Self> {
                require_buffer_size(data, $size)?;
                Ok(<$type>::from_le_bytes(data[0..$size].try_into()?))
            }
        }
    };
}

macro_rules! impl_context_transmaterializable_for_numeric {
    ($type:ty) => {
        impl_context_transmaterializable_for_le_bytes!($type, std::mem::size_of::<$type>());
    };
}

macro_rules! impl_context_transmaterializable_for_integer {
    ($type:ty) => {
        const_assert_eq!(std::mem::size_of::<$type>(), <$type>::BITS as usize / 8);
        impl_context_transmaterializable_for_numeric!($type);
    };
}

macro_rules! impl_context_transmaterializable_for_float {
    ($type:ty) => {
        impl_context_transmaterializable_for_numeric!($type);
    };
}

// Implement for all integer types
impl_context_transmaterializable_for_integer!(u8);
impl_context_transmaterializable_for_integer!(u16);
impl_context_transmaterializable_for_integer!(u32);
impl_context_transmaterializable_for_integer!(u64);
impl_context_transmaterializable_for_integer!(u128);
impl_context_transmaterializable_for_integer!(i8);
impl_context_transmaterializable_for_integer!(i16);
impl_context_transmaterializable_for_integer!(i32);
impl_context_transmaterializable_for_integer!(i64);
impl_context_transmaterializable_for_integer!(i128);

// Implement for floating point types
const_assert_eq!(std::mem::size_of::<f32>(), 4);
impl_context_transmaterializable_for_float!(f32);
const_assert_eq!(std::mem::size_of::<f64>(), 8);
impl_context_transmaterializable_for_float!(f64);

// -------------------------------------------------------------------------------------------------------

const_assert!(std::mem::size_of::<usize>() <= 8);

#[async_trait]
impl ContextTransmaterializable for usize {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8);
        // note: serialize as u64 so it's platform independent
        buffer.extend_from_slice(&(self as u64).to_le_bytes());
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_buffer_size(data, 8)?;
        Ok(u64::from_le_bytes(data[0..8].try_into()?) as usize)
    }
}

// -------------------------------------------------------------------------------------------------------

const_assert!(std::mem::size_of::<isize>() <= 8);

#[async_trait]
impl ContextTransmaterializable for isize {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(8);
        // note: serialize as i64 so it's platform independent
        buffer.extend_from_slice(&(self as i64).to_le_bytes());
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_buffer_size(data, 8)?;
        Ok(i64::from_le_bytes(data[0..8].try_into()?) as isize)
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextTransmaterializable for bool {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let mut buffer = Vec::with_capacity(1);
        buffer.extend_from_slice(if self { &[1] } else { &[0] });
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_buffer_size(data, 1)?;
        if data[0] == 0 {
            return Ok(false);
        } else if data[0] == 1 {
            return Ok(true);
        } else {
            return Err(anyhow::anyhow!("Invalid boolean value: {}", data[0]));
        }
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextTransmaterializable for () {
    async fn immaterialize(
        self,
        _ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let buffer = Vec::with_capacity(0);
        Ok(buffer)
    }

    async fn rematerialize(
        _ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_buffer_size(data, 0)?;
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------------
