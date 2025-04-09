For the server:

```
cargo run -p server -- --bind 0.0.0.0:8085
```

---------------------------------------------------

For the client:

```
cargo run -p client -- --url ws://localhost:8085
```

----------------

An instance of the server is also hosted at render.com.

![alt text](./_md_content/image.png)

You may connect to it via:

```
cargo run -p client -- --url wss://ractor-wormhole-chat-app-server.onrender.com
```