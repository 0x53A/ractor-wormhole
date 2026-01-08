sample app to demonstrate ractor-wormhole functionality.

Start two instances by:

```sh
# websocket transport
cargo run --no-default-features --features websocket -- server --bind 0.0.0.0:8085
cargo run --no-default-features --features websocket -- client --url ws://localhost:8085


# or unix sockets
cargo run --no-default-features --features unix_socket -- server --socket ractor_wormhole_sample_app
cargo run --no-default-features --features unix_socket -- client --socket ractor_wormhole_sample_app
```

