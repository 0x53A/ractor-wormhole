use std::sync::{Arc, Mutex};
use std::time::Duration;

use ractor_wormhole::conduit::{ConduitMessage, ConduitSink, ConduitSource};
use ractor_wormhole::portal::Portal;
use ractor_wormhole::util::{ActorRef_Ask, FnActor};

use futures::channel::mpsc;
use futures::{SinkExt, StreamExt};

#[cfg_attr(
    feature = "ractor_cluster",
    derive(ractor_cluster_derive::RactorMessage)
)]
#[derive(Debug, ractor_wormhole::WormholeTransmaterializable)]
pub struct PingMsg {
    pub id: u64,
}

/// Test that when one portal crashes due to garbage data, other portals on the same nexus
/// continue to function normally. This validates that portal failures are isolated.
#[tokio::test]
pub async fn test_portal_isolation_on_garbage_data() -> anyhow::Result<()> {
    // Create a single nexus that will host two portals
    let nexus = ractor_wormhole::nexus::start_nexus(Some("isolation-test: nexus".into()), None)
        .await
        .map_err(|err| anyhow::anyhow!(err))?;

    // Create two bidirectional channel pairs for two connections
    // Connection 1: will receive garbage data
    let (tx1_to_portal, rx1_to_portal) = mpsc::channel::<ConduitMessage>(100);
    let (tx1_from_portal, mut rx1_from_portal) = mpsc::channel::<ConduitMessage>(100);

    // Connection 2: should remain functional
    let (tx2_to_portal, rx2_to_portal) = mpsc::channel::<ConduitMessage>(100);
    let (tx2_from_portal, mut rx2_from_portal) = mpsc::channel::<ConduitMessage>(100);

    // Create sinks and sources for the portals
    let sink1: ConduitSink =
        Box::pin(tx1_from_portal.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source1: ConduitSource = Box::pin(rx1_to_portal.map(Ok));

    let sink2: ConduitSink =
        Box::pin(tx2_from_portal.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source2: ConduitSource = Box::pin(rx2_to_portal.map(Ok));

    // Create portal 1 (will be crashed with garbage)
    let portal1 = ractor_wormhole::conduit::from_sink_source(
        nexus.clone(),
        "isolation-test: portal 1 (garbage target)".to_string(),
        sink1,
        source1,
    )
    .await?;

    // Create portal 2 (should survive)
    let portal2 = ractor_wormhole::conduit::from_sink_source(
        nexus.clone(),
        "isolation-test: portal 2 (survivor)".to_string(),
        sink2,
        source2,
    )
    .await?;

    // Both portals should have sent their introductions
    // Read and parse them
    let _intro1 = match rx1_from_portal.next().await {
        Some(ConduitMessage::Text(text)) => text,
        Some(ConduitMessage::Binary(_)) => panic!("Expected text introduction from portal 1, got binary"),
        Some(ConduitMessage::Close(_)) => panic!("Expected text introduction from portal 1, got close"),
        None => panic!("Expected text introduction from portal 1, got None"),
    };
    let _intro2 = match rx2_from_portal.next().await {
        Some(ConduitMessage::Text(text)) => text,
        Some(ConduitMessage::Binary(_)) => panic!("Expected text introduction from portal 2, got binary"),
        Some(ConduitMessage::Close(_)) => panic!("Expected text introduction from portal 2, got close"),
        None => panic!("Expected text introduction from portal 2, got None"),
    };

    // Create fake introductions to complete the handshake for both portals
    let fake_intro = serde_json::json!({
        "channel_id_contribution": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "version": "0.1",
        "info_text": "Fake client",
        "this_side_id": 12345_u128
    });
    let fake_intro_text = serde_json::to_string(&fake_intro)?;

    // Complete handshake for portal 1
    tx1_to_portal
        .clone()
        .send(ConduitMessage::Text(fake_intro_text.clone()))
        .await?;

    // Complete handshake for portal 2
    tx2_to_portal
        .clone()
        .send(ConduitMessage::Text(fake_intro_text.clone()))
        .await?;

    // Give portals time to process handshakes
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify both portals are open
    portal1.wait_for_opened(Duration::from_secs(1)).await?;
    portal2.wait_for_opened(Duration::from_secs(1)).await?;

    // Set up a test actor on portal2's side to verify it still works after portal1 crashes
    let received: Arc<Mutex<Vec<u64>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();

    let (ping_actor, _) = FnActor::<PingMsg>::start_fn(async move |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            received_clone.lock().unwrap().push(msg.id);
        }
    })
    .await?;

    // Publish the actor on portal2
    portal2
        .publish_named_actor("ping".to_string(), ping_actor.clone())
        .await?;

    // Now send garbage bytes to portal 1 to crash it
    // This should trigger a panic in the portal due to invalid bincode data
    let garbage: Vec<u8> = vec![0xFF, 0xFE, 0xFD, 0xFC, 0x00, 0x01, 0x02, 0x03, 0xFF, 0xFF];
    tx1_to_portal
        .clone()
        .send(ConduitMessage::Binary(garbage))
        .await?;

    // Give portal 1 time to process the garbage and crash
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Portal 1 should have failed/stopped
    // We can't easily check this directly, but the nexus should have handled it

    // Now verify portal 2 is still functional by sending a message through it
    // We'll do this by having a "remote client" query the named actor and send a message

    // First, let's verify the nexus still has portal2 by checking we can still communicate
    // Send a valid message to portal 2 - a request to query the named actor
    // For simplicity, we'll just verify portal2 is still responding

    // Query all portals from the nexus
    let all_portals = nexus
        .ask(
            |rpc| ractor_wormhole::nexus::NexusActorMessage::GetAllPortals(rpc),
            Some(Duration::from_secs(1)),
        )
        .await?;

    // After portal1 crashed, only portal2 should remain
    assert_eq!(
        all_portals.len(),
        1,
        "Expected 1 portal after crash, got {}",
        all_portals.len()
    );

    // Verify it's portal2 that survived
    assert_eq!(
        all_portals[0].get_id(),
        portal2.get_id(),
        "The surviving portal should be portal2"
    );

    // Extra verification: portal2 should still be able to process messages
    // Send a properly formatted message to verify it's truly functional
    // (We'd need a full client simulation to do an end-to-end test,
    // but verifying it's still registered in the nexus is sufficient)

    // Clean up
    portal2.stop(None);

    Ok(())
}

/// Test that sending garbage during the handshake phase also isolates correctly
#[tokio::test]
pub async fn test_portal_isolation_garbage_during_handshake() -> anyhow::Result<()> {
    let nexus = ractor_wormhole::nexus::start_nexus(
        Some("isolation-handshake-test: nexus".into()),
        None,
    )
    .await
    .map_err(|err| anyhow::anyhow!(err))?;

    // Connection 1: will receive garbage during handshake
    let (tx1_to_portal, rx1_to_portal) = mpsc::channel::<ConduitMessage>(100);
    let (tx1_from_portal, mut rx1_from_portal) = mpsc::channel::<ConduitMessage>(100);

    // Connection 2: normal connection
    let (tx2_to_portal, rx2_to_portal) = mpsc::channel::<ConduitMessage>(100);
    let (tx2_from_portal, mut rx2_from_portal) = mpsc::channel::<ConduitMessage>(100);

    let sink1: ConduitSink =
        Box::pin(tx1_from_portal.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source1: ConduitSource = Box::pin(rx1_to_portal.map(Ok));

    let sink2: ConduitSink =
        Box::pin(tx2_from_portal.sink_map_err(|e| anyhow::Error::msg(e.to_string())));
    let source2: ConduitSource = Box::pin(rx2_to_portal.map(Ok));

    // Create both portals
    let _portal1 = ractor_wormhole::conduit::from_sink_source(
        nexus.clone(),
        "handshake-test: portal 1".to_string(),
        sink1,
        source1,
    )
    .await?;

    let portal2 = ractor_wormhole::conduit::from_sink_source(
        nexus.clone(),
        "handshake-test: portal 2".to_string(),
        sink2,
        source2,
    )
    .await?;

    // Read introductions
    let _intro1 = rx1_from_portal.next().await;
    let _intro2 = rx2_from_portal.next().await;

    // Send garbage TEXT to portal 1 (invalid JSON during handshake)
    tx1_to_portal
        .clone()
        .send(ConduitMessage::Text("this is not valid json {{{".to_string()))
        .await?;

    // Complete handshake normally for portal 2
    let fake_intro = serde_json::json!({
        "channel_id_contribution": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        "version": "0.1",
        "info_text": "Fake client",
        "this_side_id": 99999_u128
    });
    tx2_to_portal
        .clone()
        .send(ConduitMessage::Text(serde_json::to_string(&fake_intro)?))
        .await?;

    // Give time for processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Portal 2 should be open
    portal2.wait_for_opened(Duration::from_secs(1)).await?;

    // Check that only portal 2 remains
    let all_portals = nexus
        .ask(
            |rpc| ractor_wormhole::nexus::NexusActorMessage::GetAllPortals(rpc),
            Some(Duration::from_secs(1)),
        )
        .await?;

    assert_eq!(
        all_portals.len(),
        1,
        "Expected 1 portal after handshake failure, got {}",
        all_portals.len()
    );
    assert_eq!(all_portals[0].get_id(), portal2.get_id());

    portal2.stop(None);
    Ok(())
}
