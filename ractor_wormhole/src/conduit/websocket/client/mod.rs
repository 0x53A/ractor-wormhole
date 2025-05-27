#[cfg(target_arch = "wasm32")]
pub mod ewebsock;
#[cfg(not(target_arch = "wasm32"))]
pub mod tokio_tungstenite;
