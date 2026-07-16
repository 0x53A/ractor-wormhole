use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use ewebsock::{WsEvent, WsMessage, WsSender};
use log::{debug, error, info};
use ractor::{ActorRef, ActorStatus};
use ractor_wormhole::{
    conduit::{self, ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
    util::{ActorRef_Ask, FnActor},
};
#[cfg(not(target_arch = "wasm32"))]
use tokio::{
    sync::{
        mpsc::{self, UnboundedReceiver},
        oneshot,
    },
    task,
};

#[cfg(target_arch = "wasm32")]
use tokio_with_wasm::alias::{
    sync::{
        mpsc::{self, UnboundedReceiver},
        oneshot,
    },
    task,
};

use futures::{
    Sink,
    task::{Context, Poll},
};
use std::pin::Pin;

// Define a struct to wrap WsSender and implement Sink
struct WsSenderSink {
    sender: ActorRef<ConduitMessage>,
}

impl Sink<ConduitMessage> for WsSenderSink {
    type Error = ConduitError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.sender.get_status() == ActorStatus::Running {
            Poll::Ready(Ok(()))
        } else {
            Poll::Ready(Err(anyhow!("Disconnected")))
        }
    }

    fn start_send(self: Pin<&mut Self>, item: ConduitMessage) -> Result<(), Self::Error> {
        let _ = self.sender.send_message(item);
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        let _ = self
            .sender
            .send_message(ConduitMessage::Close(Some("Closing the Sink".to_string())));
        Poll::Ready(Ok(()))
    }
}

// note: WsSender is `struct { tx: Option<std::sync::mpsc::Sender<WsMessage>>, }`
// note: ConduitSink is `Pin<Box<dyn Sink<ConduitMessage, Error = ConduitError> + Send>>`
#[allow(non_snake_case)]
pub async fn adapt_WsSender_to_Conduit(sender: WsSender) -> Result<ConduitSink, ConduitError> {
    let (tx, mut rx) = mpsc::unbounded_channel();

    task::spawn(async move {
        let mut sender = sender;
        while let Some(msg) = rx.recv().await {
            sender.send(msg);
        }
        sender.close();
    });

    let (actor_ref, _handle) = FnActor::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                ConduitMessage::Handshake(text) => {
                    tx.send(WsMessage::Text(text.to_string())).unwrap()
                }
                ConduitMessage::Content(data) => tx.send(WsMessage::Binary(data.to_vec())).unwrap(),
                ConduitMessage::Close(close_frame) => {
                    debug!("Closing the WebSocket connection: {close_frame:?}");
                    ctx.actor_ref.stop(None);
                }
            }
        }
    })
    .await?;

    // Create and box the sink implementation
    let sink = WsSenderSink { sender: actor_ref };
    Ok(Box::pin(sink))
}

// note: ConduitSource is `Pin<Box<dyn Stream<Item = Result<ConduitMessage, ConduitError>> + Send>>`
#[allow(non_snake_case)]
pub fn adapt_tokio_receiver_to_Conduit(rx: UnboundedReceiver<ConduitMessage>) -> ConduitSource {
    // Create a stream from the receiver
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|msg| (Ok(msg), rx))
    });

    // Box and pin the stream
    Box::pin(stream)
}

pub async fn connect_to_server(
    nexus: ActorRef<NexusActorMessage>,
    url: String,
) -> Result<ActorRef<PortalActorMessage>, anyhow::Error> {
    info!("Connecting to WebSocket server at: {url}");

    let (opened_tx, opened_rx) = oneshot::channel();

    let (_internal_tx, ws_rx) = mpsc::unbounded_channel();
    let opened_tx = Arc::new(Mutex::new(Some(opened_tx)));
    let handler: Box<dyn Send + Fn(WsEvent) -> ControlFlow<()>> = Box::new(move |evt| {
        match evt {
            WsEvent::Opened => {
                if let Some(opened_tx) = opened_tx.lock().unwrap().take() {
                    let _ = opened_tx.send(());
                };
                info!("WebSocket connection opened");
            }
            WsEvent::Message(ws_message) => {
                match ws_message {
                    WsMessage::Text(text) => {
                        let _ = _internal_tx.send(ConduitMessage::Handshake(text.to_string()));
                    }
                    WsMessage::Binary(bin) => {
                        let _ = _internal_tx.send(ConduitMessage::Content(bin));
                    }
                    _ => {}
                };
            }
            WsEvent::Error(_) => {
                error!("WebSocket error");
            }
            WsEvent::Closed => {
                debug!("WebSocket connection closed");
            }
        }
        ControlFlow::Continue(())
    });
    let ws_tx = ewebsock::ws_connect(url.clone(), ewebsock::Options::default(), handler)
        .map_err(|err| anyhow!(err))?;

    // Wait for the connection to be opened
    opened_rx.await.unwrap();

    let ws_tx = adapt_WsSender_to_Conduit(ws_tx).await.unwrap();
    let ws_rx = adapt_tokio_receiver_to_Conduit(ws_rx);

    // Register the portal with the nexus actor
    let portal_identifier = url.to_string();
    let portal = nexus
        .ask(
            |rpc| NexusActorMessage::Connected(portal_identifier.clone(), ws_tx, rpc),
            None,
        )
        .await?;

    info!("Portal actor started for: {url}");

    let portal_actor_copy = portal.clone();
    task::spawn(
        async move { conduit::receive_loop(ws_rx, portal_identifier, portal_actor_copy).await },
    );

    Ok(portal)
}
