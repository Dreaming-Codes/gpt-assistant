{
  description = "Iced example";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = inputs@{ self, nixpkgs, flake-parts, ...}:
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = nixpkgs.lib.systems.flakeExposed;
      perSystem = {self', pkgs, system, ...}:
        let
          pkgs = import nixpkgs {
            inherit system;
          };
          runtimeDeps = with pkgs; [
            wayland libxkbcommon libGL xorg.libX11.dev xorg.libXi xorg.libXtst
          ];
        in {
          devShells.default = pkgs.mkShell rec {
            buildInputs = with pkgs; [
              pkg-config
            ] ++ runtimeDeps;
            LD_LIBRARY_PATH = "${nixpkgs.lib.makeLibraryPath buildInputs}";
          };
        };
    };
}
