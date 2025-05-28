use std::sync::{Arc, Mutex};
use std::time::Duration;

use ractor::ActorStatus;
use ractor_wormhole::conduit::{ConduitMessage, ConduitSink, ConduitSource};
use ractor_wormhole::portal::{self, Portal};
use ractor_wormhole::util::{ActorRef_Ask, FnActor};

use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};

#[cfg_attr(
    feature = "ractor_cluster",
    derive(ractor_cluster_derive::RactorMessage)
)]
#[derive(Debug, ractor_wormhole::WormholeTransmaterializable)]
pub struct HelloMsg {
    pub msg: String,
}

#[tokio::test]
pub async fn test_remote_linking_works() -> anyhow::Result<()> {
    // a bidirectional duplex channel
    let (tx1, rx1) = mpsc::channel::<ConduitMessage>(100);
    let (tx2, rx2) = mpsc::channel::<ConduitMessage>(100);

    let sink1: ConduitSink = Box::pin(tx1.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source1: ConduitSource = Box::pin(rx1.map(Ok));

    let sink2: ConduitSink = Box::pin(tx2.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source2: ConduitSource = Box::pin(rx2.map(Ok));

    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let nexus_1 = ractor_wormhole::nexus::start_nexus(Some("remote-linking: nexus 1".into()), None)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;

    let nexus_2 = ractor_wormhole::nexus::start_nexus(Some("remote-linking: nexus 2".into()), None)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;

    let portal1 = ractor_wormhole::conduit::from_sink_source(
        nexus_1,
        "remote-linking: portal 1".to_string(),
        sink1,
        source2,
    )
    .await?;

    let portal2 = ractor_wormhole::conduit::from_sink_source(
        nexus_2,
        "remote-linking: portal 2".to_string(),
        sink2,
        source1,
    )
    .await?;

    let (hello_actor, hello_actor_handle) = FnActor::<HelloMsg>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            received.lock().unwrap().push(msg.msg);
        }
    })
    .await?;

    // publish our actor on side "1"
    portal1
        .publish_named_actor("hello".to_string(), hello_actor.clone())
        .await?;

    portal2.wait_for_opened(Duration::from_secs(5)).await?;

    // lookup the actor on side "2"
    let hello_actor_id = portal2
        .ask(
            |rpc| portal::PortalActorMessage::QueryNamedRemoteActor("hello".to_string(), rpc),
            Some(Duration::from_secs(5)),
        )
        .await??;

    let hello_actor_proxy = portal2
        .instantiate_proxy_for_remote_actor(hello_actor_id)
        .await?;

    hello_actor_proxy.send_message(HelloMsg {
        msg: "Hello from side 2".to_string(),
    })?;

    // wait for the message to be processed
    tokio::time::sleep(Duration::from_millis(100)).await;

    // validate
    let received = received_clone.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0], "Hello from side 2");

    // kill the real actor and check that the proxy actor is also marked as dead
    hello_actor
        .stop_and_wait(None, Some(Duration::from_millis(100)))
        .await?;
    // wait for the information to propagate
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(hello_actor_proxy.get_status() == ActorStatus::Stopped);

    Ok(())
}
