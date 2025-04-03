use std::marker::PhantomData;

use ractor::{
    Actor, ActorProcessingErr, ActorRef, Message, MessagingErr, RpcReplyPort, SpawnErr,
    async_trait,
    concurrency::{Duration, JoinHandle},
    rpc::CallResult,
};

pub struct MappedActor<
    TFrom: Send + Sync + ractor::Message + 'static,
    TTo: Send + Sync + ractor::Message + 'static,
> {
    _a: PhantomData<TFrom>,
    _b: PhantomData<TTo>,
}

impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    Default for MappedActor<TFrom, TTo>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    MappedActor<TFrom, TTo>
{
    pub fn new() -> Self {
        MappedActor {
            _a: PhantomData,
            _b: PhantomData,
        }
    }
}

pub struct MappedActorState<TFrom, TTo> {
    args: MappedActorArgs<TFrom, TTo>,
}
pub struct MappedActorArgs<TFrom, TTo> {
    pub from: ActorRef<TFrom>,
    pub to: Box<dyn Fn(TTo) -> TFrom + Send + 'static>,
}

#[async_trait]
impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    Actor for MappedActor<TFrom, TTo>
{
    type Msg = TTo;
    type State = MappedActorState<TFrom, TTo>;
    type Arguments = MappedActorArgs<TFrom, TTo>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(Self::State { args })
    }

    async fn handle(
        &self,
        _myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        let mapped_message = state.args.to.call((message,));
        state.args.from.send_message(mapped_message)?;
        Ok(())
    }
}

// --------------------------------------

#[async_trait]
pub trait ActorRef_Map<TFrom, TTo> {
    /// passing in an `ActorRef<TFrom>` and returns an `ActorRef<TTo>`
    /// ```
    /// # use ractor::{ActorRef, Message};
    /// # use ractor_wormhole::util::{ActorRef_Map, FnActor};
    /// #
    /// # let _ = async {
    /// # let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().1;
    /// let (mapped_actor_ref, handle) = actor_ref.map(|msg: u32| { msg * 2 }).await.unwrap();
    /// # };
    /// ```
    async fn map(
        self,
        to: impl Fn(TTo) -> TFrom + Send + 'static,
    ) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr>;
}

#[async_trait]
impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    ActorRef_Map<TFrom, TTo> for ActorRef<TFrom>
{
    async fn map(
        self,
        to: impl Fn(TTo) -> TFrom + Send + 'static,
    ) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr> {
        let args = MappedActorArgs {
            from: self.clone(),
            to: Box::new(to),
        };

        Actor::spawn_linked(None, MappedActor::new(), args, self.get_cell()).await
    }
}

#[cfg(test)]
pub mod test_map {
    use super::*;

    #[tokio::main]
    pub async fn test_map_int() {
        let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().1;

        let (mapped_actor_ref, handle) = actor_ref.map(|msg: u32| msg * 2).await.unwrap();
    }
}

// --------------------------------------

// #[derive(Debug)]
pub enum AskError<TMessage> {
    MessagingErr(MessagingErr<TMessage>),
    /// Timeout
    Timeout,
    /// The transmission channel was dropped without any message(s) being sent
    SenderError,
}

impl<TMessage> core::fmt::Debug for AskError<TMessage> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AskError::MessagingErr(err) => write!(f, "Messaging error: {}", err),
            AskError::Timeout => write!(f, "Timeout"),
            AskError::SenderError => write!(f, "Sender error"),
        }
    }
}
impl<TMessage> core::fmt::Display for AskError<TMessage> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AskError::MessagingErr(err) => write!(f, "Messaging error: {}", err),
            AskError::Timeout => write!(f, "Timeout"),
            AskError::SenderError => write!(f, "Sender error"),
        }
    }
}

impl<TMessage> std::error::Error for AskError<TMessage> {}

impl<TMessage> From<MessagingErr<TMessage>> for AskError<TMessage> {
    fn from(err: MessagingErr<TMessage>) -> Self {
        AskError::MessagingErr(err)
    }
}

pub trait ActorRef_Ask<TMessage: ractor::Message + 'static> {
    async fn ask<TReply, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, AskError<TMessage>>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage;
}

impl<TMessage: ractor::Message + 'static> ActorRef_Ask<TMessage> for ActorRef<TMessage> {
    async fn ask<TReply, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, AskError<TMessage>>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage,
    {
        let call_result = self.call(msg_builder, timeout_option).await?;

        match call_result {
            CallResult::Success(result) => Ok(result),
            CallResult::Timeout => Err(AskError::Timeout),
            CallResult::SenderError => Err(AskError::SenderError),
        }
    }
}

// --------------------------------------

pub struct FnActor<T> {
    _a: PhantomData<T>,
}
impl<T: Message + Sync> FnActor<T> {
    fn new() -> Self {
        FnActor { _a: PhantomData }
    }

    /// start a new actor, and returns a Receive handle to its message queue.
    /// It's the obligation of the caller to poll the receive handle.
    pub async fn start() -> Result<
        (
            ractor::concurrency::MpscReceiver<T>,
            ActorRef<T>,
            JoinHandle<()>,
        ),
        SpawnErr,
    > {
        let (tx, rx) = tokio::sync::mpsc::channel::<T>(usize::MAX);

        let args = FnActorArgs { tx };
        let (actor_ref, handle) = Actor::spawn(None, FnActor::new(), args).await?;
        Ok((rx, actor_ref, handle))
    }

    /// starts a new actor based on a function that takes the Receive handle.
    /// The function will be executed as a task, it should loop and poll the receive handle.
    pub async fn start_fn<F, Fut>(f: F) -> Result<(ActorRef<T>, JoinHandle<()>), SpawnErr>
    where
        F: FnOnce(tokio::sync::mpsc::Receiver<T>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send,
        T: Message + Sync,
    {
        let (rx, actor_ref, handle) = Self::start().await?;

        tokio::spawn(async move {
            f(rx).await;
        });

        Ok((actor_ref, handle))
    }
}

#[cfg(test)]
pub mod fn_actor_tests {
    use std::sync::{Arc, Mutex};

    use super::*;

    #[tokio::main]
    pub async fn test_start() {
        let (mut rx, actor_ref, _handle) = FnActor::<u32>::start().await.unwrap();

        // Send a message to the actor
        actor_ref.send_message(42).unwrap();

        // Receive the message
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg, 42);
    }

    #[tokio::main]
    pub async fn test_start_fn() {
        let i = Arc::new(Mutex::new(0));

        let i_clone = i.clone();
        let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut rx| async move {
            while let Some(msg) = rx.recv().await {
                *i_clone.lock().unwrap() = msg;
            }
        })
        .await
        .unwrap();

        // Send a message to the actor
        actor_ref.send_message(42).unwrap();
        assert!(*i.lock().unwrap() == 42);

        actor_ref.send_message(666).unwrap();
        assert!(*i.lock().unwrap() == 666);
    }

    #[tokio::main]
    pub async fn start_fn_example() {
        let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut rx| async move {
            while let Some(msg) = rx.recv().await {
                println!("Received message: {}", msg);
            }
        })
        .await
        .unwrap();

        // Send a message to the actor
        actor_ref.send_message(42).unwrap();
    }
}

pub struct FnActorState<T> {
    args: FnActorArgs<T>,
}
pub struct FnActorArgs<T> {
    pub tx: tokio::sync::mpsc::Sender<T>,
}

#[async_trait]
impl<T: Message + Sync> Actor for FnActor<T> {
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
