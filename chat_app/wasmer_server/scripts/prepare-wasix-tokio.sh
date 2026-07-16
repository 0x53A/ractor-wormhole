#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
checkout="$repo_root/.wasix-deps/tokio"
tokio_manifest="$checkout/tokio/Cargo.toml"

if [ ! -d "$checkout/.git" ]; then
  mkdir -p "$(dirname "$checkout")"
  git clone --depth 1 --branch wasix-1.47.0 https://github.com/wasix-org/tokio.git "$checkout"
fi

current_version="$(perl -ne 'if (/^version = "([^"]+)"/) { print $1; exit }' "$tokio_manifest")"

if [ "$current_version" != "1.52.3" ]; then
  perl -0pi -e 's/(\[package\][\s\S]*?^version = )"[^"]+"/$1"1.52.3"/m' "$tokio_manifest"
fi
