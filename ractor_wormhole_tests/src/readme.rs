// -------------------------------------------------

// This file contains the code fragments for README.md.

#![allow(unused)]

// -------------------------------------------------

use ractor::{Actor, ActorProcessingErr, ActorRef, SpawnErr, async_trait};
use ractor_cluster_derive::RactorMessage;
use ractor_wormhole::util::FnActor;

pub struct MyActor;

#[derive(RactorMessage, Debug)]
pub struct MyMessage {}
pub struct MyArgs {}
pub struct MyState {}

// -------------------------------------------------

pub async fn example_1() -> Result<(), Box<dyn std::error::Error>> {
    let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut ctx| async move {
        while let Some(msg) = ctx.rx.recv().await {
            println!("Received message: {}", msg);
        }
    })
    .await?;

    // Send a message to the actor
    actor_ref.send_message(42)?;

    Ok(())
}

// -------------------------------------------------
// <| BEGIN |>
#[async_trait]
impl Actor for MyActor {
    type Msg = MyMessage;
    type State = MyState;
    type Arguments = MyArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(Self::State {})
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Received message: {:?}", message);
        Ok(())
    }
}

pub async fn start_actor(args: MyArgs) -> Result<ActorRef<MyMessage>, SpawnErr> {
    let (actor_ref, _handle) = FnActor::<MyMessage>::start_fn(|mut ctx| async move {
        // [pre_start]
        let mut state: MyState = MyState {};

        while let Some(msg) = ctx.rx.recv().await {
            // [handle]
            println!("Received message: {:?}", msg);
        }
    })
    .await?;

    Ok(actor_ref)
}
// <| END |>
// -------------------------------------------------
