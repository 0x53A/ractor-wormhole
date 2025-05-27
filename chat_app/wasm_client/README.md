## Usage

Despite the name, this is an egui based client, and it can be run natively or as a web app.

### native

```
cargo run -- --url https://ractor-wormhole-chat-app-server.onrender.com
```

### web

```
# here
trunk build

# cd ../wasm_server
cargo run
```