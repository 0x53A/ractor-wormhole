use std::marker::PhantomData;

use ractor::{
    Actor, ActorProcessingErr, ActorRef, RpcReplyPort, async_trait, concurrency::Duration,
};

// -------------------------------------------------------------------------------------------------------

//#[derive(RactorMessage)]
#[derive(ractor_wormhole_derive::WormholeSerializable)]
pub struct RpcProxyMsg<T: Send + Sync + 'static> {
    pub data: T,
}

impl<T: Send + Sync + 'static> ractor::Message for RpcProxyMsg<T> {}

// -------------------------------------------------------------------------------------------------------

pub struct RpcProxyActor<T: Send + Sync + 'static> {
    _a: PhantomData<T>,
}

impl<T: Send + Sync + 'static> RpcProxyActor<T> {
    pub fn new() -> Self {
        RpcProxyActor { _a: PhantomData }
    }
}

impl<T: Send + Sync + 'static> Default for RpcProxyActor<T> {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------------------------------------

pub struct RpcProxyActorState<T> {
    rpc_reply_port: Option<RpcReplyPort<T>>,
}
pub struct RpcProxyActorArgs<T> {
    pub rpc_reply_port: RpcReplyPort<T>,
}

#[async_trait]
impl<T: Send + Sync + 'static> Actor for RpcProxyActor<T> {
    type Msg = RpcProxyMsg<T>;
    type State = RpcProxyActorState<T>;
    type Arguments = RpcProxyActorArgs<T>;

    async fn pre_start(
        &self,
        _myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        tracing::info!("Starting the RpcProxyActor actor");
        Ok(RpcProxyActorState {
            rpc_reply_port: Some(args.rpc_reply_port),
        })
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        tracing::info!("RpcProxyActor handle message...");
        if let Some(rrp) = state.rpc_reply_port.take() {
            if let Err(err) = rrp.send(message.data) {
                tracing::error!("Failed to send message to RpcReplyPort: {}", err);
            } else {
                tracing::info!("Message sent to RpcReplyPort successfully");
            }
        }
        myself.stop(None);
        Ok(())
    }
}

// -------------------------------------------------------------------------------------------------------

pub fn rpc_reply_port_from_actor_ref<T: Send + Sync + 'static>(
    actor_ref: ActorRef<RpcProxyMsg<T>>,
    timeout: Option<Duration>,
) -> RpcReplyPort<T> {
    let (tx, rx) = ractor::concurrency::oneshot();

    let rpc_reply_port: RpcReplyPort<T> = if let Some(t) = timeout {
        (tx, t).into()
    } else {
        tx.into()
    };

    // when the reply port is triggered, forward the message to the actor
    tokio::spawn(async move {
        let result = rx.await;
        match result {
            Ok(msg) => {
                actor_ref.send_message(RpcProxyMsg { data: msg }).unwrap();
            }
            Err(err) => {
                tracing::error!("Failed to receive message from RpcReplyPort: {}", err);
            }
        }
    });

    rpc_reply_port
}

// -------------------------------------------------------------------------------------------------------
// Tests
// -------------------------------------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use ractor::concurrency::timeout;

    use super::*;

    #[tokio::test]
    async fn test_rpc_proxy_actor() -> Result<(), anyhow::Error> {
        // construct a new RpcReplyPort
        let (tx, rx) = ractor::concurrency::oneshot();
        let rpc_reply_port: RpcReplyPort<u32> = tx.into();

        // create the proxy
        let args = RpcProxyActorArgs { rpc_reply_port };
        let (rpc_proxy, _handle) = Actor::spawn(None, RpcProxyActor::<u32>::new(), args).await?;

        // on the other side, create a new RpcReplyPort from the actor
        let rpc_reply_port_2 = rpc_reply_port_from_actor_ref(rpc_proxy.clone(), None);

        assert!(rx.is_empty());

        rpc_reply_port_2.send(42)?;
        let result = timeout(Duration::from_millis(100), rx).await??;
        assert_eq!(result, 42);
        assert_eq!(
            rpc_proxy.get_cell().get_status(),
            ractor::ActorStatus::Stopped
        );

        Ok(())
    }
}
