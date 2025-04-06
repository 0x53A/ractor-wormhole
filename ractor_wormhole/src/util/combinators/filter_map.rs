use std::marker::PhantomData;

use ractor::{Actor, ActorProcessingErr, ActorRef, SpawnErr, async_trait, concurrency::JoinHandle};

// -------------------------------------------------------------------------------------------------------

struct FilterMapActor<
    TFrom: Send + Sync + ractor::Message + 'static,
    TTo: Send + Sync + ractor::Message + 'static,
> {
    _a: PhantomData<TFrom>,
    _b: PhantomData<TTo>,
}

impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    FilterMapActor<TFrom, TTo>
{
    pub fn new() -> Self {
        FilterMapActor {
            _a: PhantomData,
            _b: PhantomData,
        }
    }
}

impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    Default for FilterMapActor<TFrom, TTo>
{
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------------------------------------

struct FilterMapActorState<TFrom, TTo> {
    args: FilterMapActorArgs<TFrom, TTo>,
}

struct FilterMapActorArgs<TFrom, TTo> {
    pub from: ActorRef<TFrom>,
    pub filter_map: Box<dyn Fn(TTo) -> Option<TFrom> + Send + 'static>,
}

#[async_trait]
impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    Actor for FilterMapActor<TFrom, TTo>
{
    type Msg = TTo;
    type State = FilterMapActorState<TFrom, TTo>;
    type Arguments = FilterMapActorArgs<TFrom, TTo>;

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
        if let Some(mapped_message) = (state.args.filter_map)(message) {
            state.args.from.send_message(mapped_message)?;
        }
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------------

#[allow(non_camel_case_types)]
#[async_trait]
pub trait ActorRef_FilterMap<TFrom, TTo> {
    /// Takes an `ActorRef<TFrom>` and returns an `ActorRef<TTo>`, applying a filter_map function that can
    /// both transform and filter messages. Messages where the function returns None are dropped.
    /// ```
    /// # use ractor::{ActorRef, Message};
    /// # use ractor_wormhole::util::{ActorRef_FilterMap, FnActor};
    /// #
    /// # let _ = async {
    /// # let actor_ref: ActorRef<i32> = FnActor::start().await.unwrap().0.actor_ref;
    /// let (filter_mapped_ref, handle) = actor_ref.filter_map(|msg: String| {
    ///     // Try to parse string to integer, dropping any that fail
    ///     msg.parse::<i32>().ok()
    /// }).await.unwrap();
    /// # };
    /// ```
    async fn filter_map(
        self,
        filter_map_fn: impl Fn(TTo) -> Option<TFrom> + Send + 'static,
    ) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr>;
}

#[async_trait]
impl<TFrom: Send + Sync + ractor::Message + 'static, TTo: Send + Sync + ractor::Message + 'static>
    ActorRef_FilterMap<TFrom, TTo> for ActorRef<TFrom>
{
    async fn filter_map(
        self,
        filter_map_fn: impl Fn(TTo) -> Option<TFrom> + Send + 'static,
    ) -> Result<(ActorRef<TTo>, JoinHandle<()>), SpawnErr> {
        let args = FilterMapActorArgs {
            from: self.clone(),
            filter_map: Box::new(filter_map_fn),
        };

        FilterMapActor::spawn_linked(None, FilterMapActor::new(), args, self.get_cell()).await
    }
}

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod test_filter_map {
    use crate::util::FnActor;

    use super::*;

    #[tokio::test]
    pub async fn test_filter_map_basic() {
        // Create an actor that accepts i32 messages
        let (actor, _) = FnActor::<i32>::start().await.unwrap();

        // Create an actor that accepts strings but only forwards those that can be parsed as i32
        let (filter_mapped_actor, _handle) = actor
            .actor_ref
            .filter_map(|msg: String| msg.parse::<i32>().ok())
            .await
            .unwrap();

        // These should be forwarded
        filter_mapped_actor.send_message("123".to_string()).unwrap();
        filter_mapped_actor
            .send_message("-456".to_string())
            .unwrap();

        // These should be dropped (not parseable as i32)
        filter_mapped_actor.send_message("abc".to_string()).unwrap();
        filter_mapped_actor
            .send_message("12.34".to_string())
            .unwrap();
    }

    #[tokio::test]
    pub async fn test_filter_map_complex() {
        // Create an actor that accepts u32 messages
        let (actor, _) = FnActor::<u32>::start().await.unwrap();

        // Create an actor that accepts i64 but only forwards positive even numbers as u32
        let (filter_mapped_actor, _handle) = actor
            .actor_ref
            .filter_map(|msg: i64| {
                if msg > 0 && msg % 2 == 0 && msg <= u32::MAX as i64 {
                    Some(msg as u32)
                } else {
                    None
                }
            })
            .await
            .unwrap();

        // These should be forwarded (positive, even, within u32 range)
        filter_mapped_actor.send_message(2_i64).unwrap();
        filter_mapped_actor.send_message(100_i64).unwrap();

        // These should be dropped (negative, odd, or outside u32 range)
        filter_mapped_actor.send_message(-4_i64).unwrap(); // Negative
        filter_mapped_actor.send_message(3_i64).unwrap(); // Odd
        filter_mapped_actor.send_message(5_000_000_000_i64).unwrap(); // Too large for u32
    }
}
