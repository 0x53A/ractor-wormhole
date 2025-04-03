                            
                            
      x~~            x~~     
     xxx~~          xxx~~       
    xx xx~~        xx xx~~   
    x . x~~ ────── x . x~~      
    x . x~~        x . x~~      
    x . x~~        x . x~~      
    x . x~~        x . x~~      
    x . x~~ ────── x . x~~      
    xx xx~~        xx xx~~   
     xxx~~          xxx~~       
      x~~            x~~   


# Ractor Wormhole

Ractor Wormhole connects two actor systems and allows selectively exposing specific actors to the other side. It is not a clustering solution, each connection is 1:1, though you can open multiple connections.

It very explicitly supports connecting to untrusted peers and is meant to be a replacement for HTTP/REST for full-stack rust applications.

Though the low-level connection does not have roles, most applications will have one server and multiple clients.

## Transport

Ractor Wormhole requires a trusted, bidirectional, reliable, non-fragmenting, byte-channel.
This repository implements a Websocket transport. Websockets are ideal because the web layer already handles all the annoying bits like routing, authentication, authorization, encryption. It would be trivial to implement transports for TCP or stdio. With a bit of additional logic for retransmission etc it should be possible to take advantage of lower-level non-reliable transports like UDP (without modifictations required to Ractor Wormhole itself).

                            
## Relationship to Ractor Cluster

This project is not related to, and does not depend on, the ractor native clustering. It can be used in conjunction with a cluster on one or both sides.

# Utilities

Together with the Wormhole functionality, this project implements a grab bag of utility functionality on top of ractor.

This includes:

### Map

Adds ``ActorRef`` based transformations.

Example:

```rust
// todo

```

### Function Actors

I'm a big fan of the pattern ractor uses for its actors, I believe it really makes things clear. With that said, sometimes you just want a small little actor without all the ceremony.

Without further ado:

```rust
// spawn actor
let (actor_ref, _handle) = FnActor::<u32>::start_fn(|mut ctx| async move {
    while let Some(msg) = ctx.rx.recv().await {
        println!("Received message: {}", msg);
    }
})
.await?;

// Send a message to the actor
actor_ref.send_message(42)?;

```

To compare it to the normal ractor pattern:


```rust
#[async_trait]
impl Actor for MyActor {
    type Msg = MyMessage;
    type State = MyState;
    type Arguments = MyArgs;

    async fn pre_start(
        &self,
        myself: ActorRef<Self::Msg>,
        args: Self::Arguments,
    ) -> Result<Self::State, ActorProcessingErr> {
        Ok(Self::State {})
    }

    async fn handle(
        &self,
        myself: ActorRef<Self::Msg>,
        message: Self::Msg,
        state: &mut Self::State,
    ) -> Result<(), ActorProcessingErr> {
        println!("Received message: {:?}", message);
        Ok(())
    }
}

// is equivalent to:

pub async fn start_actor(args: MyArgs) -> Result<ActorRef<MyMessage>, SpawnErr> {
    let (actor_ref, _handle) = FnActor::<MyMessage>::start_fn(|mut ctx| async move {
        // [pre_start]
        let mut state: MyState = MyState {};

        while let Some(msg) = ctx.rx.recv().await {
            // [handle]
            println!("Received message: {:?}", msg);
        }
    })
    .await?;

    Ok(actor_ref)
}

```

# Ractor Wishlist:

### Call:

Change signature from ``Result<CallResult<TReply>, MessagingErr<TMessage>>`` to ``Result<TReply, _>`` so it can be more easily unwrapped with ``?``.

### RpcReplyPort as Message

Implement ``ractor::Message`` for ``RpcReplyPort<T>`` so it can be directly used as a Msg type instead of having to wrap it.

### Generic Messages

```rust
#[derive(RactorMessage)]
pub struct SomeMessage<T: Send + Sync + 'static> {
    // ...
}
```

fails with ``Missing generics for struct `SomeMessage` `` because the derive macro doesn't properly handle the generic parameter. A manual implementation is possible.

