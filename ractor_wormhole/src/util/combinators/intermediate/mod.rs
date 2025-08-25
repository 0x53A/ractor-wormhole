use crate::util::FnActor;

pub struct SendErr;

pub trait ActorRefLike<T : Send + 'static> {
    fn send_message(&self, message: T) -> Result<(), SendErr>;
}

// --------------------------------------------------------------------------

pub trait IntoActorRef<T: Send + 'static> {
    fn into_actor_ref(&self) -> Result<ractor::ActorRef<T>, ()>;
}

impl<T: Send + 'static> IntoActorRef<T> for dyn ActorRefLike<T> {
    fn into_actor_ref(&self) -> Result<ractor::ActorRef<T>, ()> {
        
        let (actor_ref, _handle) = FnActor::start_fn_instant(async move |ctx| {
            while let x = ctx.rx.recv().await {

            }
        })?;

        Ok(actor_ref)
    }
}

// --------------------------------------------------------------------------

impl<T: Send + 'static> ActorRefLike<T> for ractor::ActorRef<T> {
    fn send_message(&self, message: T) -> Result<(), SendErr> {
        ractor::ActorRef::<T>::send_message(&self, message).map_err(|_|SendErr)
    }
}

// --------------------------------------------------------------------------

pub struct MappedActorRefLike<TFrom: Send + 'static, TTo: Send + 'static> {
    actor_ref: Box<dyn ActorRefLike<TFrom>>,
    map_f: Box<dyn Fn(TTo) -> TFrom + Send + 'static>
}

impl<TFrom: Send + 'static, TTo: Send + 'static> ActorRefLike<TTo> for MappedActorRefLike<TFrom, TTo> {
    fn send_message(&self, message: TTo) -> Result<(), SendErr> {
        let mapped_msg = (self.map_f)(message);
        self.actor_ref.send_message(mapped_msg).map_err(|_|SendErr)
    }
}


// --------------------------------------------------------------------------