sample app to demonstrate ractor-wormhole functionality.

Start two instances by:

```sh
# websocket transport
cargo run --features websocket -- server --bind 0.0.0.0:8085
cargo run --features websocket -- client --url ws://localhost:8085


# or unix sockets
cargo run --features unix_socket -- server --socket /tmp/ractor_wormhole_sample_app
cargo run --features unix_socket -- client --socket /tmp/ractor_wormhole_sample_app
```


---

Note: to test SSH:

```sh

# build the server with unix sockets
cargo build --features unix_socket

# (copy it to the remote pc)
scp /home/lukas/src/ractor-wormhole/target/release/ractor_wormhole_sample_app lukas@nixos-server:/home/lukas/

# start the server on the remote pc
./ractor_wormhole_sample_app server --socket /tmp/ractor_wormhole_sample_app

# build a client with ssh and connect
cargo run --features ssh -- client --ssh "lukas@nixos-server/tmp/ractor_wormhole_sample_app" --ssh-key /home/lukas/.ssh/id_ed25519
```