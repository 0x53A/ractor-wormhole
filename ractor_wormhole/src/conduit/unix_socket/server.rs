use futures::{SinkExt, StreamExt};
use log::{error, info};
use ractor::ActorRef;
use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
};

use crate::conduit;

/// Starts a Unix socket server on the specified path.
pub async fn start_server<P: AsRef<Path>>(
    nexus: ActorRef<NexusActorMessage>,
    socket_path: P,
) -> Result<(), anyhow::Error> {
    let socket_path = socket_path.as_ref().to_path_buf();

    // Remove the socket file if it already exists
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Create a Unix domain socket listener
    let listener = UnixListener::bind(&socket_path)?;
    info!("Unix socket server listening on: {}", socket_path.display());

    // Accept connections
    let nexus_copy = nexus.clone();
    let socket_path_copy = socket_path.clone();
    tokio::spawn(async move {
        let mut connection_counter = 0u64;
        while let Ok((stream, _addr)) = listener.accept().await {
            connection_counter += 1;
            info!("New Unix socket connection #{connection_counter}");
            let _ = handle_connection(
                stream,
                connection_counter,
                nexus_copy.clone(),
                &socket_path_copy,
            )
            .await;
        }
    });

    Ok(())
}

async fn handle_connection(
    stream: UnixStream,
    connection_id: u64,
    nexus: ActorRef<NexusActorMessage>,
    socket_path: &Path,
) {
    info!("Unix socket connection #{connection_id} established");

    let (reader, writer) = stream.into_split();

    let rx = create_source(reader);
    let tx = create_sink(writer);

    let portal_identifier = format!("unix://{}#{}", socket_path.display(), connection_id);
    if let Err(err) = conduit::from_sink_source(nexus, portal_identifier, tx, rx).await {
        error!("Error creating portal: {err}");
    }
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
