{
  description = "CADCraft — open-source pure-Rust CAD";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        lib = pkgs.lib;

        # No rust-toolchain file in the repo; workspace needs >= 1.90 (ui-egui 1.95).
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "clippy" "rustfmt" "rust-analyzer" ];
          targets = [ "wasm32-unknown-unknown" ]; # for `cargo xtask wasm`
        };
        rustPlatform = pkgs.makeRustPlatform {
          cargo = rustToolchain;
          rustc = rustToolchain;
        };

        # Everything winit/wgpu/glutin/rfd dlopen at runtime.
        runtimeLibs = with pkgs; [
          vulkan-loader # libvulkan.so.1 (wgpu)
          libglvnd # libEGL.so.1 / libGL.so.1 (GL fallback)
          libxkbcommon # libxkbcommon(.x11).so
          wayland # libwayland-client / -cursor
          xorg.libX11
          xorg.libxcb
          xorg.libXcursor
          xorg.libXi
          xorg.libXrandr
          dbus # libdbus-1.so.3 (rfd portal)
        ];
        runtimeBins = with pkgs; [ zenity xdg-utils ];
      in
      {
        packages.default = rustPlatform.buildRustPackage {
          pname = "cadcraft";
          version = "0.5.0";
          src = lib.cleanSource ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
            # Only git dependency in Cargo.lock. Key is "<name>-<version>".
            outputHashes = {
              "craft-ui-0.1.0" = "sha256-VhLeT7VLdZAIz6gR9PdFI34IJxHsGF8NE0uzFCN2vIU=";
            };
          };

          cargoBuildFlags = [ "-p" "cadcraft" "-p" "cadcraft-cli" ];
          doCheck = false; # run `cargo xtask ci` in the dev shell; tests may want a GPU

          nativeBuildInputs = [ pkgs.makeWrapper ];

          postFixup = ''
            wrapProgram $out/bin/cadcraft \
              --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs} \
              --prefix PATH : ${lib.makeBinPath runtimeBins}
          '';

          meta.mainProgram = "cadcraft";
        };

        apps.default = flake-utils.lib.mkApp { drv = self.packages.${system}.default; };

        devShells.default = pkgs.mkShell {
          packages = [ rustToolchain pkgs.git ] ++ runtimeBins;
          # dlopen targets must be findable by `cargo run`
          LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibs;
          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
          # Optional: point at a storytold/craft-fonts checkout
          # CRAFT_FONTS_DIR = "/home/you/Projects/craft-fonts";
        };
      });
}
