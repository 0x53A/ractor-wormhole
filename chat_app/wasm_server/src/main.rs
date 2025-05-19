#![feature(fn_traits)]
#![feature(try_find)]
#![feature(never_type)]

mod embedded_files;

use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use ractor::ActorRef;
use ractor::concurrency::Duration;
use ractor_wormhole::nexus::NexusActorMessage;
use ractor_wormhole::portal::PortalActorMessage;
use ractor_wormhole::util::ActorRef_Ask;
use std::net::SocketAddr;
use tokio::net::TcpListener;

use clap::Parser;

use anyhow::anyhow;
use ractor_wormhole::{nexus::start_nexus, portal::Portal, util::FnActor};

use log::info;

use server::*;

/// this implements a http server that can accept either GET (heartbeat) or UPGRADE to websocket.
pub async fn http_server_fn(
    nexus: ActorRef<NexusActorMessage>,
    addr: SocketAddr,
) -> Result<!, anyhow::Error> {
    let listener = TcpListener::bind(addr).await?;

    let mut http = hyper::server::conn::http1::Builder::new();
    http.keep_alive(true);

    loop {
        let (stream, _) = listener.accept().await?;

        let nexus_copy = nexus.clone();
        let connection = http
            .serve_connection(
                TokioIo::new(stream),
                hyper::service::service_fn(move |req| hello(nexus_copy.clone(), req)),
            )
            .with_upgrades();

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = connection.await {
                eprintln!("Error serving connection: {:?}", err);
            }
        });
    }
}

pub async fn hello(
    nexus: ActorRef<NexusActorMessage>,
    mut req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    info!("Received request: {:?}", req);

    if hyper_tungstenite::is_upgrade_request(&req) {
        let (response, websocket) = hyper_tungstenite::upgrade(&mut req, None)?;

        // Spawn a task to handle the websocket connection.
        tokio::spawn(async move {
            if let Err(e) = http_server::serve_websocket(nexus, websocket).await {
                eprintln!("Error in websocket connection: {e}");
            }
        });
        // Return the response so the spawned future can continue.
        Ok(response)
    } else if req.method() == hyper::Method::GET {
        let path = req.uri().path();
        info!("GET request for path: {}", path);
        if let Some(embedded) = embedded_files::Asset::get(path.trim_start_matches("/")) {
            let mut resp = Response::new(Full::<Bytes>::from(embedded.data));
            let content_type = match path.rsplit_once(".").unwrap().1 {
                "html" => "text/html",
                "js" => "application/javascript",
                "css" => "text/css",
                "wasm" => "application/wasm",
                "json" => "application/json",
                "png" => "image/png",
                "ico" => "image/x-icon",
                _ => "application/octet-stream",
            };
            resp.headers_mut()
                .insert("Content-Type", content_type.parse().unwrap());
            return Ok(resp);
        } else if path.is_empty() || path == "/" {
            return Ok(Response::new(Full::<Bytes>::from(
                embedded_files::Asset::get("index.html").unwrap().data,
            )));
        }

        Ok(Response::builder()
            .status(404)
            .body(Full::<Bytes>::from(format!("404 Not Found: {path}")))
            .unwrap())
    } else {
        Ok(Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(Full::<Bytes>::from("Method not supported"))
            .unwrap())
    }
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Address to bind to (e.g. 127.0.0.1:8080)
    #[arg(long)]
    bind: SocketAddr,
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

    // already start the actual chat server actor
    let chat_server = chat_server::start_chatserver_actor().await?;

    // create a callback for when a client connects
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    // create the nexus. whenever a new portal is opened (aka a websocket-client connects),
    //  the callback will be invoked
    let nexus = start_nexus(None, Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    // Start the HTTP server
    println!("Starting server, binding to: {}", cli.bind);
    let nexus_clone = nexus.clone();
    tokio::spawn(async move {
        http_server_fn(nexus_clone, cli.bind).await.unwrap();
    });

    // loop around the client connection receiver
    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        let result = handle_connected_client(&chat_server, msg).await;
        if let Err(err) = result {
            eprintln!("Error handling connected client: {:#}", err);
        }
    }

    Ok(())
}

async fn handle_connected_client(
    chat_server: &ActorRef<chat_server::Msg>,
    msg: ractor_wormhole::nexus::OnActorConnectedMessage,
) -> Result<(), anyhow::Error> {
    let hub_actor = hub::spawn_hub(chat_server.clone(), msg.actor_ref.clone()).await?;

    msg.actor_ref
        .publish_named_actor("hub".to_string(), hub_actor.clone())
        .await?;

    msg.actor_ref
        .ask(
            |rpc| PortalActorMessage::WaitForHandshake(rpc),
            Some(Duration::from_secs(5)),
        )
        .await?;

    Ok(())
}
