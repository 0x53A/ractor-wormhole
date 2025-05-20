use futures::StreamExt;
use http_body_util::Full;
use hyper::body::Bytes;
use hyper::{Request, Response};
use hyper_tungstenite::HyperWebsocket;
use hyper_util::rt::TokioIo;
use ractor::ActorRef;
use ractor_wormhole::conduit::{self};
use ractor_wormhole::nexus::NexusActorMessage;
use std::net::SocketAddr;
use tokio::net::TcpListener;

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
                eprintln!("Error serving connection: {err:?}");
            }
        });
    }
}

pub async fn hello(
    nexus: ActorRef<NexusActorMessage>,
    mut req: Request<hyper::body::Incoming>,
) -> Result<Response<Full<Bytes>>, anyhow::Error> {
    if hyper_tungstenite::is_upgrade_request(&req) {
        let (response, websocket) = hyper_tungstenite::upgrade(&mut req, None)?;

        if let Err(e) = serve_websocket(nexus, websocket).await {
            eprintln!("Error in websocket connection: {e}");
        }
        // Return the response so the spawned future can continue.
        Ok(response)
    } else if req.method() == hyper::Method::GET {
        // Handle regular HTTP requests here.
        Ok(Response::new(Full::<Bytes>::from(
            "https://www.youtube.com/watch?v=SXRteMSSZ14",
        )))
    } else {
        Ok(Response::builder()
            .status(hyper::StatusCode::BAD_REQUEST)
            .body(Full::<Bytes>::from("Method not supported"))
            .unwrap())
    }
}

/// Handle a websocket connection.
pub async fn serve_websocket(
    nexus: ActorRef<NexusActorMessage>,
    websocket: HyperWebsocket,
) -> Result<(), anyhow::Error> {
    let websocket = websocket.await?;

    let (tx, rx) = websocket.split();

    // need to adapt streams
    // Map the tungstenite messages to ConduitMessage
    let rx = conduit::websocket::server::tokio_tungstenite::map_ws_to_conduit(rx);

    // Convert the ConduitMessage to tungstenite Message
    let tx = conduit::websocket::server::tokio_tungstenite::map_conduit_to_ws(tx);

    // it seems to be non-trivial to get the address from hyper, and its usefulness is questionable, anyway, with proxies and vpns and what not.
    let identifier = format!("ws-client://{}", rand::random::<u64>());
    conduit::from_sink_source(nexus, identifier, tx, rx).await?;

    Ok(())
}
