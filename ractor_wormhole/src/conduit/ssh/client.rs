use log::{error, info, warn};
use ractor::ActorRef;
use russh::client::{self, Handle, Msg};
use russh::keys::PrivateKeyWithHashAlg;
use russh::ChannelStream;
use std::path::Path;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use ractor_wormhole::{
    conduit::{ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
};

use crate::{conduit, util::ActorRef_Ask};

// ===================================================================================
// Configuration Types
// ===================================================================================

/// Configuration for SSH connection
#[derive(Clone)]
pub struct SshConfig {
    /// SSH server hostname or IP
    pub host: String,
    /// SSH server port (default: 22)
    pub port: u16,
    /// SSH username
    pub username: String,
    /// Authentication method
    pub auth_method: SshAuthMethod,
}

#[derive(Clone)]
pub enum SshAuthMethod {
    /// Password authentication
    Password(String),
    /// Public key authentication with private key path
    PublicKey {
        private_key_path: String,
        passphrase: Option<String>,
    },
    /// Agent-based authentication
    Agent,
}

// ===================================================================================
// SSH Client Handler (required by russh)
// ===================================================================================

pub struct Client;

impl client::Handler for Client {
    type Error = russh::Error;

    fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> impl std::future::Future<Output = Result<bool, Self::Error>> + Send {
        // TODO: Implement proper host key verification
        // For now, accept all keys
        async { Ok(true) }
    }
}

// ===================================================================================
// Low-Level: SSH Connection Primitive
// ===================================================================================

/// Establishes an SSH connection and returns a Handle
/// This is the foundational primitive that other functions build on
pub async fn connect_ssh(config: SshConfig) -> Result<Handle<Client>, anyhow::Error> {
    info!(
        "Establishing SSH connection to {}@{}:{}",
        config.username, config.host, config.port
    );

    let ssh_config = Arc::new(russh::client::Config::default());
    let client = Client;

    let mut handle = russh::client::connect(
        ssh_config,
        (config.host.as_str(), config.port),
        client,
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to connect to SSH server: {}", e))?;

    // Authenticate
    let authenticated = match config.auth_method {
        SshAuthMethod::Password(password) => {
            handle
                .authenticate_password(config.username.clone(), password)
                .await
                .map_err(|e| anyhow::anyhow!("Password authentication failed: {}", e))?
        }
        SshAuthMethod::PublicKey {
            private_key_path,
            passphrase,
        } => {
            let key_pair = russh::keys::load_secret_key(&private_key_path, passphrase.as_deref())
                .map_err(|e| anyhow::anyhow!("Failed to load private key: {}", e))?;

            let key_with_hash = PrivateKeyWithHashAlg::new(
                Arc::new(key_pair),
                handle.best_supported_rsa_hash().await
                    .map_err(|e| anyhow::anyhow!("Failed to get RSA hash algorithm: {}", e))?
                    .flatten(),
            );

            handle
                .authenticate_publickey(config.username.clone(), key_with_hash)
                .await
                .map_err(|e| anyhow::anyhow!("Public key authentication failed: {}", e))?
        }
        SshAuthMethod::Agent => {
            todo!("Agent-based authentication hasn't been fully tested yet.");

            // Connect to SSH agent
            let mut agent = russh::keys::agent::client::AgentClient::connect_env()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to connect to SSH agent: {}. Make sure SSH_AUTH_SOCK is set and ssh-agent is running.", e))?;

            // Request identities from the agent
            let identities = agent
                .request_identities()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to request identities from SSH agent: {}", e))?;

            if identities.is_empty() {
                return Err(anyhow::anyhow!(
                    "SSH agent has no identities. Add keys using 'ssh-add'."
                ));
            }

            info!("SSH agent provided {} identities", identities.len());

            // Get the best supported RSA hash algorithm
            let hash_alg = handle.best_supported_rsa_hash().await
                .map_err(|e| anyhow::anyhow!("Failed to get RSA hash algorithm: {}", e))?
                .flatten();

            // Try each identity until one succeeds
            let mut last_error = None;
            let mut auth_result = None;
            for (i, pubkey) in identities.iter().enumerate() {
                info!("Trying identity {} of {}", i + 1, identities.len());

                match handle
                    .authenticate_publickey_with(config.username.clone(), pubkey.clone(), hash_alg, &mut agent)
                    .await
                {
                    Ok(result) if result.success() => {
                        info!("Successfully authenticated with identity {}", i + 1);
                        auth_result = Some(result);
                        break;
                    }
                    Ok(_) => {
                        warn!("Identity {} rejected by server", i + 1);
                        last_error = Some(anyhow::anyhow!("Identity {} rejected", i + 1));
                    }
                    Err(e) => {
                        warn!("Failed to authenticate with identity {}: {:?}", i + 1, e);
                        last_error = Some(anyhow::anyhow!("Authentication error: {:?}", e));
                    }
                }
            }

            match auth_result {
                Some(result) => result,
                None => return Err(last_error.unwrap_or_else(|| {
                    anyhow::anyhow!("All {} identities were rejected by the server", identities.len())
                })),
            }
        }
    };

    if !authenticated.success() {
        return Err(anyhow::anyhow!("SSH authentication failed"));
    }

    info!("SSH authentication successful");
    Ok(handle)
}

// ===================================================================================
// Mid-Level: Channel Primitives
// ===================================================================================

/// Opens an SSH channel that connects to a Unix socket on the remote host
/// Returns a ChannelStream which implements AsyncRead + AsyncWrite
///
/// Note: This uses the direct-streamlocal@openssh.com extension, which requires
/// OpenSSH on the remote side. If not available, consider using ssh_exec_socat_unix_socket()
pub async fn ssh_unix_socket_channel<P: AsRef<Path>>(
    handle: &mut Handle<Client>,
    socket_path: P,
) -> Result<ChannelStream<Msg>, anyhow::Error> {
    let socket_path = socket_path.as_ref();
    info!("Opening SSH channel to Unix socket: {}", socket_path.display());

    // OpenSSH extension for Unix socket forwarding
    let channel = handle
        .channel_open_direct_streamlocal(socket_path.to_string_lossy().as_ref())
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to open direct-streamlocal channel to {}: {}. \
                 Ensure OpenSSH is used on the remote side.",
                socket_path.display(),
                e
            )
        })?;

    info!("SSH channel to Unix socket opened successfully");
    Ok(channel.into_stream())
}

/// Opens an SSH channel by executing a command that uses socat to bridge to a Unix socket
/// This is a fallback when direct-streamlocal@openssh.com is not available
/// Requires 'socat' to be installed on the remote host
pub async fn ssh_exec_socat_unix_socket<P: AsRef<Path>>(
    handle: &mut Handle<Client>,
    socket_path: P,
) -> Result<ChannelStream<Msg>, anyhow::Error> {
    let socket_path = socket_path.as_ref();
    info!(
        "Opening SSH exec channel with socat to Unix socket: {}",
        socket_path.display()
    );

    let channel = handle
        .channel_open_session()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to open SSH session channel: {}", e))?;

    let command = format!("socat STDIO UNIX-CONNECT:{}", socket_path.display());
    info!("Executing remote command: {}", command);

    channel
        .exec(true, command)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to execute socat command: {}", e))?;

    info!("SSH exec channel opened successfully");
    Ok(channel.into_stream())
}

/// Opens an SSH channel that forwards to a TCP port on the remote network
/// Returns a ChannelStream which implements AsyncRead + AsyncWrite
pub async fn ssh_tcp_channel(
    handle: &mut Handle<Client>,
    remote_host: &str,
    remote_port: u16,
) -> Result<ChannelStream<Msg>, anyhow::Error> {
    info!(
        "Opening SSH channel to TCP {}:{}",
        remote_host, remote_port
    );

    let channel = handle
        .channel_open_direct_tcpip(remote_host, remote_port as u32, "127.0.0.1", 0)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to open direct-tcpip channel to {}:{}: {}",
                remote_host,
                remote_port,
                e
            )
        })?;

    info!("SSH channel to TCP endpoint opened successfully");
    Ok(channel.into_stream())
}

// ===================================================================================
// Adapter: Channel to Conduit Source/Sink
// ===================================================================================

/// Converts a russh Channel into our Conduit sink and source primitives
/// The channel must implement AsyncRead + AsyncWrite
pub fn channel_to_conduit<C>(channel: C) -> (ConduitSource, ConduitSink)
where
    C: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (rx, tx) = tokio::io::split(channel);
    let source = create_source(rx);
    let sink = create_sink(tx);
    (source, sink)
}
// ===================================================================================
// High-Level: Complete Connection to Portal
// ===================================================================================

/// High-level function: Connect to a Unix socket on a remote server via SSH
/// and register it as a Portal with the Nexus
pub async fn connect_via_ssh_to_unix_socket<P: AsRef<Path>>(
    nexus: ActorRef<NexusActorMessage>,
    ssh_config: SshConfig,
    remote_socket_path: P,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error> {
    let remote_socket_path = remote_socket_path.as_ref();
    
    // Establish SSH connection
    let mut handle = connect_ssh(ssh_config.clone()).await?;
    
    // Try direct-streamlocal first, fall back to socat if it fails
    let channel = match ssh_unix_socket_channel(&mut handle, remote_socket_path).await {
        Ok(channel) => channel,
        Err(e) => {
            info!(
                "direct-streamlocal failed ({}), falling back to socat method",
                e
            );
            ssh_exec_socat_unix_socket(&mut handle, remote_socket_path).await?
        }
    };
    
    // Convert channel to conduit primitives
    let (source, sink) = channel_to_conduit(channel);
    
    // Register with nexus
    let portal_identifier = format!(
        "ssh://{}@{}:{}/unix:{}",
        ssh_config.username,
        ssh_config.host,
        ssh_config.port,
        remote_socket_path.display()
    );
    
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), sink, rpc),
            None,
        )
        .await?;

    info!("Portal registered: {}", portal_identifier);

    // Start receive loop
    let portal_actor_copy = portal.clone();
    ractor::concurrency::spawn(async move {
        conduit::receive_loop(source, portal_identifier, portal_actor_copy).await
    });

    Ok(portal)
}

/// High-level function: Connect to a TCP port via SSH tunnel
/// and register it as a Portal with the Nexus
pub async fn connect_via_ssh_to_tcp(
    nexus: ActorRef<NexusActorMessage>,
    ssh_config: SshConfig,
    remote_host: String,
    remote_port: u16,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error> {
    // Establish SSH connection
    let mut handle = connect_ssh(ssh_config.clone()).await?;
    
    // Open TCP channel
    let channel = ssh_tcp_channel(&mut handle, &remote_host, remote_port).await?;
    
    // Convert channel to conduit primitives
    let (source, sink) = channel_to_conduit(channel);
    
    // Register with nexus
    let portal_identifier = format!(
        "ssh://{}@{}:{}/tcp/{}:{}",
        ssh_config.username,
        ssh_config.host,
        ssh_config.port,
        remote_host,
        remote_port
    );
    
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), sink, rpc),
            None,
        )
        .await?;

    info!("Portal registered: {}", portal_identifier);

    // Start receive loop
    let portal_actor_copy = portal.clone();
    ractor::concurrency::spawn(async move {
        conduit::receive_loop(source, portal_identifier, portal_actor_copy).await
    });

    Ok(portal)
}

// ===================================================================================
// Low-Level: Binary Framing for Conduit Protocol
// ===================================================================================

fn create_sink<W>(writer: W) -> ConduitSink
where
    W: AsyncWriteExt + Unpin + Send + 'static,
{
    let sink = futures::sink::unfold(writer, |mut writer, element: ConduitMessage| async move {
        let result = match element {
            ConduitMessage::Text(text) => {
                // Format: [type:1byte][length:4bytes][data]
                let data = text.as_bytes();
                let len = data.len() as u32;

                if let Err(e) = writer.write_u8(0).await {
                    error!("Failed to write message type: {}", e);
                    return Err(ConduitError::from(e));
                }
                if let Err(e) = writer.write_u32(len).await {
                    error!("Failed to write message length: {}", e);
                    return Err(ConduitError::from(e));
                }
                if let Err(e) = writer.write_all(data).await {
                    error!("Failed to write text message: {}", e);
                    return Err(ConduitError::from(e));
                }
                writer.flush().await.map_err(ConduitError::from)
            }
            ConduitMessage::Binary(data) => {
                // Format: [type:1byte][length:4bytes][data]
                let len = data.len() as u32;

                if let Err(e) = writer.write_u8(1).await {
                    error!("Failed to write message type: {}", e);
                    return Err(ConduitError::from(e));
                }
                if let Err(e) = writer.write_u32(len).await {
                    error!("Failed to write message length: {}", e);
                    return Err(ConduitError::from(e));
                }
                if let Err(e) = writer.write_all(&data).await {
                    error!("Failed to write binary message: {}", e);
                    return Err(ConduitError::from(e));
                }
                writer.flush().await.map_err(ConduitError::from)
            }
            ConduitMessage::Close(reason) => {
                info!("Closing SSH connection: {:?}", reason);
                // Optionally write a close frame
                writer.write_u8(2).await.ok();
                writer.flush().await.ok();
                return Err(ConduitError::msg("Connection closed"));
            }
        };

        result.map(|_| writer)
    });

    Box::pin(sink)
}

fn create_source<R>(reader: R) -> ConduitSource
where
    R: AsyncReadExt + Unpin + Send + 'static,
{
    let stream = futures::stream::unfold(reader, |mut reader| async move {
        // Read message type (1 byte)
        let msg_type = match reader.read_u8().await {
            Ok(t) => t,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    info!("SSH connection closed");
                    return None;
                }
                error!("Failed to read message type: {}", e);
                return Some((Err(ConduitError::from(e)), reader));
            }
        };

        // Read message length (4 bytes)
        let len = match reader.read_u32().await {
            Ok(l) => l as usize,
            Err(e) => {
                error!("Failed to read message length: {}", e);
                return Some((Err(ConduitError::from(e)), reader));
            }
        };

        // Read message data
        let mut buffer = vec![0u8; len];
        if let Err(e) = reader.read_exact(&mut buffer).await {
            error!("Failed to read message data: {}", e);
            return Some((Err(ConduitError::from(e)), reader));
        }

        let message = match msg_type {
            0 => {
                // Text message
                match String::from_utf8(buffer) {
                    Ok(text) => ConduitMessage::Text(text),
                    Err(e) => {
                        error!("Failed to decode text message: {}", e);
                        return Some((Err(ConduitError::from(e)), reader));
                    }
                }
            }
            1 => {
                // Binary message
                ConduitMessage::Binary(buffer)
            }
            2 => {
                // Close message
                let reason = String::from_utf8(buffer).ok();
                info!("Received close message: {:?}", reason);
                return Some((Ok(ConduitMessage::Close(reason)), reader));
            }
            _ => {
                error!("Unknown message type: {}", msg_type);
                return Some((
                    Err(ConduitError::msg(format!("Unknown message type: {}", msg_type))),
                    reader,
                ));
            }
        };

        Some((Ok(message), reader))
    });

    Box::pin(stream)
}
