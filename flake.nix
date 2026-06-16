{
  description = "snproxy + sncli — ServiceNow proxy and CLI via SN Utils WebSocket";

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
    )

    //

    {
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.services.snproxy;
          # Resolve the package for the system this module is evaluated on.
          # buildRustPackage builds the entire workspace, so this derivation
          # contains both snproxy (daemon) and sncli (CLI client).
          package = self.packages.${pkgs.system}.default;
        in
        {
          options.services.snproxy = {
            enable = lib.mkEnableOption "snproxy ServiceNow WebSocket proxy (also installs sncli into PATH)";

            host = lib.mkOption {
              type    = lib.types.str;
              default = "127.0.0.1";
              description = lib.mdDoc "Bind address for both the WebSocket and HTTP servers.";
            };

            wsPort = lib.mkOption {
              type    = lib.types.port;
              default = 1978;
              description = lib.mdDoc ''
                WebSocket port. SN Utils Helper Tab always connects to 1978 — only
                change this if you know what you are doing.
              '';
            };

            port = lib.mkOption {
              type    = lib.types.port;
              default = 8766;
              description = lib.mdDoc "HTTP REST API port.";
            };

            timeout = lib.mkOption {
              type    = lib.types.ints.positive;
              default = 30;
              description = lib.mdDoc "Seconds to wait for a Helper Tab response before returning 504.";
            };

            openFirewall = lib.mkOption {
              type    = lib.types.bool;
              default = false;
              description = lib.mdDoc ''
                Open the WebSocket and HTTP ports in the firewall.
                Not needed when binding to 127.0.0.1 (the default).
              '';
            };
          };

          config = lib.mkIf cfg.enable {
            # Expose both snproxy and sncli in the system PATH so any user or
            # script on the host can talk to the running proxy without extra setup.
            environment.systemPackages = [ package ];

            systemd.services.snproxy = {
              description = "snproxy — ServiceNow REST proxy via SN Utils WebSocket";
              wantedBy    = [ "multi-user.target" ];
              after       = [ "network.target" ];

              serviceConfig = {
                ExecStart = lib.escapeShellArgs [
                  "${package}/bin/snproxy"
                  "--host"     cfg.host
                  "--ws-port"  (toString cfg.wsPort)
                  "--port"     (toString cfg.port)
                  "--timeout"  (toString cfg.timeout)
                ];

                Restart    = "on-failure";
                RestartSec = "5s";

                # Run as an unprivileged transient user.
                DynamicUser = true;

                # Hardening
                NoNewPrivileges  = true;
                ProtectSystem    = "strict";
                ProtectHome      = true;
                PrivateTmp       = true;
                PrivateDevices   = true;
                ProtectKernelTunables = true;
                ProtectControlGroups  = true;
                RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
                RestrictNamespaces = true;
                LockPersonality    = true;
                MemoryDenyWriteExecute = true;
              };
            };

            networking.firewall = lib.mkIf cfg.openFirewall {
              allowedTCPPorts = [ cfg.wsPort cfg.port ];
            };
          };
        };
    };
}
