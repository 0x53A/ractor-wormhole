use std::marker::PhantomData;

use futures::channel::mpsc as futures_mpsc;
use ractor::{
    ActorCell, ActorProcessingErr, ActorRef, Message, SpawnErr, SupervisionEvent,
    concurrency::JoinHandle,
    thread_local::{ThreadLocalActor, ThreadLocalActorSpawner},
};

#[cfg(feature = "async-trait")]
use async_trait::async_trait;

// -------------------------------------------------------------------------------------------------------

/// this context is passed to the function that runs the thread-local actor.
pub struct ThreadLocalFnActorCtx<T> {
    pub rx: futures_mpsc::Receiver<T>,
    pub actor_ref: ActorRef<T>,
}

// -------------------------------------------------------------------------------------------------------

struct ThreadLocalFnActorImpl<T> {
    _a: PhantomData<T>,
}

impl<T> Default for ThreadLocalFnActorImpl<T> {
    fn default() -> Self {
        Self { _a: PhantomData }
    }
}

// -------------------------------------------------------------------------------------------------------

struct ThreadLocalFnActorState<T> {
    args: ThreadLocalFnActorArgs<T>,
}
struct ThreadLocalFnActorArgs<T> {
    pub tx: futures_mpsc::Sender<T>,
}

impl<T: Message> ThreadLocalActor for ThreadLocalFnActorImpl<T> {
    type Msg = T;
    type State = ThreadLocalFnActorState<T>;
    type Arguments = ThreadLocalFnActorArgs<T>;

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
        match state.args.tx.try_send(message) {
            Ok(()) => Ok(()),
            Err(err) => {
                tracing::error!("ThreadLocalFnActor: Failed to forward message: {}", err);
                myself.stop(None);
                Err(ActorProcessingErr::from(format!(
                    "Failed to send message: {}",
                    err
                )))
            }
        }
    }

    // by default, an actor would stop when a child dies.
    async fn handle_supervisor_evt(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: SupervisionEvent,
        _state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        match message {
            SupervisionEvent::ActorTerminated(who, _, _)
            | SupervisionEvent::ActorFailed(who, _) => {
                // log it,
                tracing::error!(
                    "ThreadLocalFnActor: Child actor {} terminated ({:?})",
                    who.get_id(),
                    who.get_name()
                );
                // but do not stop though
            }
            _ => {}
        }
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------------

/// A functional abstraction over thread-local actor structs.
///
/// This is similar to `FnActor`, but for actors that don't implement `Send`.
/// Thread-local actors run on a single thread and cannot be sent across threads.
///
/// There are a few variations, the primary use is ``start_fn``, which takes an async callback function.
/// Alternatively you can use ``start``, which returns a raw receiver handle and you'll need to poll it for messages manually.
///
/// Both of these functions have ``_linked`` variants, which will link the actor to a supervisor using ``ractor::spawn_linked``.
/// ```rust
/// # use ractor_wormhole::util::{ThreadLocalFnActor, ThreadLocalFnActorCtx};
/// # use ractor::thread_local::ThreadLocalActorSpawner;
/// # use futures::StreamExt;
/// # use tokio::runtime;
/// #
/// # let rt = runtime::Builder::new_current_thread()
/// #     .enable_all()
/// #     .build()
/// #     .unwrap();
/// # let _: Result<(), anyhow::Error> = rt.block_on(async {
/// # let local = tokio::task::LocalSet::new();
/// # local.run_until(async {
/// // Create a spawner for thread-local actors
/// let spawner = ThreadLocalActorSpawner::new();
///
/// // spawn actor
/// let (actor_ref, _handle) = ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
///     while let Some(msg) = ctx.rx.next().await {
///         println!("Received message: {}", msg);
///     }
/// })
/// .await?;
///
/// // Send a message to the actor
/// actor_ref.send_message(42)?;
/// # Ok::<(), anyhow::Error>(())
/// # }).await
/// # });
/// ```
pub struct ThreadLocalFnActor<T> {
    _marker: PhantomData<T>,
}

// todo
const MAX_CHANNEL_SIZE: usize = 294967295 - 1000; // todo lmao

impl<T: Message> ThreadLocalFnActor<T> {
    /// start a new thread-local actor, and returns a Receive handle to its message queue.
    /// It's the obligation of the caller to poll the receive handle.
    ///
    /// * `spawner` - The [ThreadLocalActorSpawner] which controls which thread the actor runs on
    pub async fn start(
        spawner: ThreadLocalActorSpawner,
    ) -> Result<(ThreadLocalFnActorCtx<T>, JoinHandle<()>), SpawnErr> {
        let (tx, rx) = futures_mpsc::channel::<T>(MAX_CHANNEL_SIZE);

        let args = ThreadLocalFnActorArgs { tx };
        let (actor_ref, handle) = ThreadLocalFnActorImpl::<T>::spawn(None, args, spawner).await?;
        Ok((ThreadLocalFnActorCtx { rx, actor_ref }, handle))
    }

    /// start a new thread-local actor, and returns a Receive handle to its message queue.
    /// It's the obligation of the caller to poll the receive handle.
    ///
    /// * `spawner` - The [ThreadLocalActorSpawner] which controls which thread the actor runs on
    /// * `supervisor` - The supervisor actor that will monitor this actor
    pub async fn start_linked(
        spawner: ThreadLocalActorSpawner,
        supervisor: ActorCell,
    ) -> Result<(ThreadLocalFnActorCtx<T>, JoinHandle<()>), SpawnErr> {
        let (tx, rx) = futures_mpsc::channel::<T>(MAX_CHANNEL_SIZE);

        let args = ThreadLocalFnActorArgs { tx };
        let (actor_ref, handle) =
            ThreadLocalFnActorImpl::<T>::spawn_linked(None, args, supervisor, spawner).await?;
        Ok((ThreadLocalFnActorCtx { rx, actor_ref }, handle))
    }

    /// starts a new thread-local actor based on a function that takes the Receive handle.
    /// The function will be executed as a task, it should loop and poll the receive handle.
    ///
    /// * `spawner` - The [ThreadLocalActorSpawner] which controls which thread the actor runs on
    /// * `f` - The async function that will process messages
    pub async fn start_fn<F, Fut>(
        spawner: ThreadLocalActorSpawner,
        f: F,
    ) -> Result<(ActorRef<T>, JoinHandle<()>), SpawnErr>
    where
        F: FnOnce(ThreadLocalFnActorCtx<T>) -> Fut + 'static,
        Fut: std::future::Future<Output = ()>,
        T: Message,
    {
        let (ctx, handle) = Self::start(spawner).await?;
        let actor_ref = ctx.actor_ref.clone();

        // For thread-local actors, we spawn using ractor's concurrency primitives
        // which handles the platform-specific spawning
        ractor::concurrency::spawn_local(async move {
            f(ctx).await;
        });

        Ok((actor_ref, handle))
    }

    /// starts a new thread-local actor based on a function that takes the Receive handle.
    /// The function will be executed as a task, it should loop and poll the receive handle.
    ///
    /// * `spawner` - The [ThreadLocalActorSpawner] which controls which thread the actor runs on
    /// * `supervisor` - The supervisor actor that will monitor this actor
    /// * `f` - The async function that will process messages
    pub async fn start_fn_linked<F, Fut>(
        spawner: ThreadLocalActorSpawner,
        supervisor: ActorCell,
        f: F,
    ) -> Result<(ActorRef<T>, JoinHandle<()>), SpawnErr>
    where
        F: FnOnce(ThreadLocalFnActorCtx<T>) -> Fut + 'static,
        Fut: std::future::Future<Output = ()>,
        T: Message,
    {
        let (ctx, handle) = Self::start_linked(spawner, supervisor).await?;
        let actor_ref = ctx.actor_ref.clone();

        // For thread-local actors, we spawn using ractor's concurrency primitives
        // which handles the platform-specific spawning
        ractor::concurrency::spawn_local(async move {
            f(ctx).await;
        });

        Ok((actor_ref, handle))
    }

    /// start a new thread-local actor, and returns a Receive handle to its message queue.
    /// It's the obligation of the caller to poll the receive handle.
    pub fn start_instant(
        spawner: ThreadLocalActorSpawner,
    ) -> Result<
        (
            ThreadLocalFnActorCtx<T>,
            JoinHandle<Result<JoinHandle<()>, SpawnErr>>,
        ),
        SpawnErr,
    > {
        let (tx, rx) = futures_mpsc::channel::<T>(MAX_CHANNEL_SIZE);

        let args = ThreadLocalFnActorArgs { tx };
        let spawn_result = ThreadLocalFnActorImpl::<T>::spawn_instant(None, args, spawner);

        match spawn_result {
            Ok((actor_ref, handle)) => Ok((ThreadLocalFnActorCtx { rx, actor_ref }, handle)),
            Err(e) => Err(e),
        }
    }

    /// starts a new thread-local actor based on a function that takes the Receive handle.
    /// The function will be executed as a task, it should loop and poll the receive handle.
    pub fn start_fn_instant<F, Fut>(
        spawner: ThreadLocalActorSpawner,
        f: F,
    ) -> Result<(ActorRef<T>, JoinHandle<Result<JoinHandle<()>, SpawnErr>>), SpawnErr>
    where
        F: FnOnce(ThreadLocalFnActorCtx<T>) -> Fut + 'static,
        Fut: std::future::Future<Output = ()>,
        T: Message,
    {
        let (ctx, handle) = Self::start_instant(spawner)?;
        let actor_ref = ctx.actor_ref.clone();

        ractor::concurrency::spawn_local(async move {
            f(ctx).await;
        });

        Ok((actor_ref, handle))
    }
}

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod thread_local_fn_actor_tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::test]
    pub async fn test_start() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let spawner = ractor::thread_local::ThreadLocalActorSpawner::new();
                let (mut ctx, _handle) = ThreadLocalFnActor::<u32>::start(spawner).await.unwrap();

                // Send a message to the actor
                ctx.actor_ref.send_message(42).unwrap();

                // Receive the message
                use futures::StreamExt;
                let msg = ctx.rx.next().await.unwrap();
                assert_eq!(msg, 42);
            })
            .await;
    }

    #[tokio::test]
    pub async fn test_start_fn() -> Result<(), anyhow::Error> {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let i = Arc::new(Mutex::new(0));

                let i_clone = i.clone();
                let spawner = ractor::thread_local::ThreadLocalActorSpawner::new();
                let (actor_ref, _handle) =
                    ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
                        use futures::StreamExt;
                        while let Some(msg) = ctx.rx.next().await {
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

                Ok::<(), anyhow::Error>(())
            })
            .await
    }

    #[tokio::test]
    pub async fn start_fn_example() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let spawner = ractor::thread_local::ThreadLocalActorSpawner::new();
                let (actor_ref, _handle) =
                    ThreadLocalFnActor::<u32>::start_fn(spawner, |mut ctx| async move {
                        use futures::StreamExt;
                        while let Some(msg) = ctx.rx.next().await {
                            println!("Received message: {msg}");
                        }
                    })
                    .await
                    .unwrap();

                // Send a message to the actor
                actor_ref.send_message(42).unwrap();
            })
            .await;
    }
}
