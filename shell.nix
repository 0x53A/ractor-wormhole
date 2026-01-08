{ pkgs ? import <nixpkgs> {} }:

let
  libPath = with pkgs; lib.makeLibraryPath [
    openssl
  ];
in
pkgs.mkShell {
  buildInputs = with pkgs; [
    pkg-config
    openssl
  ];
  
  LD_LIBRARY_PATH = libPath;
}
