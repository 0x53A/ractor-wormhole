# Analysis: decoupling ractor_wormhole from ractor (and porting the core to no_std)

*Initial analysis, 2026-07-16. Goal: make the crate independent of ractor internally,
port the core to no_std for MCUs, and re-add ractor as one adapter among several.*

## 1. Where the coupling actually is

ractor types are used in two very different roles:

**As the public currency of the API (essential, by design):**
- `ActorRef<T>` and `RpcReplyPort<T>` are the things the wormhole exists to proxy.
  They appear in the `Portal`/`Nexus` traits, in `TransmaterializationContext`, and in
  the derive macro's generated code.
- `ractor::Message` is a bound on everything transmaterializable.

**As the internal implementation substrate (incidental, replaceable):**
- `PortalActor`, `NexusActor`, `RpcProxyActor`, `FnActorImpl` are `ractor::Actor`s.
- Supervision (`spawn_linked`, `handle_supervisor_evt`) implements portal isolation
  and proxy lifetime management.
- `ractor::concurrency::{spawn, spawn_local, JoinHandle, Duration, timeout, oneshot}`
  is used as the runtime abstraction (this is ractor's own tokio/wasm shim).
- The `util` grab bag (FnActor, combinators, ask) is pure ractor sugar.

The essential coupling is ~4 concepts: *a typed handle you can send messages to*,
*a one-shot reply channel*, *a stable identity for such handles*, and *a death
notification*. Everything else is implementation.

## 2. Proposed architecture: sans-io core + adapters

```
┌─────────────────────────────────────────────────────────┐
│ wormhole-core        (no_std + alloc, no async runtime) │
│  - protocol types: Introduction, CrossPortalMessage,    │
│    RemoteActorId, OpaqueActorId, framing                │
│  - transmaterialization traits + default impls          │
│  - PortalStateMachine: sans-io event loop               │
│    fn handle(&mut self, Event) -> heapless/Vec<Effect>  │
└─────────────────────────────────────────────────────────┘
          ▲                    ▲                    ▲
┌─────────┴────────┐ ┌─────────┴────────┐ ┌─────────┴────────┐
│ wormhole-ractor  │ │ wormhole-embassy │ │ (wasm, uniffi,…) │
│ (current API,    │ │ (MCU: embassy    │ │                  │
│  ActorRef/RpcPort│ │  tasks+channels) │ │                  │
│  endpoint impls) │ │                  │ │                  │
└──────────────────┘ └──────────────────┘ └──────────────────┘
```

The core state machine consumes **events** (frame received, local send requested,
endpoint died, timer fired, …) and emits **effects** (send frame, deliver message to
local endpoint, start timer, close, …). The adapter owns the runtime: it hosts the
state machine (in a ractor actor / an embassy task), drives timers, and implements
the endpoint traits.

Sketch of the endpoint abstraction the core needs (instead of `ActorCell`/`ActorRef`):

```rust
/// identity + delivery for a local message target (actor, task, channel, ...)
pub trait LocalEndpoint {
    fn id(&self) -> EndpointId;                      // replaces ActorId
    fn deliver(&self, msg: &[u8], ctx: &mut Ctx) -> Result<()>;  // replaces rematerializer+send
}
/// created by the adapter when the core needs a proxy for a remote actor
pub trait EndpointFactory { ... }
```

## 3. The crux: synchronous serialization

Today `ContextTransmaterializable::immaterialize` is `async` for exactly one reason:
serializing an `ActorRef` RPCs back into the portal actor to publish it
(`PortalActorMessage::PublishActor` + reply). This is also why
`ImmaterializeMessage` has to spawn a detached task to avoid deadlocking the
portal's own message pump.

In a sans-io core this reason disappears: serialization runs *inside* the state
machine turn, which owns the registry, so publishing an actor is a synchronous map
insert. Consequences:

- `immaterialize`/`rematerialize` can become **sync** (`fn`, not `async fn`),
  dropping `async_trait` and the boxed futures from the hot path.
- The deadlock workaround (and its `spawn` + reordering hazard) disappears.
- A sync serialization trait is trivially no_std.

This is the single highest-leverage design change, and it is worth doing *even if
the no_std port never happens*.

Note: `rematerialize` of an `RpcReplyPort` creates a proxy + timeout today; in the
core this becomes an effect (`CreateReplyProxy { timeout }`) handled by the adapter.

## 4. no_std audit of dependencies

| crate | no_std? | notes |
|---|---|---|
| bincode 2 | yes (`default-features=false`, alloc) | already the wire format |
| serde / serde_json | yes (alloc) | only used for the handshake |
| uuid | yes | |
| anyhow | yes (alloc) | error type works without std |
| rand | needs a source | MCUs: hardware RNG via `getrandom` custom handler, or inject an RNG through the adapter |
| log | yes | or `defmt` behind a feature on MCU |
| HashMap | no | swap for `hashbrown` (same API) |
| async_trait / futures | avoided | core becomes sync (see §3); `ConduitSink/Source` stay in the adapters |
| ractor | **std-only** | that's the point: it moves to an adapter |

Nothing here is a blocker. The core would be `#![no_std]` + `extern crate alloc`
(ESP32/STM32 with a heap allocator; a heapless variant is a much bigger ask and
probably not worth it initially).

## 5. What stays where

- **wormhole-core**: portal state machine, registry, opaque ids, handshake,
  framing, transmaterialization traits + primitive/tuple/Vec/String impls,
  derive macro target.
- **wormhole-ractor** (keeps the name `ractor_wormhole` for compat): Nexus/Portal
  actors hosting the state machine, `ActorRef`/`RpcReplyPort` endpoint impls,
  supervision-based lifetime management, `util` (FnActor, combinators, ask),
  the existing transports (websocket, unix socket, ssh — they are tokio-based).
- **wormhole-embassy** (later): endpoint impls over embassy channels, a UART/USB
  conduit, timer driver via `embassy_time`.
- The derive macro generates calls against wormhole-core paths; a re-export shim in
  the adapter keeps `#[derive(WormholeTransmaterializable)]` working unchanged.

## 6. Incremental migration path (each step keeps tests green)

1. **Extract the state machine in-place**: refactor `PortalActor::handle` so all
   protocol logic lives in a `PortalStateMachine` struct (still in this crate,
   still std). The actor becomes a thin driver: msg → event, effect → send/spawn.
2. **Make serialization sync** (§3): registry access moves into the state machine;
   `ContextTransmaterializable` loses `async`. Breaking change for manual impls,
   mechanical for derived ones.
3. **Introduce the endpoint traits**: replace `ActorCell`+`BoxedRematerializer`
   pairs with `Box<dyn LocalEndpoint>`; implement it for ractor refs.
4. **Crate split**: move core modules to `wormhole_core`, `ractor_wormhole`
   re-exports and adds the adapter. Public API unchanged.
5. **no_std-ify the core**: hashbrown, alloc imports, feature-gate log/rand.
   CI check: `cargo check -p wormhole_core --target thumbv7em-none-eabihf`.
6. **Proof-of-concept MCU adapter** (embassy on ESP32/STM32), stdio or UART conduit.

## 7. Effort estimate & risks

- Steps 1–2 are the substantial refactor (portal.rs is ~870 lines; the state
  machine falls out of it naturally). Rough guess: 2–4 focused days including tests.
- Steps 3–5 are mostly mechanical, gated on 1–2 being right.
- Biggest risk: the sync-serialization change ripples through every
  `ContextTransmaterializable` impl and the derive macro — do it as its own PR
  with the derive tests as the safety net.
- Second risk: proxy lifetime management currently rides on ractor supervision;
  the core needs an explicit `EndpointDied(EndpointId)` event so adapters without
  supervision (embassy) can implement it with channel-closed detection.
- The wire protocol does not change at all — old and new implementations
  interoperate.

## 8. Alternative considered and rejected

Making ractor itself no_std (so the internals could stay): ractor is built on
tokio, dashmap, std sync primitives, and its global registry. That's an upstream
project of its own and would still leave MCU users paying for abstractions they
don't need. The sans-io split is less total work and yields a cleaner layering.
