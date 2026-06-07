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

      in {
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
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''

            # whisper-rs-sys runs bindgen over ggml-vulkan.h. bindgen invokes
            # libclang directly, bypassing the nix cc-wrapper, so it cannot find
            # the libc headers (stdio.h) or clang's own builtin headers
            # (stddef.h). bindgen then errors and whisper-rs-sys SILENTLY falls
            # back to its bundled no-Vulkan bindings, so the ggml_backend_vk_*
            # symbols go missing and whisper-rs fails to compile its Vulkan
            # module (issue #64). Feed bindgen the cc-wrapper's libc flags plus
            # clang's resource dir. A standard apt system finds these in
            # /usr/include and lib/clang, so CI does not need this.
            export BINDGEN_EXTRA_CLANG_ARGS="$(< ${pkgs.stdenv.cc}/nix-support/libc-cflags) -idirafter ${pkgs.llvmPackages.libclang.lib}/lib/clang/${pkgs.lib.versions.major pkgs.llvmPackages.libclang.version}/include"
          '';
        };
      });
}
