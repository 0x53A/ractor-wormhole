* optionally use a string name for the actor ref in the ``RemoteActorId`` instead of the uuid. This would allow reconnecting to an Actor after a connection is broken and re-established.

* need to notify the remote side if an exposed actor dies?

* implement per-connection limits (rate limiting, max number of rpc-ports, max number of projected actors.)

* add a more generic ``SerializationAdapter`` option to the derive macro

* make it possible to publish actors on the **Nexus**, not the Portal, so they are immediately available to all clients

* two TODOs in derive_tests (and add more test cases)

* rename NexusResult => ???

* FunctionActors: stop actor when async fn returns

* FunctionActors: add ThreadLocal variant



## Ractor

```
tokio-rustls = { version = "0.26", default-features = false, features = ["ring"] }
```

```
#![feature(try_trait_v2)]
impl<T> Try for CallResult<T> {
```