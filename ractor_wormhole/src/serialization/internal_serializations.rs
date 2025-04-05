use crate::gateway::{CrossGatewayMessage, RemoteActorId};

use super::{SerializationResult, SerializedRpcReplyPort, util::require_buffer_size};

// -------------------------------------------------------------------------------------------------------

pub trait SimpleByteSerializable {
    fn serialize(&self) -> SerializationResult<Vec<u8>>;
    fn deserialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized;
}

impl SimpleByteSerializable for SerializedRpcReplyPort {
    fn serialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn deserialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        let (rpc, consumed): (SerializedRpcReplyPort, _) =
            bincode::decode_from_slice(data, bincode::config::standard())?;
        require_buffer_size(data, consumed)?;
        Ok(rpc)
    }
}

// -------------------------------------------------------------------------------------------------------

impl SimpleByteSerializable for RemoteActorId {
    fn serialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn deserialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        let (remote_actor_id, consumed): (RemoteActorId, _) =
            bincode::decode_from_slice(data, bincode::config::standard())?;
        require_buffer_size(data, consumed)?;
        Ok(remote_actor_id)
    }
}

// -------------------------------------------------------------------------------------------------------

impl SimpleByteSerializable for CrossGatewayMessage {
    fn serialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn deserialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        let (msg, consumed): (CrossGatewayMessage, _) =
            bincode::decode_from_slice(data, bincode::config::standard())?;
        require_buffer_size(data, consumed)?;
        Ok(msg)
    }
}
