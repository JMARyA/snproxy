{
  description = "snproxy — ServiceNow REST API proxy via SN Utils WebSocket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "snproxy";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustc
            cargo
            clippy
            rust-analyzer
            # testing / manual inspection
            websocat
            curl
            jq
          ];

          shellHook = ''
            echo "snproxy dev shell"
            echo "  cargo build --release"
            echo "  cargo check"
            echo ""
            echo "  nix build          # reproducible build"
            echo "  ./result/bin/snproxy --help"
          '';
        };
      }
    );
}
