{
  description = "snproxy + sncli — ServiceNow proxy and CLI via SN Utils WebSocket";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, flake-utils, crane }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
        craneLib = crane.mkLib pkgs;

        # Source filtered to Rust-relevant files only — flake.nix / README edits
        # don't invalidate the dependency cache.
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
        };

        # Build all workspace dependencies once and cache the result.
        # Every per-crate derivation below inherits this artifact store, so
        # dep compilation is not repeated across crates or rebuilds.
        cargoArtifacts = craneLib.buildDepsOnly commonArgs;

        mkPackage = pname: craneLib.buildPackage (commonArgs // {
          inherit cargoArtifacts;
          pname    = pname;
          version  = "0.1.0";
          cargoExtraArgs = "-p ${pname}";
        });

        snproxy = mkPackage "snproxy";
        sncli   = mkPackage "sncli";
        snstate = mkPackage "snstate";
        sntui   = mkPackage "sntui";
      in
      {
        packages.snproxy = snproxy;
        packages.sncli   = sncli;
        packages.snstate = snstate;
        packages.sntui   = sntui;
        packages.default = snproxy;

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
            echo "  nix build .#snproxy   # reproducible build"
            echo "  nix build .#sncli"
            echo "  nix build .#snstate"
            echo "  nix build .#sntui"
          '';
        };
      }
    )

    //

    {
      nixosModules.default = { config, lib, pkgs, ... }:
        let
          cfg     = config.services.snproxy;
          snproxy = self.packages.${pkgs.system}.snproxy;
          sncli   = self.packages.${pkgs.system}.sncli;
          snstate = self.packages.${pkgs.system}.snstate;
          sntui   = self.packages.${pkgs.system}.sntui;
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
            environment.systemPackages = [ snproxy sncli snstate sntui ];

            systemd.services.snproxy = {
              description = "snproxy — ServiceNow REST proxy via SN Utils WebSocket";
              wantedBy    = [ "multi-user.target" ];
              after       = [ "network.target" ];

              serviceConfig = {
                ExecStart = lib.escapeShellArgs [
                  "${snproxy}/bin/snproxy"
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
