use std::sync::{Arc, Mutex};
use std::time::Duration;

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

pub async fn side_a(
    sink: ConduitSink,
    source: ConduitSource,
    received: Arc<Mutex<Vec<String>>>,
) -> anyhow::Result<()> {
    let nexus_1 = ractor_wormhole::nexus::start_nexus(Some("nexus 1".into()), None)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;

    let portal1 =
        ractor_wormhole::conduit::from_sink_source(nexus_1, "portal 1".to_string(), sink, source)
            .await?;

    let (hello_actor, _) = FnActor::<HelloMsg>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            received.lock().unwrap().push(msg.msg);
        }
    })
    .await?;

    // publish our actor on side "1"
    portal1
        .publish_named_actor("hello".to_string(), hello_actor.clone())
        .await?;

    portal1.wait_for_opened(Duration::from_secs(5)).await?;

    Ok(())
}

pub async fn side_b(sink: ConduitSink, source: ConduitSource) -> anyhow::Result<()> {
    let nexus_2 = ractor_wormhole::nexus::start_nexus(Some("nexus 2".into()), None)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;

    let portal2 =
        ractor_wormhole::conduit::from_sink_source(nexus_2, "portal 2".to_string(), sink, source)
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

    Ok(())
}

#[tokio::test]
pub async fn test_tiny_wormhole() -> anyhow::Result<()> {
    // a bidirectional duplex channel
    let (tx1, rx1) = mpsc::channel::<ConduitMessage>(100);
    let (tx2, rx2) = mpsc::channel::<ConduitMessage>(100);

    let sink1: ConduitSink = Box::pin(tx1.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source1: ConduitSource = Box::pin(rx1.map(Ok));

    let sink2: ConduitSink = Box::pin(tx2.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source2: ConduitSource = Box::pin(rx2.map(Ok));

    let received: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

    // spawn the two sides as independent tasks.
    // Normally these would be seperate processes or even connected over the internet.
    let a = tokio::spawn(side_a(sink1, source2, received.clone()));
    let b = tokio::spawn(side_b(sink2, source1));

    // wait for both sides to finish
    tokio::time::timeout(Duration::from_secs(1), a).await???;
    tokio::time::timeout(Duration::from_secs(1), b).await???;

    let received = received.lock().unwrap();
    assert_eq!(received.len(), 1);
    assert_eq!(received[0], "Hello from side 2");
    Ok(())
}
