// note: typically, client and server would be in separate crates, with a shared crate defining these interfaces.

use std::collections::HashMap;

use ractor::ActorRef;
use ractor::{ActorId, RpcReplyPort};
use ractor_wormhole::portal::PortalActorMessage;
use ractor_wormhole::{portal::NexusResult, util::FnActor};
use shared::{ChatClientMessage, ChatServerMessage, UserAlias};

use crate::alias_gen;

// ----------------------------------------------------------------------------------

/// the sum of all messages handled by the chat server. This can either be a message from a client forwarded through the hub,
/// or a Connect event from a new client.
pub enum Msg {
    /// forwarded from the hub
    Connect(
        ActorRef<PortalActorMessage>,
        ActorRef<ChatClientMessage>,
        RpcReplyPort<UserAlias>,
    ),
    /// forwarded from the hub
    Public(ActorRef<PortalActorMessage>, ChatServerMessage),
}

struct ChatServerState {
    /// the key is the ActorID of the **portal**
    clients: HashMap<ActorId, (ActorRef<ChatClientMessage>, UserAlias)>,
    // note: we are not keeping history, but if we did, we would do it here
    // history: Vec<UserAlias, ChatMessage>,
}

pub async fn start_chatserver_actor() -> NexusResult<ActorRef<Msg>> {
    let (actor_ref, _) = FnActor::<Msg>::start_fn(async |mut ctx| {
        let mut alias_generator = alias_gen::AliasGenerator::new();

        let mut state = ChatServerState {
            clients: HashMap::new(),
        };

        while let Some(msg) = ctx.rx.recv().await {
            match msg {
                Msg::Public(sender, msg) => {
                    // lookup the sender
                    let Some(sender) = state
                        .clients
                        .get(&sender.get_id())
                        .map(|(_, alias)| alias.clone())
                    else {
                        // error: sender not found
                        continue;
                    };

                    match msg {
                        ChatServerMessage::PostMessage(chat_message, reply_port) => {
                            // forward the message to all clients
                            for (client, _) in state.clients.values() {
                                let _ = client.send_message(ChatClientMessage::MessageReceived(
                                    sender.clone(),
                                    chat_message.clone(),
                                ));
                            }

                            // acknowledge the sent message but ignore errors on sending the acknowledgement.
                            let _ = reply_port.send(());
                        }
                    }
                }
                Msg::Connect(portal, new_client, rpc) => {
                    let user_alias = UserAlias(alias_generator.generate_capitalized());
                    state
                        .clients
                        .insert(portal.get_id(), (new_client.clone(), user_alias.clone()));

                    // send the user alias back to the client
                    let _ = rpc.send(user_alias.clone());

                    // send a message to all (other) clients about the new connection
                    for (id, (ref_, _)) in &state.clients {
                        if id != &new_client.get_id() {
                            let _ = ref_
                                .send_message(ChatClientMessage::UserConnected(user_alias.clone()));
                        }
                    }
                }
            }
        }
    })
    .await?;

    Ok(actor_ref)
}
