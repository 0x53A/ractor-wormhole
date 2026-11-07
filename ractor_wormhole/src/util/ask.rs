use ractor::{
    ActorRef, RpcReplyPort,
    concurrency::{self, Duration},
};

// -------------------------------------------------------------------------------------------------------

#[allow(non_camel_case_types)]
pub trait ActorRef_Ask<TMessage: ractor::Message + 'static> {
    fn ask<TReply: Send + 'static, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> impl std::future::Future<Output = Result<TReply, anyhow::Error>> + Send
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage;

    fn ask_then<TReply: Send + 'static, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
        callback: impl FnOnce(Result<TReply, anyhow::Error>) + Send + 'static,
    ) -> Result<(), anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage;
}

impl<TMessage: ractor::Message + 'static> ActorRef_Ask<TMessage> for ActorRef<TMessage> {
    fn ask<TReply: Send + 'static, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> impl std::future::Future<Output = Result<TReply, anyhow::Error>> + Send
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage,
    {
        // eagerly send the message...
        let (tx, rx) = concurrency::oneshot();
        let port: RpcReplyPort<TReply> = match timeout_option {
            Some(duration) => (tx, duration).into(),
            None => tx.into(),
        };
        let send_msg_result = self.send_message(msg_builder(port));

        // lazily wait for the reply
        async move {
            send_msg_result?;

            receive_reply(rx, timeout_option).await
        }
    }

    fn ask_then<TReply: Send + 'static, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
        callback: impl FnOnce(Result<TReply, anyhow::Error>) + Send + 'static,
    ) -> Result<(), anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage,
    {
        let (tx, rx) = ractor::concurrency::oneshot();

        let rpc_reply_port: RpcReplyPort<TReply> = if let Some(t) = timeout_option {
            (tx, t).into()
        } else {
            tx.into()
        };

        // Eagerly evaluate msg_builder
        let msg = msg_builder(rpc_reply_port);
        let send_result = self.send_message(msg);

        send_result?;

        ractor::concurrency::spawn(async move {
            callback(receive_reply(rx, timeout_option).await);
        });

        Ok(())
    }
}

async fn receive_reply<T: Send + 'static>(
    rx: concurrency::OneshotReceiver<T>,
    timeout_option: Option<Duration>,
) -> Result<T, anyhow::Error> {
    match timeout_option {
        Some(duration) => Ok(with_timeout(duration, rx).await??),
        None => Ok(rx.await?),
    }
}

/// A Send timeout future on both native and browser runtimes. Dropping it also
/// cancels the browser-local operation, rather than leaving it running until expiry.
pub(crate) async fn with_timeout<T: Send + 'static>(
    duration: Duration,
    future: impl std::future::Future<Output = T> + Send + 'static,
) -> Result<T, anyhow::Error> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let (mut tx, rx) = concurrency::oneshot();
        wasm_bindgen_futures::spawn_local(async move {
            let result = {
                let operation = Box::pin(ractor::concurrency::timeout(duration, future));
                let cancelled = Box::pin(tx.closed());
                match futures::future::select(operation, cancelled).await {
                    futures::future::Either::Left((result, _)) => Some(result),
                    futures::future::Either::Right(_) => None,
                }
            };
            if let Some(result) = result {
                let _ = tx.send(result);
            }
        });
        Ok(rx.await??)
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        Ok(ractor::concurrency::timeout(duration, future).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::FnActor;

    struct Request(RpcReplyPort<u32>);

    #[cfg(feature = "ractor_cluster")]
    impl ractor::Message for Request {}

    #[tokio::test]
    async fn ask_then_times_out_while_recipient_keeps_reply_port() {
        let (mut ctx, handle) = FnActor::<Request>::start().await.unwrap();
        let (tx, rx) = concurrency::oneshot();
        ctx.actor_ref
            .ask_then(Request, Some(Duration::from_millis(20)), move |result| {
                let _ = tx.send(result);
            })
            .unwrap();

        let Request(reply) = ctx.rx.recv().await.unwrap();
        let result = concurrency::timeout(Duration::from_secs(2), rx)
            .await
            .unwrap()
            .unwrap();
        assert!(result.is_err());
        assert!(reply.is_closed());
        ctx.actor_ref.stop(None);
        handle.await.unwrap();
    }

    #[tokio::test]
    async fn ask_then_delivers_reply_with_and_without_timeout() {
        let (mut ctx, handle) = FnActor::<Request>::start().await.unwrap();
        for timeout in [None, Some(Duration::from_secs(2))] {
            let (tx, rx) = concurrency::oneshot();
            ctx.actor_ref
                .ask_then(Request, timeout, move |result| {
                    let _ = tx.send(result);
                })
                .unwrap();
            let Request(reply) = ctx.rx.recv().await.unwrap();
            reply.send(42).unwrap();
            assert_eq!(
                concurrency::timeout(Duration::from_secs(2), rx)
                    .await
                    .unwrap()
                    .unwrap()
                    .unwrap(),
                42
            );
        }
        ctx.actor_ref.stop(None);
        handle.await.unwrap();
    }
}
