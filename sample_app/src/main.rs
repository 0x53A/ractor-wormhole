#![feature(fn_traits)]
#![feature(try_find)]

mod common;

pub mod client;
pub mod server;

use clap::{Parser, Subcommand};
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
    Client {
        #[cfg(feature = "unix_socket")]
        #[arg(long)]
        socket: String,

        #[cfg(feature = "websocket")]
        /// Server URL to connect to
        #[arg(long)]
        url: String,

        #[cfg(feature = "ssh")]
        /// SSH connection string (format: user@host:port/path/to/socket)
        #[arg(long)]
        ssh: Option<String>,

        #[cfg(feature = "ssh")]
        /// SSH password (if not using key-based auth)
        #[arg(long)]
        ssh_password: Option<String>,

        #[cfg(feature = "ssh")]
        /// Path to SSH private key
        #[arg(long)]
        ssh_key: Option<String>,

        #[cfg(feature = "ssh")]
        /// SSH private key passphrase
        #[arg(long)]
        ssh_key_passphrase: Option<String>,
    },
    /// Start in server mode
    Server {
        #[cfg(feature = "unix_socket")]
        #[arg(long)]
        socket: String,

        #[cfg(feature = "websocket")]
        /// Address to bind to (e.g. 127.0.0.1:8080)
        #[arg(long)]
        bind: SocketAddr,
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

    #[cfg(feature = "unix_socket")]
    match cli.command {
        Commands::Client { socket } => {
            println!("Starting client, connecting to: {socket}");
            client::run(socket).await?;
        }
        Commands::Server { socket } => {
            println!("Starting server, binding to: {socket}");
            server::run(socket).await?;
        }
    }

    #[cfg(feature = "websocket")]
    match cli.command {
        Commands::Client { url } => {
            println!("Starting client, connecting to: {url}");
            client::run(url).await?;
        }
        Commands::Server { bind } => {
            println!("Starting server, binding to: {bind}");
            server::run(bind).await?;
        }
    }

    #[cfg(feature = "ssh")]
    match cli.command {
        Commands::Client {
            ssh,
            ssh_password,
            ssh_key,
            ssh_key_passphrase,
            ..
        } => {
            if let Some(ssh_conn) = ssh {
                println!("Starting client, connecting via SSH: {ssh_conn}");
                client::run_ssh(ssh_conn, ssh_password, ssh_key, ssh_key_passphrase).await?;
            } else {
                eprintln!("SSH feature enabled but no --ssh argument provided");
                std::process::exit(1);
            }
        }
        Commands::Server { .. } => {
            eprintln!("SSH feature only supports client mode");
            std::process::exit(1);
        }
    }

    Ok(())
}
