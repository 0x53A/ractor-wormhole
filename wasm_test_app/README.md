> [!NOTE]  
> This folder was fully LLM generated


# WASM Thread-Local Actor Test App

This is a browser-based test application for the `ThreadLocalFnActor` utility in `ractor_wormhole`.

## Purpose

The `ThreadLocalFnActor` is designed to work with non-`Send` types by keeping everything on a single thread. This is particularly useful in WASM environments where:
- JavaScript interop often involves non-`Send` types
- `Rc` (instead of `Arc`) is preferred for single-threaded performance
- Browser APIs are single-threaded

## Running the Tests

### Prerequisites

Install Trunk (if you haven't already):
```bash
cargo install trunk
```

Install the WASM target:
```bash
rustup target add wasm32-unknown-unknown
```

### Run the App

From this directory:
```bash
trunk serve
```

This will:
1. Build the WASM module
2. Start a local web server (default: http://127.0.0.1:8080)
3. Open your browser automatically
4. Watch for changes and rebuild automatically

### Running the Tests

Once the app loads in your browser:
1. Click the **"Run All Tests"** button
2. Watch the test output appear in the log area
3. Check the status indicator for overall results

## Tests Included

1. **Basic Message Passing** - Verifies that messages can be sent and received
2. **Multiple Actors** - Tests that multiple actors can run independently
3. **Non-Send Types** - The key test! Uses `Rc<String>` which is NOT `Send`
4. **High Throughput** - Sends 1000 messages to verify performance

## What Makes This Special

The third test (`test_non_send_types`) demonstrates the real value of thread-local actors:

```rust
struct NonSendData {
    data: std::rc::Rc<String>,  // Rc is NOT Send!
}
```

This would fail with a regular `FnActor` but works perfectly with `ThreadLocalFnActor` because everything stays on the same thread.

## Building for Production

To build an optimized release version:
```bash
trunk build --release
```

The output will be in the `dist/` directory.
