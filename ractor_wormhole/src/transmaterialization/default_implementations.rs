use async_trait::async_trait;
use ractor::{ActorRef, RpcReplyPort};

use super::{
    util::{i64_to_isize, require_buffer_size, require_min_buffer_size, u64_to_usize, usize_from_u64_le_bytes},
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
        let count = usize_from_u64_le_bytes(&data[offset..offset + 8])?;
        offset += 8;

        let mut buffer = Vec::with_capacity(count);

        while offset < data.len() {
            require_min_buffer_size(data, offset + 8)?;
            let length = usize_from_u64_le_bytes(&data[offset..offset + 8])?;
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
        require_min_buffer_size(data, 8)?;
        let length = usize_from_u64_le_bytes(&data[0..8])?;
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
        require_min_buffer_size(data, 8)?;
        let length = usize_from_u64_le_bytes(&data[0..8])?;
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
        u64_to_usize(u64::from_le_bytes(data[0..8].try_into()?))
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
        i64_to_isize(i64::from_le_bytes(data[0..8].try_into()?))
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

#[async_trait]
impl<T, E> ContextTransmaterializable for Result<T, E>
where
    T: ContextTransmaterializable + Send + Sync + 'static,
    E: ContextTransmaterializable + Send + Sync + 'static,
{
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        match self {
            Ok(value) => {
                let mut buffer = Vec::with_capacity(9);
                buffer.extend_from_slice(&[0u8]);

                let value_bytes = value.immaterialize(ctx).await?;
                let length: u64 = value_bytes.len() as u64;
                buffer.extend_from_slice(&length.to_le_bytes());
                buffer.extend_from_slice(&value_bytes);

                Ok(buffer)
            }
            Err(err) => {
                let mut buffer = Vec::with_capacity(9);
                buffer.extend_from_slice(&[1u8]);
                let err_bytes = err.immaterialize(ctx).await?;
                let length: u64 = err_bytes.len() as u64;
                buffer.extend_from_slice(&length.to_le_bytes());
                buffer.extend_from_slice(&err_bytes);
                Ok(buffer)
            }
        }
    }

    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_min_buffer_size(data, 9)?; // discriminant(1) + length(8)
        let discriminant = data[0];
        let mut offset = 1;

        let length = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
        offset += 8;

        require_min_buffer_size(data, offset + length)?; // Ensure there are bytes for data
        let inner_data = &data[offset..offset + length];
        offset += length;

        require_buffer_size(data, offset)?; // Ensure all data is consumed

        match discriminant {
            0u8 => {
                let value = T::rematerialize(ctx, inner_data).await?;
                Ok(Ok(value))
            }
            1u8 => {
                let err_val = E::rematerialize(ctx, inner_data).await?;
                Ok(Err(err_val))
            }
            _ => Err(anyhow::anyhow!(
                "Invalid discriminant for Result: {}. Expected 0 for Ok or 1 for Err.",
                discriminant
            )),
        }
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl<T> ContextTransmaterializable for Option<T>
where
    T: ContextTransmaterializable + Send + Sync + 'static,
{
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        match self {
            Some(value) => {
                // Capacity: 1 (discriminant) + 8 (length) + value_bytes.len()
                // Initial capacity for discriminant and length.
                let mut buffer = Vec::with_capacity(1 + 8);
                buffer.push(1u8);

                let value_bytes = value.immaterialize(ctx).await?;
                let length: u64 = value_bytes.len() as u64;
                buffer.extend_from_slice(&length.to_le_bytes());
                buffer.extend_from_slice(&value_bytes);

                Ok(buffer)
            }
            None => {
                // Capacity: 1 (discriminant)
                let mut buffer = Vec::with_capacity(1);
                buffer.push(0u8);
                Ok(buffer)
            }
        }
    }

    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        require_min_buffer_size(data, 1)?;
        let discriminant = data[0];
        let mut offset = 1;

        match discriminant {
            0u8 => {
                require_buffer_size(data, offset)?; // Ensure all data is consumed for None
                Ok(None)
            }
            1u8 => {
                require_min_buffer_size(data, offset + 8)?; // Ensure there are bytes for length
                let length = u64::from_le_bytes(data[offset..offset + 8].try_into()?) as usize;
                offset += 8;

                require_min_buffer_size(data, offset + length)?; // Ensure there are bytes for data
                let inner_data = &data[offset..offset + length];
                offset += length;

                require_buffer_size(data, offset)?; // Ensure all data is consumed

                let value = T::rematerialize(ctx, inner_data).await?;
                Ok(Some(value))
            }
            _ => Err(anyhow::anyhow!(
                "Invalid discriminant for Option: {}. Expected 0 for None or 1 for Some.",
                discriminant
            )),
        }
    }
}

// -------------------------------------------------------------------------------------------------------

#[async_trait]
impl ContextTransmaterializable for anyhow::Error {
    async fn immaterialize(
        self,
        ctx: &TransmaterializationContext,
    ) -> TransmaterializationResult<Vec<u8>> {
        let error_string = format!("{}", self);
        error_string.immaterialize(ctx).await
    }

    async fn rematerialize(
        ctx: &TransmaterializationContext,
        data: &[u8],
    ) -> TransmaterializationResult<Self> {
        let error_message = String::rematerialize(ctx, data).await?;
        Ok(anyhow::anyhow!(error_message))
    }
}

// -------------------------------------------------------------------------------------------------------
