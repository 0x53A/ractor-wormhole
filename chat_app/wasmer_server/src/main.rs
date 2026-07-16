mod embedded_files;

use std::net::SocketAddr;

use anyhow::anyhow;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use ractor::{ActorRef, concurrency::Duration};
use ractor_wormhole::{
    nexus::{NexusActorMessage, start_nexus},
    portal::{Portal, PortalActorMessage},
    util::{ActorRef_Ask, FnActor},
};
use tokio::net::TcpListener;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), anyhow::Error> {
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    let bind = bind_address()?;
    run_chat_app(bind).await
}

fn bind_address() -> Result<SocketAddr, anyhow::Error> {
    let port = std::env::var("PORT").unwrap_or_else(|_| "80".to_owned());
    let bind = std::env::var("BIND_ADDRESS").unwrap_or_else(|_| format!("127.0.0.1:{port}"));

    Ok(bind.parse()?)
}

async fn run_chat_app(bind: SocketAddr) -> Result<(), anyhow::Error> {
    let chat_server = server::chat_server::start_chatserver_actor().await?;
    let (mut ctx_on_client_connected, _) = FnActor::start().await?;

    let nexus = start_nexus(None, Some(ctx_on_client_connected.actor_ref.clone()))
        .await
        .map_err(|err| anyhow!(err))?;

    println!("Starting Wasmer chat app, binding to: {bind}");
    tokio::spawn(http_server(nexus.clone(), bind));

    while let Some(msg) = ctx_on_client_connected.rx.recv().await {
        if let Err(err) = handle_connected_client(&chat_server, msg).await {
            eprintln!("Error handling connected client: {err:#}");
        }
    }

    Ok(())
}

async fn http_server(
    nexus: ActorRef<NexusActorMessage>,
    addr: SocketAddr,
) -> Result<(), anyhow::Error> {
    let listener = TcpListener::bind(addr).await?;

    let mut http = hyper::server::conn::http1::Builder::new();
    http.keep_alive(true);

    loop {
        let (stream, _) = listener.accept().await?;
        let nexus_copy = nexus.clone();
        let connection = http
            .serve_connection(
                TokioIo::new(stream),
                hyper::service::service_fn(move |req| {
                    serve_request(nexus_copy.clone(), req)
                }),
            )
            .with_upgrades();

        tokio::spawn(async move {
            if let Err(err) = connection.await {
                eprintln!("Error serving connection: {err:?}");
            }
        });
    }
}

async fn serve_request(
    nexus: ActorRef<NexusActorMessage>,
    mut req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    if hyper_tungstenite::is_upgrade_request(&req) {
        let (response, websocket) = hyper_tungstenite::upgrade(&mut req, None)?;
        tokio::spawn(async move {
            if let Err(err) = server::http_server::serve_websocket(nexus, websocket).await {
                eprintln!("Error in websocket connection: {err}");
            }
        });
        return Ok(response);
    }

    match *req.method() {
        hyper::Method::GET | hyper::Method::HEAD => serve_static(req.uri().path()),
        _ => Ok(Response::builder()
            .status(hyper::StatusCode::METHOD_NOT_ALLOWED)
            .body(Full::<Bytes>::from("Method not supported"))
            .unwrap()),
    }
}

fn serve_static(path: &str) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    if path == "/healthz" {
        return Ok(Response::new(Full::<Bytes>::from("OK")));
    }

    let asset_path = if path == "/" || path.is_empty() {
        "index.html"
    } else {
        path.trim_start_matches('/')
    };

    if let Some(asset) = embedded_files::Asset::get(asset_path) {
        let mut response = Response::new(Full::<Bytes>::from(asset.data));
        response.headers_mut().insert(
            hyper::header::CONTENT_TYPE,
            content_type(asset_path).parse().unwrap(),
        );
        return Ok(response);
    }

    Ok(Response::builder()
        .status(hyper::StatusCode::NOT_FOUND)
        .body(Full::<Bytes>::from(format!("404 Not Found: {path}")))
        .unwrap())
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "application/javascript",
        Some("css") => "text/css",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("svg") => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

async fn handle_connected_client(
    chat_server: &ActorRef<server::chat_server::Msg>,
    msg: ractor_wormhole::nexus::OnActorConnectedMessage,
) -> Result<(), anyhow::Error> {
    let hub_actor = server::hub::spawn_hub(chat_server.clone(), msg.actor_ref.clone()).await?;

    msg.actor_ref
        .publish_named_actor("hub".to_string(), hub_actor)
        .await?;

    msg.actor_ref
        .ask(
            PortalActorMessage::WaitForHandshake,
            Some(Duration::from_secs(5)),
        )
        .await?;

    Ok(())
}

#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __getrandom_v03_custom(
    dest: *mut u8,
    len: usize,
) -> Result<(), getrandom::Error> {
    if len == 0 {
        return Ok(());
    }

    if dest.is_null() {
        return Err(getrandom::Error::UNEXPECTED);
    }

    unsafe { wasi::random_get(dest, len) }.map_err(|err| getrandom::Error::new_custom(err.raw()))
}
