# Linux Development Setup

How to build and run Thoth from source on Linux. Thoth supports macOS (first-class) and Linux; this guide covers the Linux-specific build dependencies and the runtime packages an installed build needs.

## Build dependencies

Tauri needs the system GTK/WebKit/audio development libraries to compile, and the Whisper Vulkan backend needs the Vulkan toolchain to compile its compute shaders at build time. On Debian/Ubuntu:

```bash
sudo apt-get update
sudo apt-get install -y \
  libgtk-3-dev libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev \
  patchelf libasound2-dev \
  libvulkan-dev glslc spirv-headers
```

- `libgtk-3-dev`, `libwebkit2gtk-4.1-dev`, `librsvg2-dev`, `patchelf` — Tauri's webview and bundler.
- `libappindicator3-dev` — system tray.
- `libasound2-dev` — ALSA, for cpal audio capture.
- `libvulkan-dev`, `glslc`, `spirv-headers` — the Vulkan GPU backend. whisper.cpp's GGML Vulkan backend compiles its shaders at build time via CMake (`find_package(Vulkan COMPONENTS glslc REQUIRED)` and `find_package(SPIRV-Headers REQUIRED)`); `glslang-tools` does **not** satisfy this. Omit these only if you build CPU-only without `--features vulkan`.

The toolchain otherwise is the standard one: a recent stable Rust (via `rustup`), Node.js LTS, and `pnpm`. `direnv` loads the project environment from `.envrc` (run `direnv allow` once), and on Debian/Ubuntu you install the system packages above through apt. On NixOS — or any machine with Nix — the committed `flake.nix` provides the entire toolchain and every build dependency (Rust, Node, pnpm, GTK/WebKit, the Vulkan toolchain, CUDA); run `nix develop` and build inside that shell instead of installing the apt packages.

## Building

```bash
pnpm install

# CPU-only (no GPU build deps needed beyond the GTK/audio set)
pnpm tauri build -- --no-default-features

# Vulkan GPU (vendor-neutral: NVIDIA, AMD, Intel) — what the Linux release ships
pnpm tauri build -- --no-default-features --features vulkan

# NVIDIA-specific (CUDA Toolkit 12.x + drivers)
pnpm tauri build -- --no-default-features --features cuda

# AMD-specific (ROCm 6.x)
pnpm tauri build -- --no-default-features --features hipblas
```

`--no-default-features` is required on Linux: the default features (Parakeet + FluidAudio) are off here. FluidAudio is Apple-Neural-Engine only (macOS/Apple Silicon). Parakeet — now the official k2-fsa `sherpa-onnx` crate — **does** build and link on Linux, and it links **statically** (no extra runtime library to ship); this was previously blocked when the backend used `sherpa-rs`, whose Linux package shipped no static archive (see [#53](https://github.com/poodle64/thoth/issues/53)). It is still left out of the default Linux build pending runtime verification of a Linux transcription; to try it, add `--features parakeet`. The default Linux transcription backend is Whisper (whisper.cpp).

Pick exactly one GPU feature; they are mutually exclusive. If GPU initialisation fails at runtime, Thoth falls back to CPU automatically.

## Runtime dependencies

A packaged `.deb` declares these; if you run a raw binary, install them yourself.

**Required:**

- `libvulkan1` — the Vulkan loader, needed by the GPU build at runtime to find the GPU's Vulkan driver. Without it the GPU build silently falls back to CPU. You also need a vendor Vulkan driver (`mesa-vulkan-drivers` for AMD/Intel, the NVIDIA driver's Vulkan ICD for NVIDIA).

**Recommended (the app works without them, with reduced functionality):**

- `wtype` — native Wayland text insertion. Without it, insertion falls back to XWayland emulation, which on GNOME triggers an "Allow Remote Interaction" permission prompt each session.
- `xdg-utils` — opens external links and settings panels (`xdg-open`).
- `libayatana-appindicator3-1` — the system tray icon (the modern AppIndicator runtime).

## AppImage and GPU

The Linux AppImage is built with Vulkan enabled, but it does **not** bundle the Vulkan loader or GPU drivers (`bundleMediaFramework` bundles GStreamer media framework libraries, not GPU drivers — those are host-and-vendor-specific and cannot be portably bundled). For GPU-accelerated transcription from the AppImage, the host must have `libvulkan1` and a working GPU Vulkan driver installed. Without them the AppImage runs transcription on CPU.

## Display server notes

Thoth's behaviour differs between X11 and Wayland because Wayland restricts what an application may do:

- **Global shortcuts**: on Wayland these go through the XDG Desktop Portal, which KDE, wlroots-based compositors, and GNOME 48+ implement. On a compositor without it, Thoth tells you (a toast) and you use a function-key shortcut or an X11 session.
- **Recording indicator**: on Wayland the indicator cannot follow the cursor (no global cursor position) and uses a fixed on-screen position.
- **Modifier-only shortcuts** (e.g. double-tap Right Shift): unavailable on Wayland; use a function-key shortcut instead.

### Known risk to verify on Wayland

The portal's `Activated` D-Bus signals have been reported to stop arriving when the
app is launched from an application launcher (e.g. fuzzel) rather than a terminal —
the underlying zbus connection can be torn down in that environment. When testing,
**launch Thoth both from a terminal and from your normal app launcher/menu, and
confirm the global shortcut fires in both cases.** If it works from a terminal but
not from the launcher, that is this issue; report it (the fix is to hold the portal
proxy and session in process-lifetime state rather than a single task, and re-subscribe
on signal-stream end). Note also that changing a shortcut in Settings does not currently
re-bind the portal live — restart the app after changing a shortcut on Wayland.

## Continuous integration

`.github/workflows/ci.yaml` compiles, lints (`cargo fmt --check`, `cargo clippy -D warnings`), and tests on both macOS and Linux on every pull request, building Linux with `--no-default-features --features vulkan` so the Linux-only code (the Wayland portal, X11 mouse tracking, the Vulkan backend) is compile-checked even though the primary dev machine is macOS. It also validates the `.desktop` entry and type-checks the frontend.
