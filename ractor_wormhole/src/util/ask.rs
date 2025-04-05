use anyhow::anyhow;
use ractor::{ActorRef, RpcReplyPort, async_trait, concurrency::Duration, rpc::CallResult};

// -------------------------------------------------------------------------------------------------------

#[allow(non_camel_case_types)]
#[async_trait]
pub trait ActorRef_Ask<TMessage: ractor::Message + 'static> {
    async fn ask<TReply: Send, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage + Send;
}

#[async_trait]
impl<TMessage: ractor::Message + 'static> ActorRef_Ask<TMessage> for ActorRef<TMessage> {
    async fn ask<TReply: Send, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage + Send,
    {
        let call_result = self.call(msg_builder, timeout_option).await?;

        match call_result {
            CallResult::Success(result) => Ok(result),
            CallResult::Timeout => {
                let type_str = std::any::type_name::<TMessage>();
                Err(anyhow!(
                    "ask: timeout ({:?}) of actor {} [{}]",
                    timeout_option,
                    self.get_id(),
                    type_str
                ))
            }
            CallResult::SenderError => {
                let type_str = std::any::type_name::<TMessage>();
                Err(anyhow!(
                    "ask: SenderError of actor {} [{}]",
                    self.get_id(),
                    type_str
                ))
            }
        }
    }
}
