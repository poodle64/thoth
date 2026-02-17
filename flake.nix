{
  description = "Thoth - Privacy-first, offline-capable voice transcription application";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfreePredicate = pkg: builtins.elem (pkgs.lib.getName pkg) [
              "pnpm"
            ];
            allowBroken = true;  # webkitgtk for Tauri on Linux
          };
        };

        # Rust toolchain with Tauri prerequisites
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

      in {
        devShells.default = pkgs.mkShell {
          # Platform-specific library paths (Linux)
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux
            (pkgs.lib.makeLibraryPath [
              pkgs.libappindicator-gtk3
              pkgs.vulkan-loader
            ]);

          # Workaround for webkit2gtk Wayland issues (Linux only)
          # See: https://github.com/tauri-apps/tauri/issues/9460
          WEBKIT_DISABLE_COMPOSITING_MODE = pkgs.lib.optionalString pkgs.stdenv.isLinux "1";

          # libclang for whisper.cpp bindgen
          LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages.libclang ];

          packages = with pkgs; [
            # Rust / Tauri
            rustToolchain
            cargo
            rustc
            rust-analyzer

            # Tauri dependencies (platform-specific)
            openssl
            pkg-config
          ] ++ lib.optionals stdenv.isLinux [
            # Linux-only Tauri dependencies
            webkitgtk_4_1
            libappindicator-gtk3
            librsvg
            alsa-lib
            # whisper.cpp needs libclang for bindgen
            llvmPackages.libclang
            # X11 development libraries for x11rb (mouse tracking, display detection)
            libx11
            libxcursor
            libxrandr
            libxi
            # Vulkan for whisper.cpp GPU acceleration (AMD & NVIDIA)
            vulkan-loader
            vulkan-headers
            vulkan-tools
            # Shader compiler for Vulkan
            shaderc
          ] ++ lib.optionals stdenv.isDarwin [
            # macOS: applesoft libraries (via Xcode) are used automatically
            libiconv
            # libclang for bindgen (whisper.cpp)
            llvmPackages.libclang
          ] ++ [
            # Frontend
            nodejs_22
            pnpm

            # Build tools
            cmake

            # Useful utilities (Linux-only)
          ] ++ lib.optionals stdenv.isLinux [
            glib
            libsecret
          ];

          shellHook = ''
            echo "𓅝 Thoth Development Environment"
            echo "================================"
            echo "  Rust: $(rustc --version)"
            echo "  Node: $(node --version)"
            echo "  pnpm: $(pnpm --version)"
            echo ""
            echo "Commands:"
            echo "  pnpm install        - Install dependencies"
            echo "  pnpm tauri dev      - Start development build"
            echo "  pnpm tauri build    - Build for production"
            echo "  cargo test          - Run Rust tests (from src-tauri/)"
          '';
        };
      });
}
