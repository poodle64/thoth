# Thoth

Privacy-first, offline-capable voice-to-text for macOS and Linux: record on a hotkey, transcribe locally, paste at the cursor — no cloud round-trip on the core path.

## Scope

- Does: capture audio from any input device, transcribe it locally and offline, optionally enhance the result with a local Ollama model, and paste it at the cursor in whatever app has focus.
- Does not: sync anything to the cloud, change the system's default audio device, or require network access for the transcription path itself.
- Paste-at-cursor must restore the user's prior clipboard contents afterward — never leave the transcription sitting in the clipboard.
- Recording toggles on one global hotkey (press to start, press again to stop); the hotkey itself is user-configurable, not hardcoded.

## Running it

- `direnv allow` once per checkout — `.envrc` provisions the pinned Rust/Node toolchain from `flake.nix`; skip it and `cargo`/`pnpm` fall back to whatever's on `PATH`.
- Rust commands (`cargo test`, `cargo clippy`) run from `src-tauri/`, not the repo root.

## Pitfalls

- The Apple Neural Engine backend (FluidAudio, macOS/Apple Silicon only) shells out to `swift` at build time (`build.rs`) and is safe to compile everywhere only because the fork itself gates on `target_os = "macos", target_arch = "aarch64"` (`Cargo.toml`) — don't drop that gate, and don't assume a new native backend crate is cross-platform-safe without checking the same.
- Linux has no Metal path: GPU acceleration is a choice of mutually exclusive Cargo features (`vulkan`/`cuda`/`hipblas`), each requiring `--no-default-features` — read `docs/development/linux-setup.md` before touching the Linux build.
