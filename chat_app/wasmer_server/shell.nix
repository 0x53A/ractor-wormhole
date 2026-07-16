{ pkgs ? import <nixpkgs> {} }:

let
  libPath = with pkgs; lib.makeLibraryPath [
    openssl
  ];
in
pkgs.mkShell {
  packages = with pkgs; [
    binutils
    gcc
    git
    lld
    openssl
    perl
    pkg-config
    trunk
  ];

  LD_LIBRARY_PATH = libPath;

  shellHook = ''
    export PATH="$HOME/.wasmer/bin:$HOME/.cargo/bin:$PATH"

    if ! command -v cargo-wasix >/dev/null 2>&1; then
      echo "cargo-wasix is not installed. Run: cargo install cargo-wasix"
    fi

    if ! command -v wasmer >/dev/null 2>&1; then
      echo "wasmer is not installed. Run: curl https://get.wasmer.io -sSfL | sh -s v7.2.0"
    else
      case "$(command -v wasmer)" in
        /nix/store/*)
          echo "warning: using Nix wasmer at $(command -v wasmer)"
          echo "warning: the Nix build has failed to provide the WASIX proc_exec4 import locally."
          echo "warning: install upstream Wasmer with: curl https://get.wasmer.io -sSfL | sh -s v7.2.0"
          ;;
      esac
    fi

    if ! command -v trunk >/dev/null 2>&1; then
      echo "trunk is not installed. Run: cargo install --locked trunk"
    fi

    ./scripts/prepare-wasix-tokio.sh
  '';
}
