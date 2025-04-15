use anyhow::anyhow;
use ractor::{
    ActorRef, RpcReplyPort,
    concurrency::{self, Duration},
};

// -------------------------------------------------------------------------------------------------------

#[allow(non_camel_case_types)]
pub trait ActorRef_Ask<TMessage: ractor::Message + 'static> {
    fn ask<TReply: Send, TMsgBuilder>(
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
    ) -> impl std::future::Future<Output = Result<(), anyhow::Error>>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage;
}

impl<TMessage: ractor::Message + 'static> ActorRef_Ask<TMessage> for ActorRef<TMessage> {
    fn ask<TReply: Send, TMsgBuilder>(
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
                Ok(tokio::time::timeout(duration, rx).await??)
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
    ) -> impl std::future::Future<Output = Result<(), anyhow::Error>>
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

        async move {
            send_result?;

            tokio::spawn(async move {
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
}
