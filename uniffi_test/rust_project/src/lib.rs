//! UniFFI bindings for ractor-wormhole.
//!
//! This crate provides FFI-safe bindings for embedding ractor-wormhole into
//! Kotlin/Android apps. The key insight is that while ractor uses generic types
//! like `ActorRef<T>` and `RpcReplyPort<T>`, the set of concrete instantiations
//! is known at compile time. We create typed FFI wrappers for each instantiation.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Kotlin Side                                  │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  Implements callbacks:                                              │
//! │  - ChatClientActorCallback (receives server notifications)          │
//! │  - ChatServerActorCallback (handles client messages)                │
//! │  - HubActorCallback (handles connection requests)                   │
//! │                                                                      │
//! │  Uses typed wrappers:                                               │
//! │  - FfiChatClientActorRef.sendMessage(sender, content)              │
//! │  - FfiChatServerActorRef.postMessage(content)                      │
//! │  - FfiConnectReplyPort.reply(alias, serverActor)                   │
//! └─────────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                         Rust FFI Layer                               │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  Concrete wrappers for generic types:                               │
//! │  - FfiChatClientActorRef wraps ActorRef<ChatClientMessage>         │
//! │  - FfiConnectReplyPort wraps RpcReplyPort<(UserAlias, ActorRef<>)> │
//! │                                                                      │
//! │  These wrappers have typed methods:                                 │
//! │  - send_user_connected(alias: String)                               │
//! │  - reply(alias: String, server: &FfiChatServerActorLocal)          │
//! └─────────────────────────────────────────────────────────────────────┘
//!                               │
//!                               ▼
//! ┌─────────────────────────────────────────────────────────────────────┐
//! │                    Ractor / Ractor-Wormhole                          │
//! ├─────────────────────────────────────────────────────────────────────┤
//! │  Generic actor types:                                               │
//! │  - ActorRef<T>                                                      │
//! │  - RpcReplyPort<T>                                                  │
//! │                                                                      │
//! │  Transmaterialization handles serialization across network          │
//! └─────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example: Client connecting to a server
//!
//! ```kotlin
//! // Create runtime
//! val runtime = WormholeRuntime()
//!
//! // Create local actor to receive notifications
//! val clientActor = runtime.createChatClientActor(object : ChatClientActorCallback {
//!     override fun onMessageReceived(sender: String, content: String) {
//!         println("[$sender]: $content")
//!     }
//!     // ... other callbacks
//! })
//!
//! // Connect to server
//! val connection = runtime.createConnection("client", transport)
//! connection.waitForHandshake(5000u)
//!
//! // Query for the hub and connect
//! val hub = connection.getRemoteHubActor("hub")
//! val result = hub.connect(clientActor)  // Sends our ActorRef to server!
//!
//! // Use the returned server actor to post messages
//! result.serverActor.postMessage("Hello, world!")
//! ```

// UniFFI scaffolding
uniffi::setup_scaffolding!();

// Module declarations
pub mod callbacks;
pub mod error;
pub mod ffi_actors;
pub mod ffi_core;
pub mod ffi_messages;
pub mod ffi_rpc;
pub mod messages;

// Re-exports for UniFFI
pub use callbacks::*;
pub use error::*;
pub use ffi_actors::*;
pub use ffi_core::*;
pub use ffi_messages::*;
pub use ffi_rpc::*;

// Note: messages module is not re-exported as it contains internal Rust types
// that are wrapped by the ffi_* modules for FFI consumption.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_creation() {
        let runtime = WormholeRuntime::new().unwrap();
        runtime.shutdown();
    }
}
