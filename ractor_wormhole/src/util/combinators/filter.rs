use ractor::{ActorRef, SpawnErr, concurrency::JoinHandle};

use async_trait::async_trait;

use crate::util::FnActor;

// -------------------------------------------------------------------------------------------------------
// Note: I'd like to change it to use `Fn(&TMessage) -> FilterResult`,
//        and remove the value from `FilterResult::Forward`, but that caused issues with async_trait.
//       As-is, the filter function could theoretically modify the message, so it's not a pure filter.
// -------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub enum FilterResult<T: Send + Sync + ractor::Message + 'static> {
    Forward(T),
    Drop,
}

#[allow(non_camel_case_types)]
#[async_trait]
pub trait ActorRef_Filter<TMessage: Send + Sync + ractor::Message + 'static> {
    /// passing in an `ActorRef<TMessage>` and a filter function, and returning an `ActorRef<TMessage>`. Messages that are filtered out will be dropped.
    /// ```
    /// # use ractor::{ActorRef, Message};
    /// # use ractor_wormhole::util::{ActorRef_Map, FnActor};
    /// #
    /// # let _ = async {
    /// # let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().0.actor_ref;
    /// let (mapped_actor_ref, handle) = actor_ref.map(|msg: u32| { msg * 2 }).await.unwrap();
    /// # };
    /// ```
    async fn filter(
        self,
        f: impl Fn(TMessage) -> FilterResult<TMessage> + Send + Sync + 'static,
    ) -> Result<(ActorRef<TMessage>, JoinHandle<()>), SpawnErr>;
}

#[async_trait]
impl<TMessage: Send + Sync + ractor::Message + 'static> ActorRef_Filter<TMessage>
    for ActorRef<TMessage>
{
    async fn filter(
        self,
        f: impl Fn(TMessage) -> FilterResult<TMessage> + Send + Sync + 'static,
    ) -> Result<(ActorRef<TMessage>, JoinHandle<()>), SpawnErr> {
        let (actor_ref, handle) =
            FnActor::<TMessage>::start_fn_linked(self.get_cell(), async move |mut ctx| {
                while let Some(msg) = ctx.rx.recv().await {
                    match f(msg) {
                        FilterResult::Forward(msg) => {
                            if let Err(err) = self.send_message(msg) {
                                tracing::error!("FnActor: Failed to forward message: {}", err);
                                //ctx.actor_ref.stop(None);
                                // todo: decide what to do on error. Stop?
                            }
                        }
                        FilterResult::Drop => { /* todo: logging */ }
                    }
                }
            })
            .await?;

        Ok((actor_ref, handle))
    }
}

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod test_filter {
    use crate::util::FnActor;

    use super::*;

    #[tokio::test]
    pub async fn test_filter_int() {
        let actor_ref: ActorRef<u32> = FnActor::start().await.unwrap().0.actor_ref;
        let (_filtered_actor_ref, _handle) = actor_ref
            .filter(|msg| {
                if msg % 2 == 0 {
                    FilterResult::Forward(msg)
                } else {
                    FilterResult::Drop
                }
            })
            .await
            .unwrap();
    }
}
