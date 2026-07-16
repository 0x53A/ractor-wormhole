* decouple the crate from ractor (sans-io core, no_std for MCUs, ractor becomes an adapter) — see [docs/ractor-independence-analysis.md](docs/ractor-independence-analysis.md)

* optionally use a string name for the actor ref in the ``RemoteActorId`` instead of the uuid. This would allow reconnecting to an Actor after a connection is broken and re-established.

* ~~need to notify the remote side if an exposed actor dies?~~ done: ``CrossPortalMessage::ActorExited``, proxy actors are stopped when the real actor exits (tested in ``remote_linking.rs``)

* implement per-connection limits (rate limiting, max number of rpc-ports, max number of projected actors.)

* add a more generic ``SerializationAdapter`` option to the derive macro

* ~~make it possible to publish actors on the **Nexus**, not the Portal, so they are immediately available to all clients~~ done: ``nexus::Nexus::publish_named_actor`` (tested in ``nexus_publish.rs``)

* ~~two TODOs in derive_tests (and add more test cases)~~ done: uninhabited enums are now special-cased in the derive macro, ``allow(unreachable_code)`` removed. (more test cases still welcome)

* ~~rename NexusResult => ???~~ done: ``WormholeResult``, moved to lib.rs as the crate-level result type

* ~~FunctionActors: stop actor when async fn returns~~ done, all ``start_fn*`` variants

* ~~FunctionActors: add ThreadLocal variant~~ done: ``thread_local_function_actor.rs``



## Ractor

```
tokio-rustls = { version = "0.26", default-features = false, features = ["ring"] }
```

```
#![feature(try_trait_v2)]
impl<T> Try for CallResult<T> {
```