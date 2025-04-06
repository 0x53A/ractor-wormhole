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
        /// Server URL to connect to
        #[arg(long)]
        url: String,
    },
    /// Start in server mode
    Server {
        /// Address to bind to (e.g. 127.0.0.1:8080)
        #[arg(long)]
        bind: SocketAddr,
    },
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("ERROR: {:#}", e); // Pretty format with all causes
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
        Commands::Client { url } => {
            println!("Starting client, connecting to: {}", url);
            client::run(url).await?;
        }
        Commands::Server { bind } => {
            println!("Starting server, binding to: {}", bind);
            server::run(bind).await?;
        }
    }

    Ok(())
}
