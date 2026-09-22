{ pkgs, lib, inputs, ... }:

let
  # The repo's pinned toolchain. Also used to build the Stellar CLI below:
  # stellar-cli 22.6.0 depends on ethnum 1.5.0, which fails to compile on
  # current rustc (E0512), so nixpkgs' rustPlatform cannot build it.
  rustPkgs = import inputs.nixpkgs {
    inherit (pkgs.stdenv.hostPlatform) system;
    overlays = [ (import inputs.rust-overlay) ];
  };
  toolchain = rustPkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
  rustPlatform = pkgs.makeRustPlatform {
    cargo = toolchain;
    rustc = toolchain;
  };

  # `make build` and `make differential` need `stellar contract optimize`.
  # stellar-cli is not packaged in nixpkgs; build the pinned release from
  # crates.io. Version and `opt` feature match the ADR-0008 release evidence.
  stellar-cli = rustPlatform.buildRustPackage rec {
    pname = "stellar-cli";
    version = "22.6.0";

    src = pkgs.fetchCrate {
      inherit pname version;
      hash = "sha256-R+De07wD+R0lbooqA6ULqkxvUqE8gzlRQfKBdGfbnJ4=";
    };

    cargoHash = "sha256-GR8OU+ZV++WRqqWcJYz2RKqOLqYhhe+oVZokvgAgDZg=";

    buildFeatures = [ "opt" ];

    nativeBuildInputs = with pkgs; [ pkg-config cmake perl ];
    buildInputs = with pkgs; [ openssl systemd dbus ];

    # Upstream tests want a live network.
    doCheck = false;

    meta = {
      description = "Stellar CLI pinned for reproducible Soroban Wasm optimization";
      mainProgram = "stellar";
    };
  };
in
{
  packages = [ stellar-cli ];

  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };
}
