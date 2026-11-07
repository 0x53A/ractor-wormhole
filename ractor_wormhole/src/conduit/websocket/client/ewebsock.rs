use std::{
    ops::ControlFlow,
    sync::{Arc, Mutex},
};

use anyhow::anyhow;
use ewebsock::{WsEvent, WsMessage, WsSender};
use log::{debug, error, info};
use ractor::ActorRef;
use ractor_wormhole::{
    conduit::{self, ConduitError, ConduitMessage, ConduitSink, ConduitSource},
    nexus::NexusActorMessage,
    portal::PortalActorMessage,
    util::ActorRef_Ask,
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

// The sink owns the forwarding task's lifetime. Dropping it closes the socket,
// including when portal startup fails or the Nexus rejects registration.
struct WsSenderSink {
    sender: Option<mpsc::UnboundedSender<WsMessage>>,
    shutdown: Option<oneshot::Sender<()>>,
    finished: Option<oneshot::Receiver<()>>,
}

impl WsSenderSink {
    fn close(&mut self) {
        self.sender.take();
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
    }
}

impl Drop for WsSenderSink {
    fn drop(&mut self) {
        self.close();
    }
}

impl Sink<ConduitMessage> for WsSenderSink {
    type Error = ConduitError;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(if self.sender.as_ref().is_some_and(|tx| !tx.is_closed()) {
            Ok(())
        } else {
            Err(anyhow!("Disconnected"))
        })
    }

    fn start_send(mut self: Pin<&mut Self>, item: ConduitMessage) -> Result<(), Self::Error> {
        let msg = match item {
            ConduitMessage::Handshake(text) => WsMessage::Text(text),
            ConduitMessage::Content(data) => WsMessage::Binary(data),
            ConduitMessage::Close(_) => {
                self.sender.take(); // Drain already queued messages before closing.
                return Ok(());
            }
        };
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow!("Disconnected"))?
            .send(msg)
            .map_err(|_| anyhow!("Disconnected"))
    }

    // ewebsock provides no acknowledgement of writes; flush only checks that
    // the forwarding task is still available.
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        if self.sender.is_none() {
            self.poll_close(cx)
        } else {
            self.poll_ready(cx)
        }
    }

    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.sender.take();
        if let Some(finished) = &mut self.finished {
            match std::future::Future::poll(Pin::new(finished), cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(_) => {
                    self.finished.take();
                }
            }
        }
        Poll::Ready(Ok(()))
    }
}

#[allow(non_snake_case)]
pub async fn adapt_WsSender_to_Conduit(sender: WsSender) -> Result<ConduitSink, ConduitError> {
    Ok(adapt_sender(sender, None))
}

fn adapt_sender(sender: WsSender, terminal: Option<oneshot::Receiver<()>>) -> ConduitSink {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    task::spawn(async move {
        let mut sender = sender;
        let terminal = async move {
            match terminal {
                Some(rx) => {
                    let _ = rx.await;
                }
                None => futures::future::pending::<()>().await,
            }
        };
        let forward = async {
            while let Some(msg) = rx.recv().await {
                sender.send(msg);
            }
        };
        let stop = futures::future::select(Box::pin(shutdown_rx), Box::pin(terminal));
        // Returning drops the losing future, releasing the borrow of sender.
        {
            let _ = futures::future::select(Box::pin(stop), Box::pin(forward)).await;
        }
        sender.close();
        let _ = finished_tx.send(());
    });
    Box::pin(WsSenderSink {
        sender: Some(tx),
        shutdown: Some(shutdown_tx),
        finished: Some(finished_rx),
    })
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

    let (opened_tx, opened_rx) = oneshot::channel::<Result<(), String>>();

    let (terminal_tx, terminal_rx) = oneshot::channel();
    let terminal_tx = Arc::new(Mutex::new(Some(terminal_tx)));
    let (_internal_tx, ws_rx) = mpsc::unbounded_channel();
    let opened_tx = Arc::new(Mutex::new(Some(opened_tx)));
    let handler: Box<dyn Send + Fn(WsEvent) -> ControlFlow<()>> = Box::new(move |evt| {
        match evt {
            WsEvent::Opened => {
                if let Some(opened_tx) = opened_tx.lock().unwrap().take() {
                    let _ = opened_tx.send(Ok(()));
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
            WsEvent::Error(err) => {
                error!("WebSocket error: {err}");
                if let Some(opened_tx) = opened_tx.lock().unwrap().take() {
                    let _ = opened_tx.send(Err(err.clone()));
                }
                let _ = _internal_tx.send(ConduitMessage::Close(Some(err)));
                if let Some(tx) = terminal_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                return ControlFlow::Break(());
            }
            WsEvent::Closed => {
                debug!("WebSocket connection closed");
                if let Some(opened_tx) = opened_tx.lock().unwrap().take() {
                    let _ = opened_tx.send(Err("WebSocket closed before opening".into()));
                }
                let _ = _internal_tx.send(ConduitMessage::Close(None));
                if let Some(tx) = terminal_tx.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    });
    let ws_tx = ewebsock::ws_connect(url.clone(), ewebsock::Options::default(), handler)
        .map_err(|err| anyhow!(err))?;

    // Wait for the connection to be opened
    crate::util::with_timeout(conduit::CONNECTION_SETUP_TIMEOUT, opened_rx)
        .await??
        .map_err(|err| anyhow!(err))?;

    let ws_tx = adapt_sender(ws_tx, Some(terminal_rx));
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

#[cfg(all(test, feature = "websocket_server", not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::time::Duration;
    use tokio_tungstenite::tungstenite::Message;

    #[tokio::test]
    async fn stopping_portal_closes_forwarded_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (ready, received) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ready.send(()).unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
        });
        let nexus = crate::nexus::start_nexus(None, None).await.unwrap();
        let portal = connect_to_server(nexus.clone(), format!("ws://{addr}"))
            .await
            .unwrap();
        received.await.unwrap();
        portal.stop(None);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
        nexus.stop(None);
    }

    #[tokio::test]
    async fn explicit_close_drains_queued_messages() {
        use futures::SinkExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert_eq!(
                ws.next().await.unwrap().unwrap(),
                Message::Text("hello".into())
            );
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Close(_)
            ));
        });
        let (opened, ready) = oneshot::channel();
        let opened = Mutex::new(Some(opened));
        let sender = ewebsock::ws_connect(
            format!("ws://{addr}"),
            ewebsock::Options::default(),
            Box::new(move |event| {
                if matches!(event, WsEvent::Opened) {
                    if let Some(tx) = opened.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
                ControlFlow::Continue(())
            }),
        )
        .unwrap();
        tokio::time::timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        let mut sink = adapt_WsSender_to_Conduit(sender).await.unwrap();
        sink.feed(ConduitMessage::Handshake("hello".into()))
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            sink.send(ConduitMessage::Close(None)),
        )
        .await
        .unwrap()
        .unwrap();
        drop(sink);
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn remote_close_stops_portal() {
        use futures::SinkExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
            assert!(matches!(
                ws.next().await.unwrap().unwrap(),
                Message::Text(_)
            ));
            ws.send(Message::Close(None)).await.unwrap();
        });
        let nexus = crate::nexus::start_nexus(None, None).await.unwrap();
        let portal = connect_to_server(nexus.clone(), format!("ws://{addr}"))
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(2), portal.wait(None))
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        nexus.stop(None);
    }

    #[tokio::test]
    async fn rejected_open_returns_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            // Closing before an upgrade completes must resolve the opening waiter.
            drop(stream);
        });
        let nexus = crate::nexus::start_nexus(None, None).await.unwrap();
        let result = tokio::time::timeout(
            Duration::from_secs(2),
            connect_to_server(nexus.clone(), format!("ws://{addr}")),
        )
        .await
        .unwrap();
        assert!(result.is_err());
        server.await.unwrap();
        nexus.stop(None);
    }
}
