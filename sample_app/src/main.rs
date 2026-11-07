mod common;

pub mod client;
#[cfg(any(feature = "websocket", feature = "unix_socket"))]
pub mod server;

use clap::{ArgGroup, Parser, Subcommand};
#[cfg(feature = "websocket")]
use std::net::SocketAddr;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start in client mode
    #[command(group(ArgGroup::new("transport").required(true).multiple(false)))]
    Client {
        #[cfg(feature = "unix_socket")]
        #[arg(long, group = "transport")]
        socket: Option<String>,

        #[cfg(feature = "websocket")]
        /// Server URL to connect to
        #[arg(long, group = "transport")]
        url: Option<String>,

        #[cfg(feature = "ssh")]
        /// SSH connection string (format: user@host:port/path/to/socket)
        #[arg(long, group = "transport")]
        ssh: Option<String>,

        #[cfg(feature = "ssh")]
        /// SSH password (if not using key-based auth)
        #[arg(long, requires = "ssh")]
        #[cfg_attr(feature = "websocket", arg(conflicts_with = "url"))]
        #[cfg_attr(feature = "unix_socket", arg(conflicts_with = "socket"))]
        ssh_password: Option<String>,

        #[cfg(feature = "ssh")]
        /// Path to SSH private key
        #[arg(long, requires = "ssh")]
        #[cfg_attr(feature = "websocket", arg(conflicts_with = "url"))]
        #[cfg_attr(feature = "unix_socket", arg(conflicts_with = "socket"))]
        ssh_key: Option<String>,

        #[cfg(feature = "ssh")]
        /// SSH private key passphrase
        #[arg(long, requires = "ssh")]
        #[cfg_attr(feature = "websocket", arg(conflicts_with = "url"))]
        #[cfg_attr(feature = "unix_socket", arg(conflicts_with = "socket"))]
        ssh_key_passphrase: Option<String>,
    },
    #[cfg(any(feature = "websocket", feature = "unix_socket"))]
    /// Start in server mode
    #[command(group(ArgGroup::new("transport").required(true).multiple(false)))]
    Server {
        #[cfg(feature = "unix_socket")]
        #[arg(long, group = "transport")]
        socket: Option<String>,

        #[cfg(feature = "websocket")]
        /// Address to bind to (e.g. 127.0.0.1:8080)
        #[arg(long, group = "transport")]
        bind: Option<SocketAddr>,
    },
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {e:#}"); // Pretty format with all causes
        std::process::exit(1);
    }
}

async fn run() -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let cli = Cli::parse();

    match cli.command {
        Commands::Client {
            #[cfg(feature = "unix_socket")]
            socket,
            #[cfg(feature = "websocket")]
            url,
            #[cfg(feature = "ssh")]
            ssh,
            #[cfg(feature = "ssh")]
            ssh_password,
            #[cfg(feature = "ssh")]
            ssh_key,
            #[cfg(feature = "ssh")]
            ssh_key_passphrase,
        } => {
            #[cfg(feature = "unix_socket")]
            if let Some(socket) = socket {
                return client::run(client::Transport::UnixSocket(socket)).await;
            }
            #[cfg(feature = "websocket")]
            if let Some(url) = url {
                return client::run(client::Transport::WebSocket(url)).await;
            }
            #[cfg(feature = "ssh")]
            if let Some(ssh) = ssh {
                return client::run_ssh(ssh, ssh_password, ssh_key, ssh_key_passphrase).await;
            }
            anyhow::bail!("Select a client transport");
        }
        #[cfg(any(feature = "websocket", feature = "unix_socket"))]
        Commands::Server {
            #[cfg(feature = "unix_socket")]
            socket,
            #[cfg(feature = "websocket")]
            bind,
        } => {
            #[cfg(feature = "unix_socket")]
            if let Some(socket) = socket {
                return server::run(server::Transport::UnixSocket(socket)).await;
            }
            #[cfg(feature = "websocket")]
            if let Some(bind) = bind {
                return server::run(server::Transport::WebSocket(bind)).await;
            }
            anyhow::bail!("Select a server transport (enable websocket or unix_socket)");
        }
    }
}

#[cfg(all(test, feature = "ssh", feature = "websocket"))]
mod tests {
    use super::*;

    #[test]
    fn transports_are_selected_exclusively() {
        assert!(Cli::try_parse_from(["sample", "client", "--ssh", "user@host/tmp/socket"]).is_ok());
        assert!(Cli::try_parse_from(["sample", "client", "--url", "ws://localhost:1234"]).is_ok());
        assert!(Cli::try_parse_from(["sample", "client"]).is_err());
        assert!(
            Cli::try_parse_from([
                "sample",
                "client",
                "--ssh",
                "user@host/tmp/socket",
                "--url",
                "ws://localhost:1234"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "sample",
                "client",
                "--url",
                "ws://localhost:1234",
                "--ssh-key",
                "key"
            ])
            .is_err()
        );
        assert!(Cli::try_parse_from(["sample", "server", "--bind", "127.0.0.1:1234"]).is_ok());
    }
}
