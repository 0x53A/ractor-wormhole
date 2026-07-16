#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/prepare-wasix-tokio.sh

if ! command -v trunk >/dev/null 2>&1; then
  echo "trunk is required to build the embedded wasm client." >&2
  echo "Install it with: cargo install --locked trunk" >&2
  exit 1
fi

(
  cd ../wasm_client
  env -u NO_COLOR trunk build --release --public-url /
  printf '*\n!.gitignore\n' > dist/.gitignore
)

crate_version="$(perl -ne 'if (/^version = "([^"]+)"/) { print $1; exit }' Cargo.toml)"
commit_count="$(git rev-list --count HEAD)"
package_version="${crate_version}-${commit_count}"
perl -0pi -e 's/(\[package\][\s\S]*?^version = )"[^"]+"/$1"'"$package_version"'"/m' wasmer.toml

if ! rustc +wasix --version | grep -q '1\.90\.0-dev'; then
  cat >&2 <<'EOF'
The active rustup toolchain named "wasix" is not the Edge-compatible Rust 1.90 toolchain.

Install it with:

  cargo install cargo-wasix --version 0.1.31 --root .cargo-wasix-0.1.31
  ./.cargo-wasix-0.1.31/bin/cargo-wasix wasix download-toolchain 'v2026-06-09.1+rust-1.90'
  rustup toolchain install 1.90.0-x86_64-unknown-linux-gnu

Then rerun this script.
EOF
  exit 1
fi

wasix_sysroot="$(rustc +wasix --print sysroot)"
target_dir="${CARGO_TARGET_DIR:-target-edge-compatible}"
host_linker="$(command -v clang || command -v cc)"

RUSTC="$wasix_sysroot/bin/rustc" \
RUSTFLAGS='--cfg getrandom_backend="custom" -Aunexpected_cfgs' \
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$host_linker" \
CARGO_TARGET_DIR="$target_dir" \
cargo +1.90.0 build --target wasm32-wasmer-wasi --release

mkdir -p target/wasm32-wasmer-wasi/release
cp "$target_dir/wasm32-wasmer-wasi/release/wasmer_chat_server.wasm" \
  target/wasm32-wasmer-wasi/release/wasmer_chat_server.wasm

if wasmer inspect target/wasm32-wasmer-wasi/release/wasmer_chat_server.wasm \
  | grep -E 'proc_exec4|thread_spawn'; then
  echo "Built artifact imports Edge-incompatible WASIX symbols." >&2
  exit 1
fi
