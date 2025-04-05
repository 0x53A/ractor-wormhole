use crate::nexus::RemoteActorId;
use crate::portal::CrossPortalMessage;

use super::{SerializationResult, SerializedRpcReplyPort, util::require_buffer_size};

// -------------------------------------------------------------------------------------------------------

pub trait SimpleByteTransmaterializable {
    fn immaterialize(&self) -> SerializationResult<Vec<u8>>;
    fn rematerialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized;
}

impl SimpleByteTransmaterializable for SerializedRpcReplyPort {
    fn immaterialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn rematerialize(data: &[u8]) -> SerializationResult<Self>
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

impl SimpleByteTransmaterializable for RemoteActorId {
    fn immaterialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn rematerialize(data: &[u8]) -> SerializationResult<Self>
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

impl SimpleByteTransmaterializable for CrossPortalMessage {
    fn immaterialize(&self) -> SerializationResult<Vec<u8>> {
        Ok(bincode::encode_to_vec(self, bincode::config::standard())?)
    }

    fn rematerialize(data: &[u8]) -> SerializationResult<Self>
    where
        Self: Sized,
    {
        let (msg, consumed): (CrossPortalMessage, _) =
            bincode::decode_from_slice(data, bincode::config::standard())?;
        require_buffer_size(data, consumed)?;
        Ok(msg)
    }
}
