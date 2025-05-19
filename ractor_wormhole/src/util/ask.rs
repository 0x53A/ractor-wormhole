use anyhow::anyhow;
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

            if let Some(duration) = timeout_option {
                #[cfg(all(all(target_arch = "wasm32", target_os = "unknown")))]
                {
                    // in wasm, timeout isn't Send, so use the timeout in an isolated task and push the result through a oneshot channel
                    let (tx_timeout, rx_timeout) = concurrency::oneshot();
                    wasm_bindgen_futures::spawn_local(async move {
                        let result = ractor::concurrency::timeout(duration, rx).await;
                        let _ = tx_timeout.send(result); // todo: track failures?
                    });
                    let result = rx_timeout.await???;
                    Ok(result)
                }
                #[cfg(not(all(all(target_arch = "wasm32", target_os = "unknown"))))]
                {
                    Ok(ractor::concurrency::timeout(duration, rx).await??)
                }
            } else {
                Ok(rx.await?)
            }
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
            let result = rx.await;
            match result {
                Ok(msg) => callback(Ok(msg)),
                Err(err) => callback(Err(anyhow!(
                    "Failed to receive message from RpcReplyPort: {}",
                    err
                ))),
            }
        });

        Ok(())
    }
}
