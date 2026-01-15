use ractor::ActorRef;
use ractor_wormhole::{
    nexus::start_nexus,
    portal::{Portal, PortalActorMessage},
    util::ActorRef_Ask,
};
use std::time::Duration;
use tokio::time;

use crate::common::{PingPongMsg, start_pingpong_actor};

#[cfg(any(feature = "unix_socket", feature = "websocket"))]
pub async fn run(
    #[cfg(feature = "unix_socket")] socket_path: String,
    #[cfg(feature = "websocket")] server_url: String,
) -> Result<(), anyhow::Error> {
    // Start the nexus actor
    let nexus = start_nexus(None, None).await.unwrap();

    // connect to the server
    #[cfg(feature = "unix_socket")]
    let portal =
        ractor_wormhole::conduit::unix_socket::client::connect_to_server(nexus, socket_path)
            .await?;
    #[cfg(feature = "websocket")]
    let portal = ractor_wormhole::conduit::websocket::client::tokio_tungstenite::connect_to_server(
        nexus, server_url,
    )
    .await?;

    // wait for the portal to be ready (handshake)
    portal
        .ask(
            PortalActorMessage::WaitForHandshake,
            Some(Duration::from_secs(5)),
        )
        .await?;

    // the server has published a named actor
    let remote_pingpong_actor_id = portal
        .ask(
            |rpc| PortalActorMessage::QueryNamedRemoteActor("pingpong".to_string(), rpc),
            None,
        )
        .await??;

    let remote_pingpong: ActorRef<PingPongMsg> = portal
        .instantiate_proxy_for_remote_actor(remote_pingpong_actor_id)
        .await?;

    let local_pingpong = start_pingpong_actor().await?;

    remote_pingpong.send_message(PingPongMsg::Ping(local_pingpong.clone()))?;

    println!("Sent ping to remote pingpong actor");

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}

#[cfg(feature = "ssh")]
pub async fn run_ssh(
    ssh_connection_string: String,
    ssh_password: Option<String>,
    ssh_key: Option<String>,
    ssh_key_passphrase: Option<String>,
) -> Result<(), anyhow::Error> {
    use ractor_wormhole::conduit::ssh::client::{SshAuthMethod, SshConfig, connect_via_ssh_to_unix_socket};

    // Parse SSH connection string: user@host:port/path/to/socket
    // Example: "user@example.com:22/tmp/ractor_wormhole_sample_app"
    let (connection, socket_path) = ssh_connection_string
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid SSH connection string format. Expected: user@host:port/path/to/socket"))?;

    let (user_host, port_str) = if let Some((uh, p)) = connection.rsplit_once(':') {
        (uh, p)
    } else {
        (connection, "22")
    };

    let (username, host) = user_host
        .split_once('@')
        .ok_or_else(|| anyhow::anyhow!("Invalid SSH connection string format. Expected: user@host"))?;

    let port: u16 = port_str.parse()
        .map_err(|_| anyhow::anyhow!("Invalid port number: {}", port_str))?;

    // Determine authentication method
    let auth_method = if let Some(key_path) = ssh_key {
        SshAuthMethod::PublicKey {
            private_key_path: key_path,
            passphrase: ssh_key_passphrase,
        }
    } else if let Some(password) = ssh_password {
        SshAuthMethod::Password(password)
    } else {
        // Note: this tries the first existing key, but would not continue trying the other keys if one exists but does not grant access.
        // Default: try common SSH key locations (like standard ssh command)
        // Try ~/.ssh/id_ed25519, ~/.ssh/id_rsa, etc.
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let default_keys = [
            format!("{}/.ssh/id_ed25519", home),
            format!("{}/.ssh/id_rsa", home),
            format!("{}/.ssh/id_ecdsa", home),
            format!("{}/.ssh/id_dsa", home),
        ];
        
        let found_key = default_keys.iter()
            .find(|path| std::path::Path::new(path).exists())
            .ok_or_else(|| anyhow::anyhow!(
                "No SSH key found in default locations. Use --ssh-key to specify a key, --ssh-password for password auth, or load keys into ssh-agent and use --ssh-agent"
            ))?;
        
        SshAuthMethod::PublicKey {
            private_key_path: found_key.clone(),
            passphrase: None,
        }
    };

    let ssh_config = SshConfig {
        host: host.to_string(),
        port,
        username: username.to_string(),
        auth_method,
    };

    println!(
        "Connecting via SSH to {}@{}:{} and tunneling to socket: {}",
        username, host, port, socket_path
    );

    // Start the nexus actor
    let nexus = start_nexus(None, None).await.unwrap();

    // Connect via SSH tunnel
    let portal = connect_via_ssh_to_unix_socket(nexus, ssh_config, socket_path).await?;

    // wait for the portal to be ready (handshake)
    portal
        .ask(
            PortalActorMessage::WaitForHandshake,
            Some(Duration::from_secs(5)),
        )
        .await?;

    // the server has published a named actor
    let remote_pingpong_actor_id = portal
        .ask(
            |rpc| PortalActorMessage::QueryNamedRemoteActor("pingpong".to_string(), rpc),
            None,
        )
        .await??;

    let remote_pingpong: ActorRef<PingPongMsg> = portal
        .instantiate_proxy_for_remote_actor(remote_pingpong_actor_id)
        .await?;

    let local_pingpong = start_pingpong_actor().await?;

    remote_pingpong.send_message(PingPongMsg::Ping(local_pingpong.clone()))?;

    println!("Sent ping to remote pingpong actor via SSH tunnel");

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}
