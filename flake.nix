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
        # dlopen'd at runtime by winit/wgpu; patched into the GUI's rpath.
        guiLibs = with pkgs; [
          wayland
          libxkbcommon
          vulkan-loader
          xorg.libX11
          xorg.libXcursor
          xorg.libXi
        ];
      in
      {
        packages.narvi = pkgs.rustPlatform.buildRustPackage {
          pname = "narvi";
          version = "0.1.0";
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          buildInputs = guiLibs;
          postFixup = ''
            patchelf --add-rpath ${pkgs.lib.makeLibraryPath guiLibs} $out/bin/narvi-gui
          '';
          meta = {
            description = "Real-time color management for Hyprland";
            homepage = "https://github.com/zer0dot/narvi";
            license = pkgs.lib.licenses.mit;
            mainProgram = "narvi";
          };
        };
        packages.default = self.packages.${system}.narvi;

        devShells.default = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [ cargo rustc rustfmt clippy pkg-config ];
          buildInputs = guiLibs;
          # cargo-built (unwrapped) GUI needs these at runtime on NixOS.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath guiLibs;
        };
      })
    // {
      homeManagerModules.narvi = import ./nix/hm-module.nix self;
      homeManagerModules.default = self.homeManagerModules.narvi;
    };
}
