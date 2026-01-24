#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UNIFFI_DIR="$(dirname "$SCRIPT_DIR")"
ROOT_DIR="$(dirname "$UNIFFI_DIR")"

echo "=== Building Rust library ==="
cd "$ROOT_DIR"
cargo build -p uniffi_test

echo ""
echo "=== Generating Kotlin bindings ==="
cargo run -p uniffi_test --bin uniffi-bindgen -- \
    generate --library "$ROOT_DIR/target/debug/libuniffi_test.so" \
    --language kotlin --out-dir "$SCRIPT_DIR/src/main/kotlin"

echo ""
echo "=== Building Kotlin client ==="
cd "$SCRIPT_DIR"

# Check if gradle wrapper exists, if not use system gradle
if [ -f "./gradlew" ]; then
    ./gradlew build --quiet
else
    gradle wrapper --quiet 2>/dev/null || true
    if [ -f "./gradlew" ]; then
        ./gradlew build --quiet
    else
        gradle build --quiet
    fi
fi

echo ""
echo "=== Running Kotlin client ==="
echo "Usage: $0 [server_url]"
echo "Default: ws://127.0.0.1:8080"
echo ""

SERVER_URL="${1:-ws://127.0.0.1:8080}"

# Set library path and run
export LD_LIBRARY_PATH="$ROOT_DIR/target/debug:$LD_LIBRARY_PATH"

if [ -f "./gradlew" ]; then
    ./gradlew run --args="$SERVER_URL" --quiet
else
    gradle run --args="$SERVER_URL" --quiet
fi
