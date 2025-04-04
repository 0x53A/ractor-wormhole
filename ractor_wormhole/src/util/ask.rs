use anyhow::anyhow;
use ractor::{ActorRef, RpcReplyPort, concurrency::Duration, rpc::CallResult};

// -------------------------------------------------------------------------------------------------------

// #[derive(Debug)]
// pub enum AskError<TMessage> {
//     MessagingErr(MessagingErr<TMessage>),
//     /// Timeout
//     Timeout,
//     /// The transmission channel was dropped without any message(s) being sent
//     SenderError,
// }

// impl<TMessage> core::fmt::Debug for AskError<TMessage> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             AskError::MessagingErr(err) => write!(f, "Messaging error: {}", err),
//             AskError::Timeout => write!(f, "Timeout"),
//             AskError::SenderError => write!(f, "Sender error"),
//         }
//     }
// }
// impl<TMessage> core::fmt::Display for AskError<TMessage> {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         match self {
//             AskError::MessagingErr(err) => write!(f, "Messaging error: {}", err),
//             AskError::Timeout => write!(f, "Timeout"),
//             AskError::SenderError => write!(f, "Sender error"),
//         }
//     }
// }

// impl<TMessage> std::error::Error for AskError<TMessage> {}

// impl<TMessage> From<MessagingErr<TMessage>> for AskError<TMessage> {
//     fn from(err: MessagingErr<TMessage>) -> Self {
//         AskError::MessagingErr(err)
//     }
// }

pub trait ActorRef_Ask<TMessage: ractor::Message + 'static> {
    async fn ask<TReply, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage;
}

impl<TMessage: ractor::Message + 'static> ActorRef_Ask<TMessage> for ActorRef<TMessage> {
    async fn ask<TReply, TMsgBuilder>(
        &self,
        msg_builder: TMsgBuilder,
        timeout_option: Option<Duration>,
    ) -> Result<TReply, anyhow::Error>
    where
        TMsgBuilder: FnOnce(RpcReplyPort<TReply>) -> TMessage,
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
