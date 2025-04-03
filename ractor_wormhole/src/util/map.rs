use std::marker::PhantomData;

use ractor::{Actor, ActorProcessingErr, ActorRef, SpawnErr, async_trait, concurrency::JoinHandle};

// -------------------------------------------------------------------------------------------------------

struct MappedActor<
    TFrom: Send + Sync + ractor::Message + 'static,
    TTo: Send + Sync + ractor::Message + 'static,
> {
    _a: PhantomData<TFrom>,
    _b: PhantomData<TTo>,
}

// impl MappedActor
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

// impl Default for MappedActor
impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    Default for MappedActor<TFrom, TTo>
{
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------------------------------------

struct MappedActorState<TFrom, TTo> {
    args: MappedActorArgs<TFrom, TTo>,
}
struct MappedActorArgs<TFrom, TTo> {
    pub from: ActorRef<TFrom>,
    pub to: Box<dyn Fn(TTo) -> TFrom + Send + 'static>,
}

// impl Actor for MappedActor
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

// -------------------------------------------------------------------------------------------------------

#[allow(non_camel_case_types)]
#[async_trait]
pub trait ActorRef_Map<TFrom, TTo> {
    /// passing in an `ActorRef<TFrom>` and returning an `ActorRef<TTo>`
    /// ```
    /// # use ractor::{ActorRef, Message};
    /// # use ractor_wormhole::util::{ActorRef_Map, FnActor};
    /// #
    /// # let _ = async {
    /// # let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().0.actor_ref;
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

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod test_map {
    use crate::util::FnActor;

    use super::*;

    #[tokio::test]
    pub async fn test_map_int() {
        let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().0.actor_ref;

        let (_mapped_actor_ref, _handle) = actor_ref.map(|msg: u32| msg * 2).await.unwrap();
    }
}
