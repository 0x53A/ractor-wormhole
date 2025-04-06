mod connection;

use ractor::{ActorRef, ActorStatus};
use ractor_wormhole::{
    nexus::start_nexus,
    portal::{PortalActorMessage, UserFriendlyPortal},
    transmaterialization::GetReceiver,
    util::{ActorRef_Ask, FnActor},
};
use std::time::Duration;
use tokio::time;

use crate::common::{
    ClientToServerMessage, PingPongMsg, ServerToClientMessage, start_pingpong_actor,
};

pub async fn start_local_actor() -> Result<ActorRef<ServerToClientMessage>, anyhow::Error> {
    let (local_actor, _) = FnActor::<ServerToClientMessage>::start_fn(async |mut ctx| {
        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                ServerToClientMessage::Ask(rpc_reply_port) => {
                    print!("The server asked for a value, please enter it: ");
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).unwrap();
                    let value = input.trim().to_string();
                    println!("Sending value: {}", value);
                    rpc_reply_port.send(value).unwrap();
                }
            }
        }
    })
    .await?;

    Ok(local_actor)
}

pub async fn run(server_url: String) -> Result<(), anyhow::Error> {
    // Initialize logger
    env_logger::init_from_env(
        env_logger::Env::default().filter_or(env_logger::DEFAULT_FILTER_ENV, "info"),
    );

    // Start the nexus actor
    let nexus = start_nexus(None).await.unwrap();

    let portal = connection::connect_to_server(nexus, server_url).await?;

    // create a local actor and publish it on the portal
    let local_actor: ActorRef<ServerToClientMessage> = start_local_actor().await?;

    tokio::time::sleep(Duration::from_secs(1)).await;

    println!("Publishing local actor on portal");

    portal.send_message(PortalActorMessage::PublishNamedActor(
        "root".to_string(),
        local_actor.get_cell(),
        local_actor.get_receiver(),
        None,
    ))?;

    println!("Local actor published on portal");

    // the server also published an actor under the name "root" (note that these names are arbitrary)
    let server_root_id = portal
        .ask(
            |rpc| PortalActorMessage::QueryNamedRemoteActor("root".to_string(), rpc),
            None,
        )
        .await??;

    println!("Server root actor id: {:?}", server_root_id);

    let server_root_actor_ref: ActorRef<ClientToServerMessage> = portal
        .instantiate_proxy_for_remote_actor(server_root_id)
        .await?;

    println!("Server root actor ref: {:?}", server_root_actor_ref);

    // we can now use `server_root_actor_ref` to send messages through the portal to the server.
    let remote_pingpong = server_root_actor_ref
        .ask(
            ClientToServerMessage::GetPingPong,
            Some(Duration::from_secs(5)),
        )
        .await?;

    println!("Remote pingpong actor ref: {:?}", remote_pingpong);
    assert_eq!(remote_pingpong.get_status(), ActorStatus::Running);

    let local_pingpong = start_pingpong_actor().await?;

    println!("Local pingpong actor ref: {:?}", local_pingpong);

    remote_pingpong
        .send_message(PingPongMsg::Ping(local_pingpong.clone()))
        .unwrap();

    println!("Sent ping to remote pingpong actor");

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}
