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

        # CUDA packages for whisper.cpp GPU acceleration
        cudaPackages = pkgs.cudaPackages_12;

        # CUDA-enabled sherpa-onnx prebuilt (k2-fsa release) for GPU Parakeet.
        # The `parakeet-cuda` cargo feature links sherpa-onnx as `shared`, and
        # SHERPA_ONNX_LIB_DIR points it here instead of downloading the CPU build.
        # This archive ships libsherpa-onnx-c-api.so + libonnxruntime.so with the
        # CUDA execution provider (libonnxruntime_providers_cuda.so). cuDNN/cudart
        # are supplied via LD_LIBRARY_PATH in the `cuda` dev shell below.
        sherpaOnnxCuda = pkgs.stdenvNoCC.mkDerivation {
          pname = "sherpa-onnx-cuda";
          version = "1.13.2";
          src = pkgs.fetchurl {
            url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.2/sherpa-onnx-v1.13.2-cuda-12.x-cudnn-9.x-linux-x64-gpu.tar.bz2";
            hash = "sha256-vRE8k6GLoPm24MrEaramoXYGvde3cbbq7gy9b5bOY/4=";
          };
          dontConfigure = true;
          dontBuild = true;
          installPhase = "mkdir -p $out && cp -r lib $out/lib";
        };

        # Dev-shell packages (shared by both shells).
        commonPackages = with pkgs; [
          # Rust / Tauri
          rustToolchain
          cargo
          rustc
          rust-analyzer

          # Tauri dependencies (platform-specific)
          openssl
          pkg-config
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
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
        ] ++ pkgs.lib.optionals pkgs.stdenv.isDarwin [
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
        ] ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
          glib
          libsecret
          # Native Wayland keyboard simulation (alternative to X11-based enigo)
          wtype
        ];

        # whisper-rs-sys runs bindgen over ggml-vulkan.h. bindgen invokes libclang
        # directly, bypassing the nix cc-wrapper, so it cannot find the libc headers
        # (stdio.h) or clang's own builtin headers (stddef.h). bindgen then errors and
        # whisper-rs-sys SILENTLY falls back to its bundled no-Vulkan bindings, so the
        # ggml_backend_vk_* symbols go missing and whisper-rs fails to compile its
        # Vulkan module (issue #64). Feed bindgen the cc-wrapper's libc flags plus
        # clang's resource dir. A standard apt system finds these in /usr/include and
        # lib/clang, so CI does not need this.
        bindgenHook = pkgs.lib.optionalString pkgs.stdenv.isLinux ''

          export BINDGEN_EXTRA_CLANG_ARGS="$(< ${pkgs.stdenv.cc}/nix-support/libc-cflags) -idirafter ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.versions.major pkgs.llvmPackages.libclang.version}/include"
        '';

        # One dev-shell definition, optionally wired for GPU Parakeet (CUDA).
        mkThothShell = { gpuParakeet ? false }: pkgs.mkShell ({
          # Platform-specific library paths (Linux). With gpuParakeet, also expose
          # the CUDA sherpa-onnx libs + cuDNN so the CUDA execution provider loads.
          LD_LIBRARY_PATH = pkgs.lib.optionalString pkgs.stdenv.isLinux
            (pkgs.lib.makeLibraryPath ([
              pkgs.libappindicator-gtk3
              pkgs.vulkan-loader
              cudaPackages.cuda_cudart
              cudaPackages.cuda_cccl
              cudaPackages.libcublas
            ] ++ pkgs.lib.optionals gpuParakeet [
              sherpaOnnxCuda           # libsherpa-onnx-c-api.so + onnxruntime CUDA EP
              cudaPackages.cudnn       # libcudnn.so.9
              # The onnxruntime CUDA execution provider dlopen()s the full CUDA
              # math-library set; a single missing one makes it abort (no CPU
              # fallback), so provide all of them.
              cudaPackages.libcurand   # libcurand.so.10
              cudaPackages.libcufft    # libcufft.so.11
              cudaPackages.libcusparse # libcusparse.so.12
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

          packages = commonPackages
            ++ pkgs.lib.optionals (gpuParakeet && pkgs.stdenv.isLinux) [ cudaPackages.cudnn ];

          shellHook = ''
            echo "𓅝 Thoth Development Environment${pkgs.lib.optionalString gpuParakeet " (GPU Parakeet / CUDA)"}"
            echo "================================"
            echo "  Rust: $(rustc --version)"
            echo "  Node: $(node --version)"
            echo "  pnpm: $(pnpm --version)"
            echo ""
          '' + (if gpuParakeet then ''
            echo "GPU Parakeet (NVIDIA CUDA) is wired up. Build/run with:"
            echo "  pnpm tauri dev --no-default-features --features parakeet-cuda,vulkan"
            echo "  pnpm tauri build --no-default-features --features parakeet-cuda,vulkan"
            echo ""
            echo "Then transcribe and watch 'nvidia-smi' to confirm the GPU engages."
            echo "Logs show 'Attempting CUDA provider...' / 'CUDA provider initialised'."
          '' else ''
            echo "Commands:"
            echo "  pnpm install        - Install dependencies"
            echo "  pnpm tauri dev      - Start development build"
            echo "  pnpm tauri dev -- --features cuda    - Dev with CUDA GPU acceleration"
            echo "  pnpm tauri build -- --features cuda  - Build with CUDA"
            echo "  cargo test          - Run Rust tests (from src-tauri/)"
            echo ""
            echo "GPU Acceleration (Linux):"
            echo "  --features cuda     - NVIDIA GPUs (Whisper)"
            echo "  --features vulkan   - Cross-platform (Whisper)"
            echo "  nix develop .#cuda  - GPU Parakeet (NVIDIA, via sherpa-onnx CUDA)"
          '') + bindgenHook;
        } // pkgs.lib.optionalAttrs gpuParakeet {
          # Make sherpa-onnx-sys link the CUDA libs instead of downloading CPU ones.
          SHERPA_ONNX_LIB_DIR = "${sherpaOnnxCuda}/lib";
        });

        # Runtime libraries the wrapped binary loads (CUDA EP + Vulkan + tray).
        runtimeLibs = [
          pkgs.libappindicator-gtk3
          pkgs.vulkan-loader
          sherpaOnnxCuda
          cudaPackages.cudnn
          cudaPackages.libcurand
          cudaPackages.libcufft
          cudaPackages.libcusparse
          cudaPackages.cuda_cudart
          cudaPackages.libcublas
        ];

        # ---------------------------------------------------------------------
        # Installable, importable package:
        #   inputs.thoth.packages.${system}.default
        #
        # Builds GPU Parakeet (CUDA, via the prebuilt sherpa-onnx pinned above)
        # plus Whisper (Vulkan). The binary is wrapped with the Wayland runtime
        # tools (wl-clipboard, wtype) and the CUDA/Vulkan libraries it dlopen()s.
        # `hyprctl` is taken from the user's PATH (present on any Hyprland system).
        #
        # Refresh the two hashes whenever Cargo.lock / pnpm-lock.yaml change:
        # set both to lib.fakeHash, run `nix build`, paste the reported pnpmDeps
        # hash, run again, paste the cargoHash, run once more to compile.
        # ---------------------------------------------------------------------
        thothPackage = pkgs.rustPlatform.buildRustPackage (finalAttrs: {
          pname = "thoth";
          version = "2026.6.3";
          src = ./.;

          cargoRoot = "src-tauri";
          buildAndTestSubdir = "src-tauri";
          # TODO(one-time): run `nix build .#packages.x86_64-linux.default`, then
          # paste the `got:` hash it reports here. (Couldn't be pre-computed where
          # this was authored — crates.io rate-limited the vendor fetch.)
          cargoHash = "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

          # GPU Parakeet (sherpa-onnx CUDA via SHERPA_ONNX_LIB_DIR) + Whisper (Vulkan).
          # No default features (drops fluidaudio, a macOS-only git dep, and the
          # CPU `parakeet` link mode).
          buildNoDefaultFeatures = true;
          buildFeatures = [ "parakeet-cuda" "vulkan" ];

          pnpmDeps = pkgs.fetchPnpmDeps {
            inherit (finalAttrs) pname version src;
            fetcherVersion = 3;
            hash = "sha256-dDxzWfpqs2wuT6rSojfP7r0neOnCbkB3ha2fA45Icws=";
          };

          nativeBuildInputs = with pkgs; [
            cargo-tauri.hook
            nodejs
            pnpmConfigHook
            pnpm
            pkg-config
            cmake
            git
            llvmPackages.libclang
            shaderc # glslc — compiles whisper.cpp's Vulkan shaders
            wrapGAppsHook4
            makeWrapper
          ];

          buildInputs = with pkgs; [
            openssl
            webkitgtk_4_1
            glib
            glib-networking
            libsecret
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
            sherpaOnnxCuda # Parakeet C API, linked via SHERPA_ONNX_LIB_DIR
          ];

          env = {
            LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
            WEBKIT_DISABLE_COMPOSITING_MODE = "1";
            SHERPA_ONNX_LIB_DIR = "${sherpaOnnxCuda}/lib";
          };

          # whisper-rs-sys bindgen needs the cc-wrapper libc flags + clang headers
          # (issue #64); `$(< … )` must run at build time, so it lives here.
          preConfigure = bindgenHook;

          # No updater signing key in the sandbox.
          preBuild = ''
            substituteInPlace src-tauri/tauri.conf.json \
              --replace-fail '"createUpdaterArtifacts": true' '"createUpdaterArtifacts": false'
          '';

          postFixup = ''
            wrapProgram $out/bin/thoth \
              --prefix PATH : ${pkgs.lib.makeBinPath [
                pkgs.wl-clipboard      # wl-copy / wl-paste
                pkgs.wtype             # Wayland keyboard simulation
                pkgs.glib.bin          # gsettings (theme detection)
                pkgs.libcanberra-gtk3  # canberra-gtk-play (sound feedback)
              ]} \
              --prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath runtimeLibs}:/run/opengl-driver/lib \
              --set WEBKIT_DISABLE_COMPOSITING_MODE 1
          '';

          doCheck = false; # tests need audio hardware

          meta = with pkgs.lib; {
            description = "Privacy-first, offline-capable voice transcription (GPU Parakeet + Whisper)";
            homepage = "https://github.com/poodle64/thoth";
            license = licenses.mit;
            platforms = [ "x86_64-linux" ];
            mainProgram = "thoth";
          };
        });

      in {
        # `nix build` / `inputs.thoth.packages.${system}.default`
        packages.default = thothPackage;
        packages.thoth = thothPackage;

        devShells.default = mkThothShell { };
        devShells.cuda = mkThothShell { gpuParakeet = true; };
      });
}
