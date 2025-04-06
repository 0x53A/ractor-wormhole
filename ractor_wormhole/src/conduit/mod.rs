pub mod websocket;

use futures::{Sink, Stream, StreamExt};
use ractor::ActorRef;
use std::pin::Pin;

use log::{error, info};

use crate::portal::PortalActorMessage;

// -------------------------------------------------------------------------------------------------------

pub enum ConduitMessage {
    Text(String),
    Binary(Vec<u8>),
    Close(Option<String>),
}

pub type ConduitError = anyhow::Error;

pub type ConduitSink = Pin<Box<dyn Sink<ConduitMessage, Error = ConduitError> + Send>>;
pub type ConduitSource = Pin<Box<dyn Stream<Item = Result<ConduitMessage, ConduitError>> + Send>>;

// -------------------------------------------------------------------------------------------------------

pub async fn receive_loop(
    mut receiver: ConduitSource,
    identifier: String,
    actor_ref: ActorRef<PortalActorMessage>,
) {
    // Process incoming messages
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(msg) => match msg {
                ConduitMessage::Text(text) => {
                    if let Err(err) = actor_ref.cast(PortalActorMessage::Text(text.to_string())) {
                        error!("Error sending text message to actor: {}", err);
                        break;
                    }
                }
                ConduitMessage::Binary(data) => {
                    if let Err(err) = actor_ref.cast(PortalActorMessage::Binary(data.to_vec())) {
                        error!("Error sending binary message to actor: {}", err);
                        break;
                    }
                }
                ConduitMessage::Close(close_frame) => {
                    info!(
                        "Portal with {} closed because of reason: {:?}",
                        identifier, close_frame
                    );
                    break;
                }
            },
            Err(e) => {
                error!("Error receiving message from {}: {}", e, identifier);
                break;
            }
        }
    }

    info!("Portal with {} closed", identifier);
    let _ = actor_ref.cast(PortalActorMessage::Close);
}
