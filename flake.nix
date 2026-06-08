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
            allowUnfree = true;  # Required for CUDA packages
            allowBroken = true;  # webkitgtk for Tauri on Linux
            cudaSupport = true;
          };
        };

        # Rust toolchain with Tauri prerequisites
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        # CUDA packages for whisper.cpp GPU acceleration (dev shell only)
        cudaPackages = pkgs.cudaPackages_12;

        # A SECOND nixpkgs for the buildable package, WITHOUT cudaSupport.
        # `cudaSupport = true` forces onnxruntime, sherpa-onnx, cuDNN and
        # whisper.cpp to be compiled from source (the CUDA variants aren't in the
        # binary cache) — that's a multi-hour, >32 GB-RAM build that can lock the
        # machine. Without it, onnxruntime/sherpa-onnx come prebuilt from the
        # cache and whisper is GPU-accelerated via Vulkan instead — far lighter,
        # and Vulkan works on the same NVIDIA/AMD hardware.
        pkgsPkg = import nixpkgs {
          inherit system overlays;
          config = {
            allowUnfree = true;
            allowBroken = true;
          };
        };

        # ---------------------------------------------------------------------
        # Buildable package — `nix build` / importable as
        #   inputs.thoth.packages.${system}.default
        #
        # Engines in the packaged app:
        #   • Whisper (whisper.cpp) with Vulkan — GPU accelerated, cache-friendly
        #   • Parakeet (Sherpa-ONNX) on CPU — linked against nixpkgs' sherpa-onnx
        #
        # Parakeet normally pulls its native libraries via sherpa-rs's
        # `download-binaries` feature, which needs network access at build time —
        # impossible in the sealed Nix sandbox. Instead we build with the
        # `parakeet` feature only (no download) and point SHERPA_LIB_PATH at
        # nixpkgs' prebuilt `sherpa-onnx`; sherpa-rs-sys then skips its own build
        # and links `${sherpa-onnx}/lib/libsherpa-onnx-c-api.so`. That build is
        # CPU-only, which matches Thoth's current Parakeet behaviour on Linux.
        #
        # ── First build / updating dependencies ──────────────────────────────
        # `pnpmDeps.hash` and `cargoHash` are content hashes of the locked
        # dependency sets and MUST be refreshed whenever pnpm-lock.yaml or
        # Cargo.lock change. To (re)compute them:
        #   1. Leave both set to lib.fakeHash (as below).
        #   2. Run `nix build`. It fails reporting the correct pnpmDeps hash —
        #      paste it into `pnpmDeps.hash`.
        #   3. Run `nix build` again. It fails reporting the correct cargoHash —
        #      paste it into `cargoHash`.
        #   4. Run `nix build` once more; it now compiles through.
        # (sherpa-onnx version skew: nixpkgs ships a newer sherpa-onnx than the
        # crate's vendored headers. The C API is stable, but if linking fails on
        # a missing symbol, pin nixpkgs or bump sherpa-rs.)
        # ---------------------------------------------------------------------
        # Build Thoth against a given nixpkgs with a chosen whisper GPU backend.
        #   gpu = "vulkan" → GPU via Vulkan; deps come prebuilt from the cache
        #                    (light build — only Thoth itself compiles).
        #   gpu = "cuda"   → GPU via CUDA; pass `cuda'` built with cudaSupport so
        #                    onnxruntime/sherpa-onnx/whisper compile their CUDA
        #                    variants from source (HEAVY: hours + lots of RAM —
        #                    build with `--cores N -j1` to avoid memory spikes).
        # Both variants share the same cargo/pnpm dependency hashes (they don't
        # depend on the GPU backend).
        mkThoth = { pkgs', gpu }:
          let
            isCuda = gpu == "cuda";
            cuda12 = pkgs'.cudaPackages_12;
          in
          pkgs'.rustPlatform.buildRustPackage (finalAttrs: {
            pname = "thoth";
            version = "2026.6.3";
            src = ./.;

            cargoRoot = "src-tauri";
            buildAndTestSubdir = "src-tauri";
            cargoHash = "sha256-y7kVQ4t2Iko2MXV5EWmyEiQdYxdiNSsI3DOFYF4/el4=";

            # Parakeet (system sherpa-onnx via SHERPA_LIB_PATH) + Whisper (GPU).
            # No default features: excludes `parakeet-bundled` (network download)
            # and `fluidaudio` (macOS-only git dep that can't build in the sandbox).
            buildNoDefaultFeatures = true;
            buildFeatures = [ "parakeet" gpu ];

            # Pre-fetched pnpm dependencies for the Svelte frontend
            pnpmDeps = pkgs'.fetchPnpmDeps {
              inherit (finalAttrs) pname version src;
              fetcherVersion = 3;
              hash = "sha256-VJf+nQuNQHAknwIjs9WuDl2IdFJTifsGQ2/qQL1MUfE=";
            };

            nativeBuildInputs = with pkgs'; [
              cargo-tauri.hook      # Replaces cargoBuildHook/cargoInstallHook
              nodejs
              pnpmConfigHook        # Sets up pre-fetched pnpm deps
              pnpm
              pkg-config
              cmake                 # Required by whisper.cpp (whisper-rs dependency)
              git                   # Required by whisper.cpp CMakeLists.txt
              llvmPackages.libclang # Required by bindgen for whisper.cpp
              wrapGAppsHook4        # GTK/GLib schema wrapping
              makeWrapper           # For wrapping runtime PATH deps
            ] ++ pkgs'.lib.optionals isCuda [
              cuda12.cuda_nvcc
              gcc                   # Required for CUDA compilation
            ];

            buildInputs = with pkgs'; [
              openssl
              webkitgtk_4_1
              glib
              glib-networking       # HTTPS support in webkitgtk
              libsecret             # Credential storage
              libappindicator-gtk3
              alsa-lib
              librsvg
              libx11
              libxcursor
              libxrandr
              libxi
              vulkan-loader         # Whisper Vulkan GPU backend / Vulkan ICD loader
              vulkan-headers
              shaderc               # glslc — compiles whisper.cpp's Vulkan shaders
              sherpa-onnx           # Parakeet backend (C API), linked via SHERPA_LIB_PATH
            ] ++ pkgs'.lib.optionals isCuda [
              cuda12.cudatoolkit
              cuda12.cuda_cudart
              cuda12.cuda_cccl
              cuda12.libcublas
            ];

            env = {
              LIBCLANG_PATH = "${pkgs'.llvmPackages.libclang.lib}/lib";
              WEBKIT_DISABLE_COMPOSITING_MODE = "1";
              # Make sherpa-rs-sys skip its own build and link nixpkgs' sherpa-onnx
              # (it appends `/lib` and globs the C-API .so out of it).
              SHERPA_LIB_PATH = "${pkgs'.sherpa-onnx}";
            } // pkgs'.lib.optionalAttrs isCuda {
              CUDA_PATH = "${cuda12.cudatoolkit}";
              RUSTFLAGS = "-L ${cuda12.cuda_cudart}/lib/stubs";
            };

            postFixup = ''
              wrapProgram $out/bin/thoth \
                --prefix PATH : ${pkgs'.lib.makeBinPath [
                  pkgs'.wl-clipboard       # wl-copy, wl-paste
                  pkgs'.wtype              # Wayland keyboard simulation
                  pkgs'.glib.bin           # gsettings (theme detection)
                  pkgs'.libcanberra-gtk3   # canberra-gtk-play (sound feedback)
                  pkgs'.hyprland           # hyprctl (indicator positioning, keybind bridge)
                  pkgs'.socat              # Unix socket IPC for Hyprland shortcut delivery
                ]} \
                --prefix LD_LIBRARY_PATH : ${pkgs'.lib.makeLibraryPath ([
                  pkgs'.libappindicator-gtk3
                  pkgs'.vulkan-loader
                  pkgs'.sherpa-onnx        # libsherpa-onnx-c-api.so at runtime
                ] ++ pkgs'.lib.optionals isCuda [
                  cuda12.cuda_cudart
                  cuda12.libcublas
                ])}:/run/opengl-driver/lib \
                --set WEBKIT_DISABLE_COMPOSITING_MODE 1
            '';

            # Disable updater artifact signing (no private key in the nix sandbox)
            preBuild = ''
              substituteInPlace src-tauri/tauri.conf.json \
                --replace-fail '"createUpdaterArtifacts": true' '"createUpdaterArtifacts": false'
            '';

            doCheck = false; # Tests need audio hardware

            meta = with pkgs'.lib; {
              description = "Privacy-first, offline-capable voice transcription";
              homepage = "https://github.com/kirin-ri/thoth";
              license = licenses.mit;
              platforms = platforms.linux;
              mainProgram = "thoth";
            };
          });

        # Default: Vulkan (light, cache-friendly). CUDA: opt-in, heavy.
        thothVulkan = mkThoth { pkgs' = pkgsPkg; gpu = "vulkan"; };
        thothCuda = mkThoth { pkgs' = pkgs; gpu = "cuda"; };

      in {
        # `nix build` / `inputs.thoth.packages.${system}.default`
        packages.default = thothVulkan;
        packages.thoth = thothVulkan;
        # Opt-in CUDA build:  nix build .#thoth-cuda --cores 4 -j1
        packages.thoth-cuda = thothCuda;

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
          RUSTFLAGS = pkgs.lib.optionalString pkgs.stdenv.isLinux "-L /run/opengl-driver/lib";

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
      });
}
