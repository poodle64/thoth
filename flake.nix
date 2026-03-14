{
  description = "Thoth - Privacy-first, offline-capable voice transcription application";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay, ... }:
    (flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfree = true;  # Required for CUDA packages
            allowBroken = true;  # webkitgtk for Tauri on Linux
            cudaSupport = true;
          };
        };

        # Rust toolchain with Tauri prerequisites
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # CUDA packages for whisper.cpp GPU acceleration
        cudaPackages = pkgs.cudaPackages_12;

      in {
        # =====================================================================
        # Nix package — installable via `nix build` or as a flake input
        # =====================================================================
        packages.default = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
          pname = "thoth";
          version = "2026.2.7";
          src = ./.;

          cargoRoot = "src-tauri";
          buildAndTestSubdir = "src-tauri";
          cargoHash = "sha256-bWRmk1cvPxPqnzhB6KnW586Xhlqd4z6vFUOn/ujsQJk=";

          # Disable default features to exclude fluidaudio (macOS-only git dep
          # that fails in Nix sandbox) and parakeet (download-binaries feature)
          cargoBuildNoDefaultFeatures = true;
          cargoBuildFeatures = [ "cuda" ];

          # Pre-fetched pnpm dependencies for the Svelte frontend
          pnpmDeps = pkgs.fetchPnpmDeps {
            inherit (finalAttrs) pname version src;
            fetcherVersion = 3;
            hash = "sha256-2/TWfSxppLkweslAwlWLRjHJQd44x20FKraY9o5HCGI=";
          };

          nativeBuildInputs = with pkgs; [
            cargo-tauri.hook    # Replaces cargoBuildHook/cargoInstallHook
            nodejs
            pnpmConfigHook      # Sets up pre-fetched pnpm deps
            pnpm
            pkg-config
            cmake               # Required by whisper.cpp (whisper-rs dependency)
            git                 # Required by whisper.cpp CMakeLists.txt
            llvmPackages.libclang  # Required by bindgen for whisper.cpp
            wrapGAppsHook4      # GTK/GLib schema wrapping
            makeWrapper         # For wrapping runtime PATH deps
            cudaPackages.cuda_nvcc
            gcc                 # Required for CUDA compilation
          ];

          buildInputs = with pkgs; [
            openssl
            webkitgtk_4_1
            glib
            glib-networking     # HTTPS support in webkitgtk
            libsecret           # Credential storage
            libappindicator-gtk3
            alsa-lib
            librsvg
            libx11
            libxcursor
            libxrandr
            libxi
            vulkan-loader
            vulkan-headers
            shaderc
            cudaPackages.cudatoolkit
            cudaPackages.cuda_cudart
            cudaPackages.cuda_cccl
            cudaPackages.libcublas
          ];

          env = {
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            WEBKIT_DISABLE_COMPOSITING_MODE = "1";
            CUDA_PATH = "${cudaPackages.cudatoolkit}";
            RUSTFLAGS = "-C relocation-model=dynamic-no-pic -L /run/opengl-driver/lib";
          };

          postFixup = ''
            wrapProgram $out/bin/thoth \
              --prefix PATH : ${pkgs.lib.makeBinPath [
                pkgs.wl-clipboard  # wl-copy, wl-paste
                pkgs.wtype          # Wayland keyboard simulation
              ]} \
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath [
                pkgs.libappindicator-gtk3
                pkgs.vulkan-loader
              ]}
          '';

          # Disable updater artifact signing (no private key in nix sandbox)
          preBuild = ''
            substituteInPlace src-tauri/tauri.conf.json \
              --replace-fail '"createUpdaterArtifacts": true' '"createUpdaterArtifacts": false'
          '';

          doCheck = false; # Tests need audio hardware

          meta = with pkgs.lib; {
            description = "Privacy-first, offline-capable voice transcription";
            homepage = "https://github.com/kirin-ri/thoth";
            license = licenses.mit;
            platforms = platforms.linux;
            mainProgram = "thoth";
          };
        });

        # =====================================================================
        # Development shell — unchanged from original
        # =====================================================================
        devShells.default = pkgs.mkShell {
          # Platform-specific library paths (Linux)
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux
            (pkgs.lib.makeLibraryPath ([
              pkgs.libappindicator-gtk3
              pkgs.vulkan-loader
            ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              # CUDA runtime libraries for whisper.cpp linking
              cudaPackages.cuda_cudart
              cudaPackages.cuda_cccl
              cudaPackages.libcublas
            ]) + ":/run/opengl-driver/lib");  # NVIDIA driver (libcuda.so)

          # Workaround for webkit2gtk Wayland issues (Linux only)
          # See: https://github.com/tauri-apps/tauri/issues/9460
          WEBKIT_DISABLE_COMPOSITING_MODE = pkgs.lib.optionalString pkgs.stdenv.isLinux "1";

          # libclang for whisper.cpp bindgen
          LIBCLANG_PATH = pkgs.lib.makeLibraryPath [ pkgs.llvmPackages.libclang ];

          # CUDA environment variables for whisper.cpp
          CUDA_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux "${cudaPackages.cudatoolkit}";
          CUDA_HOME = pkgs.lib.optionalString pkgs.stdenv.isLinux "${cudaPackages.cudatoolkit}";

          # Linker search path for CUDA driver (libcuda.so)
          # NOTE: -C relocation-model=dynamic-no-pic is REQUIRED for sherpa-rs static linking on Linux
          # See: https://github.com/thewh1teagle/sherpa-rs/issues/62
          RUSTFLAGS = pkgs.lib.optionalString pkgs.stdenv.isLinux "-C relocation-model=dynamic-no-pic -L /run/opengl-driver/lib";

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
            # CUDA toolkit for whisper.cpp CUDA acceleration (NVIDIA GPUs)
            cudaPackages.cudatoolkit
            cudaPackages.cuda_nvcc
            cudaPackages.cuda_cudart
            cudaPackages.cuda_cccl
            cudaPackages.libcublas
            # GCC for CUDA compilation
            gcc
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
            # Native Wayland keyboard simulation (alternative to X11-based enigo)
            wtype
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
            echo "  pnpm tauri dev -- --features cuda    - Dev with CUDA GPU acceleration"
            echo "  pnpm tauri build -- --features cuda  - Build with CUDA"
            echo "  cargo test          - Run Rust tests (from src-tauri/)"
            echo ""
            echo "GPU Acceleration (Linux):"
            echo "  --features cuda     - NVIDIA GPUs (requires CUDA drivers)"
            echo "  --features hipblas  - AMD GPUs (requires ROCm)"
            echo "  --features vulkan   - Cross-platform (experimental)"
          '';
        };
      }
    )) // {
      # Overlay — defined outside eachDefaultSystem (overlays are system-independent)
      overlays.default = final: prev: {
        thoth = self.packages.${final.system}.default;
      };
    };
}
