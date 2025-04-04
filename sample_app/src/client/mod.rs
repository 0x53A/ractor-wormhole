mod connection;

use ractor::ActorRef;
use ractor_wormhole::{
    gateway::WSConnectionMessage,
    util::{ActorRef_Ask, FnActor},
};
use std::time::Duration;
use tokio::time;

use crate::common::{ClientToServerMessage, ServerToClientMessage};

pub async fn start_local_actor()
-> Result<ActorRef<ServerToClientMessage>, anyhow::Error> {
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
    let (_gateway, connection) = connection::establish_connection(server_url).await?;

    // create a local actor and publish it on the connection
    let local_actor: ActorRef<ServerToClientMessage> = start_local_actor().await?;

    connection.send_message(WSConnectionMessage::PublishNamedActor(
        "root".to_string(),
        local_actor.get_cell(),
        local_actor.rece
        None,
    ))?;

    // the server also published an actor under the name "root" (note that these names are arbitrary)
    let server_root_id = connection
        .ask(
            |rpc| WSConnectionMessage::QueryNamedRemoteActor("root".to_string(), rpc),
            None,
        )
        .await??;
    let server_root_cell: ractor::ActorCell = connection
        .ask(
            |rpc| WSConnectionMessage::GetRemoteActorById(server_root_id, rpc),
            None,
        )
        .await??;
    let server_root_actor_ref: ActorRef<ClientToServerMessage> = ActorRef::from(server_root_cell);

    // we can now use `server_root_actor_ref` to send messages through the portal to the server.

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}
