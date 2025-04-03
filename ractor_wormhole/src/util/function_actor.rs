use std::marker::PhantomData;

use ractor::{
    Actor, ActorProcessingErr, ActorRef, Message, SpawnErr, async_trait, concurrency::JoinHandle,
};

// -------------------------------------------------------------------------------------------------------

/// this context is passed to the function that runs the actor.
pub struct FnActorCtx<T> {
    pub rx: ractor::concurrency::MpscReceiver<T>,
    pub actor_ref: ActorRef<T>,
}

// -------------------------------------------------------------------------------------------------------

struct FnActorImpl<T> {
    _a: PhantomData<T>,
}

impl<T: Message + Sync> FnActorImpl<T> {
    fn new() -> Self {
        Self { _a: PhantomData }
    }
}

// -------------------------------------------------------------------------------------------------------

struct FnActorState<T> {
    args: FnActorArgs<T>,
}
struct FnActorArgs<T> {
    pub tx: tokio::sync::mpsc::Sender<T>,
}

#[async_trait]
impl<T: Message + Sync> Actor for FnActorImpl<T> {
    type Msg = T;
    type State = FnActorState<T>;
    type Arguments = FnActorArgs<T>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(Self::State { args })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match state.args.tx.send(message).await {
            Ok(()) => Ok(()),
            Err(err) => {
                tracing::error!("FnActor: Failed to forward message: {}", err);
                myself.stop(None);
                Err(ActorProcessingErr::from(err))
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------------

pub struct FnActor<T> {
    _marker: PhantomData<T>,
}

impl<T: Message + Sync> FnActor<T> {
    /// start a new actor, and returns a Receive handle to its message queue.
    /// It's the obligation of the caller to poll the receive handle.
    pub async fn start() -> Result<(FnActorCtx<T>, JoinHandle<()>), SpawnErr> {
        let (tx, rx) = tokio::sync::mpsc::channel::<T>(2305843009213693951 - 1000); // todo lmao

        let args = FnActorArgs { tx };
        let (actor_ref, handle) = Actor::spawn(None, FnActorImpl::new(), args).await?;
        Ok((FnActorCtx { rx, actor_ref }, handle))
    }

    /// starts a new actor based on a function that takes the Receive handle.
    /// The function will be executed as a task, it should loop and poll the receive handle.
    pub async fn start_fn<F, Fut>(f: F) -> Result<(ActorRef<T>, JoinHandle<()>), SpawnErr>
    where
        F: FnOnce(FnActorCtx<T>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
        T: Message + Sync,
    {
        let (ctx, handle) = Self::start().await?;
        let actor_ref = ctx.actor_ref.clone();

        tokio::spawn(async move {
            f(ctx).await;
        });

        Ok((actor_ref, handle))
    }
}

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod fn_actor_tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    pub async fn test_start() {
        let (mut ctx, _handle) = FnActor::<u32>::start().await.unwrap();

        // Send a message to the actor
        ctx.actor_ref.send_message(42).unwrap();

        // Receive the message
        let msg = ctx.rx.recv().await.unwrap();
        assert_eq!(msg, 42);
    }

    #[tokio::test]
    pub async fn test_start_fn() -> Result<(), Box<dyn std::error::Error>> {
        let i = Arc::new(Mutex::new(0));

        let i_clone = i.clone();
        let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut ctx| async move {
            while let Some(msg) = ctx.rx.recv().await {
                *i_clone.lock().unwrap() = msg;
            }
        })
        .await
        .unwrap();

        // Send a message to the actor
        actor_ref.send_message(42)?;
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert!(*i.lock().unwrap() == 42);

        actor_ref.send_message(666).unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        assert!(*i.lock().unwrap() == 666);

        Ok(())
    }

    #[tokio::test]
    pub async fn start_fn_example() {
        let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut ctx| async move {
            while let Some(msg) = ctx.rx.recv().await {
                println!("Received message: {}", msg);
            }
        })
        .await
        .unwrap();

        // Send a message to the actor
        actor_ref.send_message(42).unwrap();
    }
}
