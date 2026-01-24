//! Typed wrappers for RpcReplyPort instantiations.
//!
//! Each specific `RpcReplyPort<T>` used in the protocol gets a concrete FFI wrapper
//! with a typed `reply()` method.

use std::sync::{Arc, Mutex};

use crate::error::WormholeError;
use crate::ffi_actors::FfiChatServerActorLocal;
use crate::messages::{AckReplyPort, ConnectReplyPort, UserAlias, UserListReplyPort};

// =============================================================================
// FfiConnectReplyPort
// =============================================================================

/// FFI wrapper for `RpcReplyPort<(UserAlias, ActorRef<ChatServerMessage>)>`.
///
/// This is used when a client connects to the hub. The hub calls `reply()`
/// to send back the assigned user alias and a reference to the chat server actor.
#[derive(uniffi::Object)]
pub struct FfiConnectReplyPort {
    inner: Mutex<Option<ConnectReplyPort>>,
}

impl FfiConnectReplyPort {
    pub fn new(port: ConnectReplyPort) -> Self {
        Self {
            inner: Mutex::new(Some(port)),
        }
    }
}

#[uniffi::export]
impl FfiConnectReplyPort {
    /// Reply to the connect request.
    ///
    /// - `user_alias`: The alias assigned to the connecting user
    /// - `server_actor`: The chat server actor the client should talk to
    pub fn reply(
        &self,
        user_alias: String,
        server_actor: &Arc<FfiChatServerActorLocal>,
    ) -> Result<(), WormholeError> {
        let port = self
            .inner
            .lock()
            .unwrap()
            .take()
            .ok_or(WormholeError::ReplyPortConsumed)?;

        let server_ref = server_actor
            .get_actor_ref()
            .ok_or(WormholeError::ActorStopped)?;

        port.send((UserAlias(user_alias), server_ref))
            .map_err(|_| WormholeError::SendFailed {
                reason: "Failed to send connect reply".into(),
            })
    }

    /// Check if this reply port has already been used.
    pub fn is_consumed(&self) -> bool {
        self.inner.lock().unwrap().is_none()
    }
}

// =============================================================================
// FfiAckReplyPort
// =============================================================================

/// FFI wrapper for `RpcReplyPort<()>` - a simple acknowledgment.
///
/// Used when you just need to acknowledge receipt of a message.
#[derive(uniffi::Object)]
pub struct FfiAckReplyPort {
    inner: Mutex<Option<AckReplyPort>>,
}

impl FfiAckReplyPort {
    pub fn new(port: AckReplyPort) -> Self {
        Self {
            inner: Mutex::new(Some(port)),
        }
    }
}

#[uniffi::export]
impl FfiAckReplyPort {
    /// Send acknowledgment.
    pub fn ack(&self) -> Result<(), WormholeError> {
        let port = self
            .inner
            .lock()
            .unwrap()
            .take()
            .ok_or(WormholeError::ReplyPortConsumed)?;

        port.send(()).map_err(|_| WormholeError::SendFailed {
            reason: "Failed to send ack".into(),
        })
    }

    /// Check if this reply port has already been used.
    pub fn is_consumed(&self) -> bool {
        self.inner.lock().unwrap().is_none()
    }
}

// =============================================================================
// FfiUserListReplyPort
// =============================================================================

/// FFI wrapper for `RpcReplyPort<Vec<UserAlias>>`.
///
/// Used to reply to user list queries.
#[derive(uniffi::Object)]
pub struct FfiUserListReplyPort {
    inner: Mutex<Option<UserListReplyPort>>,
}

impl FfiUserListReplyPort {
    pub fn new(port: UserListReplyPort) -> Self {
        Self {
            inner: Mutex::new(Some(port)),
        }
    }
}

#[uniffi::export]
impl FfiUserListReplyPort {
    /// Reply with the list of user aliases.
    pub fn reply(&self, users: Vec<String>) -> Result<(), WormholeError> {
        let port = self
            .inner
            .lock()
            .unwrap()
            .take()
            .ok_or(WormholeError::ReplyPortConsumed)?;

        let user_aliases: Vec<UserAlias> = users.into_iter().map(UserAlias).collect();

        port.send(user_aliases)
            .map_err(|_| WormholeError::SendFailed {
                reason: "Failed to send user list".into(),
            })
    }

    /// Check if this reply port has already been used.
    pub fn is_consumed(&self) -> bool {
        self.inner.lock().unwrap().is_none()
    }
}
