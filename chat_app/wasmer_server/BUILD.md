# Building the Wasmer Edge server

1. Install the upstream Wasmer runtime:

   ```sh
   curl https://get.wasmer.io -sSfL | sh -s v7.2.0
   source ~/.wasmer/wasmer.sh
   ```

   The Nix `wasmer` package currently fails to run the `cargo-wasix` output
   locally with a missing `wasix_32v1.proc_exec4` import.

2. Enter the Nix shell for native linker tools and Trunk:

   ```sh
   nix-shell chat_app/wasmer_server/shell.nix
   ```

3. Install the older `cargo-wasix` helper used to fetch the Edge-compatible
   WASIX Rust 1.85 toolchain:

   ```sh
   cargo install cargo-wasix --version 0.1.24 --root .cargo-wasix-0.1.24
   ./.cargo-wasix-0.1.24/bin/cargo-wasix wasix download-toolchain v2025-03-17.2
   rustup toolchain install 1.85.0-x86_64-unknown-linux-gnu
   ```

   Current `cargo-wasix` builds run locally on Wasmer 7.2.0, but Wasmer Edge
   rejects the generated WASIX imports (`proc_exec4` / `thread_spawn_v2`) with
   a 500 workload failure. The 2025-03-17 Rust 1.85 WASIX toolchain emits the
   older import set accepted by Edge.

4. Prepare the gitignored Wasix Tokio checkout if the shell did not already do it:

   ```sh
   cd chat_app/wasmer_server
   ./scripts/prepare-wasix-tokio.sh
   ```

5. Build the browser client and Edge server package from this directory:

   ```sh
   cd chat_app/wasmer_server
   ./scripts/build-edge-wasm.sh
   ```

   The script runs `trunk build --release` for `chat_app/wasm_client`
   first, then embeds `wasm_client/dist` into the Wasmer server binary.

6. Deploy it from this directory:

   ```sh
   cd chat_app/wasmer_server
   wasmer deploy --owner 0x53a --publish-package --non-interactive
   ```

For local testing:

```sh
wasmer run target/wasm32-wasmer-wasi/release/wasmer_chat_server.wasm --net --env PORT=3000
```

The server handles websocket upgrades and serves the embedded chat app from
`/`. `/healthz` returns `OK`.
