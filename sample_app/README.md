sample app to demonstrate ractor-wormhole functionality.

Start two instances by:

```sh
cargo run -- server --bind 0.0.0.0:8085
cargo run -- client --url ws://localhost:8085
```

