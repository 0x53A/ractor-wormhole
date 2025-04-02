#![feature(fn_traits)]

mod client;
mod gateway;
mod server;
mod util;

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
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
