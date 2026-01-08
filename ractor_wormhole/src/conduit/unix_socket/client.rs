use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::ActorRef;
use std::path::Path;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
};

use crate::{conduit, util::ActorRef_Ask};

/// Connects to a Unix socket server at the specified path.
pub async fn connect_to_server<P: AsRef<Path>>(
    nexus: ActorRef<NexusActorMessage>,
    socket_path: P,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error> {
    let socket_path = socket_path.as_ref();
    info!(
        "Connecting to Unix socket server at: {}",
        socket_path.display()
    );

    // Connect to the Unix socket server
    let stream = match UnixStream::connect(socket_path).await {
        Ok(stream) => {
            info!(
                "Unix socket connection established to: {}",
                socket_path.display()
            );
            stream
        }
        Err(e) => {
            error!(
                "Failed to connect to Unix socket server at {}: {e}",
                socket_path.display()
            );
            return Err(e.into());
        }
    };

    // Split the Unix socket stream
    let (reader, writer) = stream.into_split();

    // Create the source and sink
    let rx = create_source(reader);
    let tx = create_sink(writer);

    // Register the portal with the nexus actor
    let portal_identifier = format!("unix://{}", socket_path.display());
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), tx, rpc),
            None,
        )
        .await?;

    info!("Portal actor started for: {}", socket_path.display());

    let portal_actor_copy = portal.clone();
    ractor::concurrency::spawn(async move {
        conduit::receive_loop(rx, portal_identifier, portal_actor_copy).await
    });

    Ok(portal)
}

// ---------------------------------------------------------------------------------

fn create_sink(mut writer: tokio::net::unix::OwnedWriteHalf) -> ConduitSink {
    let sink = futures::sink::unfold(writer, |mut writer, element: ConduitMessage| async move {
        let result = match element {
            ConduitMessage::Text(text) => {
                // Format: [type:1byte][length:4bytes][data]
                let data = text.as_bytes();
                let len = data.len() as u32;

                if writer.write_u8(0).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message type"));
                }
                if writer.write_u32(len).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message length"));
                }
                if writer.write_all(data).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message data"));
                }
                writer.flush().await.map_err(ConduitError::from)
            }
            ConduitMessage::Binary(bin) => {
                // Format: [type:1byte][length:4bytes][data]
                let len = bin.len() as u32;

                if writer.write_u8(1).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message type"));
                }
                if writer.write_u32(len).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message length"));
                }
                if writer.write_all(&bin).await.is_err() {
                    return Err(ConduitError::msg("Failed to write message data"));
                }
                writer.flush().await.map_err(ConduitError::from)
            }
            ConduitMessage::Close(_) => {
                // Format: [type:1byte]
                if writer.write_u8(2).await.is_err() {
                    return Err(ConduitError::msg("Failed to write close message"));
                }
                writer.flush().await.map_err(ConduitError::from)
            }
        };

        match result {
            Ok(()) => Ok(writer),
            Err(e) => Err(e),
        }
    });

    Box::pin(sink)
}

fn create_source(mut reader: tokio::net::unix::OwnedReadHalf) -> ConduitSource {
    let stream = futures::stream::unfold(reader, |mut reader| async move {
        // Read message type (1 byte)
        let msg_type = match reader.read_u8().await {
            Ok(t) => t,
            Err(e) => return Some((Err(ConduitError::from(e)), reader)),
        };

        match msg_type {
            0 => {
                // Text message
                let len = match reader.read_u32().await {
                    Ok(l) => l as usize,
                    Err(e) => return Some((Err(ConduitError::from(e)), reader)),
                };

                let mut buf = vec![0u8; len];
                if let Err(e) = reader.read_exact(&mut buf).await {
                    return Some((Err(ConduitError::from(e)), reader));
                }

                match String::from_utf8(buf) {
                    Ok(text) => Some((Ok(ConduitMessage::Text(text)), reader)),
                    Err(e) => Some((Err(ConduitError::from(e)), reader)),
                }
            }
            1 => {
                // Binary message
                let len = match reader.read_u32().await {
                    Ok(l) => l as usize,
                    Err(e) => return Some((Err(ConduitError::from(e)), reader)),
                };

                let mut buf = vec![0u8; len];
                if let Err(e) = reader.read_exact(&mut buf).await {
                    return Some((Err(ConduitError::from(e)), reader));
                }

                Some((Ok(ConduitMessage::Binary(buf)), reader))
            }
            2 => {
                // Close message
                Some((Ok(ConduitMessage::Close(None)), reader))
            }
            _ => Some((
                Err(ConduitError::msg(format!(
                    "Unknown message type: {}",
                    msg_type
                ))),
                reader,
            )),
        }
    });

    Box::pin(stream)
}
