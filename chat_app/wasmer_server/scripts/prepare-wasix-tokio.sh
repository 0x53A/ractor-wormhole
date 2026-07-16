#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
checkout="$repo_root/.wasix-deps/tokio"
tokio_manifest="$checkout/tokio/Cargo.toml"
tokio_rev="f8a71f57d148771498551c0590ca8e9906a60d05"
libc_rev="4869d903f325632fd774f17841b895b68aa18297"
mio_rev="52c6243f3c0e3916a3320f25ca6d2bc66f952804"
socket2_rev="a2c803821c921735386bea3762ff1ea86f128891"

if [ ! -d "$checkout/.git" ]; then
  mkdir -p "$(dirname "$checkout")"
  git clone --no-checkout https://github.com/wasix-org/tokio.git "$checkout"
fi

if [ "$(git -C "$checkout" rev-parse HEAD 2>/dev/null || true)" != "$tokio_rev" ]; then
  git -C "$checkout" fetch --depth 1 origin "$tokio_rev"
  git -C "$checkout" checkout --detach "$tokio_rev"
else
  git -C "$checkout" reset --hard "$tokio_rev"
fi

perl -0pi -e 's/(\[package\][\s\S]*?^version = )"[^"]+"/$1"1.52.3"/m' "$tokio_manifest"
perl -0pi -e 's#git = "https://github\.com/wasix-org/libc\.git", branch = "wasix-0\.2\.169"#git = "https://github.com/wasix-org/libc.git", rev = "'"$libc_rev"'"#g' "$tokio_manifest"
perl -0pi -e 's#git = "https://github\.com/wasix-org/mio\.git", branch = "wasix-1\.0\.3"#git = "https://github.com/wasix-org/mio.git", rev = "'"$mio_rev"'"#g' "$tokio_manifest"
perl -0pi -e 's#git = "https://github\.com/wasix-org/socket2\.git", branch = "wasix-0\.6\.0"#git = "https://github.com/wasix-org/socket2.git", rev = "'"$socket2_rev"'"#g' "$tokio_manifest"
