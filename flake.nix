{
  description = "Narvi — real-time color-management suite for Hyprland";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        # M0 stub — fleshed out in M8 (package + AUR-equivalent build).
        packages.narvi = pkgs.rustPlatform.buildRustPackage {
          pname = "narvi";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = [ pkgs.libxkbcommon pkgs.wayland pkgs.gtk3 ];
        };
        packages.default = self.packages.${system}.narvi;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = [ pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.pkg-config ];
          buildInputs = [ pkgs.libxkbcommon pkgs.wayland pkgs.gtk3 ];
        };
      })
    # M8: // { homeManagerModules.narvi = import ./nix/hm-module.nix self; };
    ;
}
