                            
                            
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

Though the low-level connection does not have roles, most applications will likely have one server and multiple clients.

## Transport

Ractor Wormhole requires a trusted, bidirectional, reliable, non-fragmenting, byte-channel.

This repository implements a Websocket transport. Websockets are ideal because the web layer already handles all the annoying bits like routing, authentication, authorization, encryption.

It would be trivial to implement transports for TCP or stdio.

## Serialization

Ractor Wormhole uses a custom serialization scheme. This is required because it enables fishing ``ActorRef``s and ``RpcReplyPort``s out of deeply nested enums and structs, and then reconstructing everything on the other side.

This means all types that should be passed through the portal need to implement the custom trait ``ContextSerializable``.

Unfortunately, because Ractor Wormhole is a seperate crate from ractor, and the lack of specialisation, and the damned orphan roles, I found it impossible to write a fully generic automatic adapter for existing serialization libraries (serde and/or bincode).

The top level Message type needs to be either a primitive type for which this crate already provides an implementation, or a user defined type with the trait ``ContextSerializable`` implemented.

For individual fields, you can use automatic adaption when using our ``derive`` macro.

Example:

```rust

// these types already have serialization logic defined
#[derive(serde::Serialize, serde::Deserialize)]
pub struct SomeData { 
    // ...
}

#[derive(bincode::Encode, bincode::Decode)]
pub struct OtherData {
    // ...
}


// the actual message type needs our derive macro ...
#[derive(WormholeSerializable)]
pub struct WormholeMessage {
    // .. but we can use annotations that for the fields, it should delegate to an existing serialization library
    #[serde] pub a: SomeData,
    #[bincode] pub b: OtherData
}
```

This strongly couples this crate to serde and bincode, which i'm not exactly happy about, but well, better than nothing.

It's important that both ``ActorRef<T>`` and ``RpcReplyPort<T>`` MUST **always** be serialized through the ``ContextSerializable`` interface!
                            
## Relationship to Ractor Cluster

This project is not related to, and does not depend on, the ractor native clustering. It can be used in conjunction with a cluster on one or both sides.


## Example

With all that out of the way, let's see an example:

(please also take a look at the sample_app in this repo)

Common:

```rust
// we have defined two enums which represent the communication from client to server and server to client

/// Server To Client would otherwise typically be implemented through SSE or Websocket
pub enum ServerToClientMessage {
    // cases
}

// Client to Server would otherwise typically be a REST Api
pub enum ClientToServerMessage {
    // cases
}
```

Server:

```rust
// todo
```

Client:
```rust
// todo
```

## Use cases

The goal of this library is to make another small step in the direction of global rust domination. It should be possible to connect any and all arbitrary systems.

Specific use cases are communication between a wasm web app and its webserver (over a Websocket), communication between microcontrollers (over uart), between a hardware gadget and a smartphone app (over Bluetooth), between a USB device and a host computer (over USB), ...

Because the library transparently proxies actors, it's possible to create a multi-hop route by just forwarding the ``ActorRef`` through multiple portals.

There is work in progress to enable compiling and running ``ractor`` in wasm/web. Based on this I'll make sure ``ractor_wormhole`` also runs on wasm.

I'll investigate whether ``ractor`` can be used on a microcontroller like an ESP32 or STM32. The ``embedded-rust`` people have been hard at work creating [``embassy-rs``](https://github.com/embassy-rs/embassy), which is, besides other things, an async runtime for microcontrollers.


# Security

As the goal of this library is explicitly to allow connecting to untrusted peers, it's important to consider security implications.
These range from remote code execution to (D)DOS to data leaks.

A client can access:  
* all Actors explicitly published to the connection, and  
* all ``ActorRef``s that have been passed to it inside a ``Msg``.

This Actor registration is per-connection, so an Actor published to one client is not automatically reachable from a different client.

When projected through the portal, the integer actor ids used by ractor are replaced with unique, randomly generated, uuids. This prevents a malicious client from blindly guessing actor ids. (ractor starts with 0 and counts up for its internal actor id)

Serialization and Deserialization is as safe as the routines used. The default implementations, and the automatically derived ones should be safe.

There is no specific protection against Denial Of Service attacks yet. Time permitting, I'd like to implement a general per-connection rate-limiting and then a user can implement specific, per-actor logic.

# Utilities

Together with the Wormhole functionality, this project implements a grab bag of utility functionality on top of ractor.

This includes:

## Combinators

Adds ``ActorRef`` based transformations.

### Filter

Example

```rust
    let actor_ref: ActorRef<u32> = /* */;
    let (filtered_actor_ref, _handle) = actor_ref.filter(|msg| {
        if msg % 2 == 0 {
            FilterResult::Forward(msg)
        } else {
            FilterResult::Drop
        }
    }).await.unwrap();
```

### Map

Example:

```rust
let actor_ref: ActorRef<u32> = /* */;
let (mapped_actor_ref, _handle) = actor_ref.map(|msg: u32| { msg * 2 }).await?;
```

A more useful use case is mapping between different ``Msg`` types.

### Function Actors

I'm a big fan of the pattern ractor uses for its actors, I believe it really makes things clear. With that said, sometimes you just want a small little actor without all the ceremony.

Without further talk, the simplest example:

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

