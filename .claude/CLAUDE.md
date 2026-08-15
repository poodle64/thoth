# Thoth

Privacy-first, offline-capable voice-to-text for macOS and Linux: record on a hotkey, transcribe locally, paste at the cursor — no cloud round-trip on the core path.

## Scope

- Does: capture audio from any input device, transcribe it locally and offline, optionally enhance the result with a local Ollama model, and paste it at the cursor in whatever app has focus.
- Does not: sync anything to the cloud, change the system's default audio device, or require network access for the transcription path itself.
- Paste-at-cursor must restore the user's prior clipboard contents afterward — never leave the transcription sitting in the clipboard. The sole exception is a _failed_ insertion: the restore is skipped so the transcription survives on the clipboard for a manual paste, since restoring would otherwise discard it along with the failure.
- Recording toggles on one global hotkey (press to start, press again to stop); the hotkey itself is user-configurable, not hardcoded.

## Running it

- `direnv allow` once per checkout — `.envrc` provisions the pinned Rust/Node toolchain from `flake.nix`; skip it and `cargo`/`pnpm` fall back to whatever's on `PATH`.
- Rust commands (`cargo test`, `cargo clippy`) run from `src-tauri/`, not the repo root.

## Single source of truth

A version, default value, or capability flag must have exactly one definition. If another language or build system needs it, **derive it** — read the JSON, call a command, generate the constant. Do not retype it.

If a value genuinely must be duplicated, add an assertion that the copies match, and add the new location to the script that maintains it in the same commit.

This is not hypothetical bookkeeping. Every instance found so far drifted silently, because nothing asserted the copies agreed:

- `flake.nix` retyped the app version and rotted to 2026.6.3 while the app shipped 2026.6.7 (fixed in `eda03fc` by deriving it from `tauri.conf.json`).
- `src/lib/stores/config.svelte.ts` retyped the Rust shortcut defaults and disagreed with `config.rs` from February 2026 onward.
- The release workflow enumerated three version-bearing files when there were four.

Version bumps are owned by `scripts/bump-version.sh` — it is the authority on which files carry a version, and `.claude/commands/git-release.md` defers to it rather than restating the list. Adding a version to a new file means teaching the script about it in the same commit, not documenting it somewhere else.

## Pitfalls

- The Apple Neural Engine backend (FluidAudio, macOS/Apple Silicon only) shells out to `swift` at build time (`build.rs`) and is safe to compile everywhere only because the fork itself gates on `target_os = "macos", target_arch = "aarch64"` (`Cargo.toml`) — don't drop that gate, and don't assume a new native backend crate is cross-platform-safe without checking the same.
- Linux has no Metal path: GPU acceleration is a choice of mutually exclusive Cargo features (`vulkan`/`cuda`/`hipblas`), each requiring `--no-default-features` — read `docs/development/linux-setup.md` before touching the Linux build.
