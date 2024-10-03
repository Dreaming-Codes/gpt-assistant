{
  description = "Iced example with service";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = inputs@{ self, nixpkgs, flake-parts, flake-utils, ...}:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
        };
        runtimeDeps = with pkgs; [
          wayland libxkbcommon libGL xorg.libX11.dev xorg.libXi xorg.libXtst
        ];
        allBuildInputs = with pkgs; [
          pkg-config
        ] ++ runtimeDeps;
      in {
        devShells.default = pkgs.mkShell {
          buildInputs = allBuildInputs;

          shellHook = ''
            export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath allBuildInputs}
          '';
        };

        packages.default = pkgs.rustPlatform.buildRustPackage {
          pname = "gpt-overlay";
          version = "0.1.0"; # Adjust the version as needed

          src = ./.; # Assuming your Cargo.toml is in the same directory

          # Use the built-in fetcher for Cargo.lock dependencies
          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = runtimeDeps;
        };

        # NixOS module to define the service
        nixosModules.gptOverlay = { config, lib, pkgs, ... }: {
          options.gptOverlay = {
            enable = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Enable the gpt-overlay service.";
            };
          };

          config = lib.mkIf config.gptOverlay.enable {
            systemd.services.gptOverlay = {
              description = "GPT Overlay Service";
              after = [ "network.target" ];
              wantedBy = [ "multi-user.target" ];

              serviceConfig = {
                ExecStart = "${self.packages.${system}.default}/bin/gpt-overlay";
                Restart = "on-failure";
              };

              environment = {
                LD_LIBRARY_PATH = "${pkgs.lib.makeLibraryPath allBuildInputs}";
              };
            };
          };
        };
      }
    );
}
