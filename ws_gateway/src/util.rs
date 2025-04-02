use std::marker::PhantomData;

use ractor::{
    actor, async_trait, concurrency::JoinHandle, Actor, ActorId, ActorProcessingErr, ActorRef, RpcReplyPort, SpawnErr, SupervisionEvent
};

pub struct MappedActor<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static> {
    _a: PhantomData<TFrom>,
    _b: PhantomData<TTo>,
}

impl <TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static> MappedActor<TFrom, TTo> {
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
impl<TFrom : Send + Sync + ractor::Message + 'static, TTo : Send + Sync + ractor::Message + 'static> Actor for MappedActor<TFrom, TTo> {
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
    async fn map(self, to: impl Fn(TTo) -> TFrom + Send + 'static) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr>;
}

#[async_trait]
impl<TFrom : Send + Sync + ractor::Message + 'static, TTo : Send + Sync + ractor::Message + 'static> ActorRef_Map<TFrom, TTo> for ActorRef<TFrom> {
    async fn map(self, to: impl Fn(TTo) -> TFrom + Send + 'static) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr> {
        let args = MappedActorArgs {
            from: self.clone(),
            to: Box::new(to),
        };
        let result = Actor::spawn_linked(None, MappedActor::new(), args, self.get_cell()).await;
        result
    }
}

