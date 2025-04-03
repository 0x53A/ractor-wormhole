mod connection;

use ractor::RpcReplyPort;
use ractor_wormhole::{gateway::WSConnectionMessage, util::FnActor};
use std::time::Duration;
use tokio::time;

use crate::common::ServerToClientMessage;

pub async fn run(server_url: String) -> Result<(), Box<dyn std::error::Error>> {
    let (gateway, connection) = connection::establish_connection(server_url).await?;

    // create a local actor and publish it on the connection
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

    connection
        .send_message(WSConnectionMessage::PublishNamedActor(
            "root".to_string(),
            local_actor.get_cell(),
            None,
        ))
        .unwrap();

    // Keep the main task alive
    loop {
        time::sleep(Duration::from_secs(1)).await;
    }
}
