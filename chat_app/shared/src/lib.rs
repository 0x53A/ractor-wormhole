use ractor_wormhole::WormholeTransmaterializable;

// ----------------------------------------------------------------------------------

#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct UserAlias(String);
#[derive(Debug, Clone, WormholeTransmaterializable)]
pub struct ChatMessage(String);

#[derive(Debug, WormholeTransmaterializable)]
pub enum ChatServerMessage {
    PostMessage(ChatMessage),
}

#[derive(Debug, WormholeTransmaterializable)]
pub enum ChatClientMessage {
    AssignAlias(String),
    MessageReceived(UserAlias, ChatMessage),
}
