# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [2026.6.3] - 2026-06-02

A Linux/Wayland readiness pass. The audit behind it asked, for every OS-touching
code path, whether Linux was genuinely handled or silently degraded; these
changes close the real gaps and make every Wayland limitation visible to the
user rather than a silent dead feature.

### Added

- **Global shortcuts now work on Wayland** via the XDG Desktop Portal
  (`GlobalShortcuts`), which KDE, wlroots-based compositors, and GNOME 48+
  implement. Previously the Tauri global-shortcut plugin was used on every Linux
  session, but it only works under X11, so on a Wayland session the recording
  hotkey silently did nothing with no explanation. On a compositor without the
  portal, Thoth now tells you (a notification) that global shortcuts are
  unavailable and to use a function-key shortcut or an X11 session. The portal
  assigns the actual key (the app can only request a preferred one), and the
  assigned binding is reported back to the UI.
- A Linux/macOS CI workflow (build, `rustfmt`, `clippy -D warnings`, tests,
  `.desktop` validation, frontend type-check) now runs on every pull request.
  Previously only the release workflow compiled the code, so Linux-only breakage
  was invisible until a release was cut. The Linux job builds with the same
  Vulkan feature set the release ships, so the Linux-only code is compiled on
  every change.
- A user notice when the configured microphone is unavailable and recording
  falls back to the system default device (e.g. an unplugged USB mic), instead
  of silently switching.
- On Linux/Wayland without `wtype`, a one-time notice explaining that installing
  it makes text insertion seamless (otherwise GNOME prompts for "Allow Remote
  Interaction" each session).
- Contributor guide for building on Linux ([docs/development/linux-setup.md](docs/development/linux-setup.md)),
  covering build dependencies, runtime packages, the AppImage GPU caveat, and
  display-server behaviour.

### Changed

- The recording indicator degrades cleanly on Wayland: it no longer tries to
  follow the cursor (Wayland does not expose the global cursor position) and
  uses a fixed on-screen position instead. The cursor-tracking thread no longer
  starts on Wayland, where it would have polled indefinitely for a position it
  can never read.
- Modifier-only shortcuts (e.g. double-tap Right Shift) are now correctly
  refused on Wayland from the runtime re-registration path as well as at
  startup; previously re-registering a shortcut could start a key-polling thread
  on Wayland that cannot work.
- The Linux `.deb` now depends on `libvulkan1` (needed by the Vulkan GPU build
  at runtime) and recommends `wtype`, `xdg-utils`, and the AppIndicator runtime.
- Whisper initialisation logs the actual compiled GPU backend (Metal/CUDA/ROCm/
  Vulkan/CPU) rather than always claiming "Metal GPU"; the CPU-only Linux build
  now tells you how to enable GPU acceleration.

### Fixed

- The FluidAudio model-cache path (a macOS `~/Library/...` location) is no longer
  constructed on Linux, where it would have produced a bogus path; storage
  accounting and cleanup correctly treat it as not applicable off macOS.
- Tray icon theme detection now has a KDE/Plasma fallback (reads `kdeglobals`),
  so the icon matches dark themes on KDE, not just GNOME.

## [2026.6.2] - 2026-06-02

### Fixed

- Long recordings no longer occasionally lose the back half of the transcription (#46). Audio was captured into a small fixed buffer (about 0.7 seconds at a typical microphone's rate) that was resampled in place by the same thread; whenever that thread briefly fell behind — for example while a previous transcription was still running on the GPU — the buffer filled and silently discarded incoming audio, clipping whatever was being said at that moment. On a long recording there were many more chances for this to happen, so the loss tended to land near the end. Capture is now fully decoupled from resampling: the microphone callback does nothing but hand raw samples to an unbounded queue, and a separate thread resamples and writes the file at its own pace. A slow moment can now only delay the file, never shorten it, so no spoken audio can be dropped regardless of recording length or system load.
- The last fraction of a second of every recording (and of imported audio files) is no longer lost. The resampler holds a short internal delay that was never drained at end of stream, so the true tail never reached the file; it is now flushed properly on finalise.

## [2026.6.1] - 2026-06-01

### Added

- The Local Control API and bundled MCP server now default to **on**. They bind `127.0.0.1` only and require the bearer token (auto-generated on first run), so they are not network-exposed; MCP-capable assistants work out of the box. Enabling MCP also starts the Control API automatically — previously toggling MCP on while the API was off silently did nothing, which is why it took several restarts to come up. Toggles now take effect live without an app restart, and a failure to bind the port (e.g. already in use) surfaces an error instead of failing silently.

### Changed

- After an update, Thoth now resets its macOS permissions (microphone, accessibility, input monitoring) once so they can be re-granted from a clean slate. macOS ties permission grants to the app's code signature, which changes on each build, so an update silently invalidated the previous grants and left recording/shortcuts broken until manually reset. The reset fires only when the version actually changes, never on a fresh install or a normal relaunch.
- Australian-spelling conversion is rebuilt on the canonical VARCON / English Speller Database word map (the same data behind the en_AU dictionary in browsers and office suites), replacing a hand-maintained word list, and now defaults on. The whole `-ise` family now converts (realise, institutionalise, modernise, hospitalise — not just the words someone happened to list), alongside `-our`, `-re`, `-ence`, `-ogue` and irregular forms, while false friends (size, capsize, seize, prize) and homograph hazards (tire, curb, story, practice) are left untouched. ~3000 verified pairs.
- Spoken-number conversion (words → digits) now defaults **off**. Rule-based conversion of dictated numbers is inherently ambiguous — a lone "one" may be a pronoun ("a new one"), and a counted sequence ("six seven eight nine ten") is not a sum — so it is opt-in for when you are dictating numeric content rather than prose. When enabled it reads explicit digit sequences ("one two three" → "123") and clear compounds ("twenty three" → "23", "two hundred" → "200").
- Pasting transcribed text now uses a Core Graphics keystroke (Cmd+V) instead of driving System Events through AppleScript. This removes a second macOS permission prompt (Automation, on top of Accessibility), drops a subprocess launch from the paste path, and fixes the underlying reason the old code needed AppleScript at all (a thread-safety crash). Only the Accessibility permission is now required to paste.

### Fixed

- Transcription no longer drops the final words of long recordings (#46). On recordings over ~20 seconds the silence trimmer used voice-activity detection to cut both the leading and trailing silence; on quiet input (e.g. a lapel mic) it regularly misjudged the trailing-off end of a sentence as silence and sliced real words away before either transcription engine saw them. This was why both backends truncated at the identical word. The trimmer now removes leading silence only and always keeps the audio through to the very end.
- The recording start tone now plays reliably, without clipping, and without interfering with other audio (#58). It was gated behind a model-readiness check the stop tone doesn't have (so a cold start could begin recording before the check passed and skip the tone), and it was played through NSSound, which shares the app's audio output and got clipped when the app opened the microphone on the first record after the ~45-second warm-stream teardown. Recording cues now play through `AVAudioPlayer`: a mixable CoreAudio client, so the cue plays cleanly regardless of when you last recorded AND does not duck or pause music or other audio. The start tone is also no longer gated on model-readiness — it fires whenever a new recording begins, like the stop tone.
- The "update available" notification can now be dismissed (#67, #52). The toast was shown with infinite duration but only an "Update Now" action, so it could not be cleared without installing the update. It now has a "Later" button that dismisses it, plus a description line. (The old full-width banner with overlapping buttons was already replaced by this corner toast in 2026.6.0; this completes the dismiss-ability the toast was missing.)
- Recording no longer hijacks Bluetooth headphones (e.g. AirPods) into low-quality "call" mode. When the default input is a Bluetooth device, macOS would switch it from high-quality stereo (A2DP) to mono call audio (HFP) the moment Thoth opened it as a microphone — cutting out the user's music and leaving it degraded until the app quit, because the mic stream was held open between recordings. Thoth now records from the built-in microphone instead whenever the default input is Bluetooth (so music keeps playing in the headphones), and if a Bluetooth mic is deliberately selected, its stream is released the instant recording stops rather than held warm. Built-in and USB mics (e.g. a lapel mic) are unaffected.
- CSV export of transcription history is now generated with a standard CSV writer and guards against spreadsheet formula injection: a transcription whose text begins with `=`, `+`, `-` or `@` is no longer interpreted as a formula when the file is opened in Excel, Sheets or LibreOffice.
- The database migration runner no longer treats a genuine read error as "schema version 0", which could have re-run non-idempotent migrations and broken startup; a real error now surfaces instead of being silently swallowed.
- Removed a dead, non-anti-aliased audio resampler that could have aliased non-16kHz input had it ever been reached; the bearer-token check on the loopback Control API now uses the standard request-validation layer; and several duplicated frontend helpers (byte/duration formatting) were unified so they no longer diverge.

## [2026.6.0] - 2026-06-01

### Added

- **Local Control API**: an opt-in, loopback-only (`127.0.0.1`) HTTP API that exposes Thoth's existing control surface to local automation. Protected by a bearer token, off by default. Endpoints cover pipeline state, GPU/system info, transcription history and quality stats, the personal dictionary (list/add/update/delete/import/export), settings (read/update), prompt templates, and asynchronous transcription of local audio files (submit + poll). No new capability — every endpoint mirrors what the GUI can already do. (#65)
- **Bundled MCP server**: a native Model Context Protocol server (rmcp) mounted at `/mcp` on the same loopback server, so MCP-capable assistants (Claude, etc.) can operate Thoth through task-centric tools with no user-written glue. Tools: `dictionary`, `setting`, `transcription` (dispatchers), `transcribe_file` + `transcribe_status`, `get_state`, `get_system`, `list_prompts`. Opt-in; shares the Control API's auth. (#66)
- **Integrations settings pane**: enable/disable the Control API and MCP server, view live status and the served endpoint, and manage the bearer token — masked display with reveal/copy and rotate-with-confirmation.
- **Dictionary table**: the personal dictionary is now a sortable table (click column headers) with a sticky header that scrolls independently.

### Changed

- **Frontend rebuilt on stock shadcn-svelte**: the UI now uses the canonical shadcn-svelte component set and token system, replacing a divergent custom theme layer that had been suppressing component styling.
- **API tokens** use the canonical secret-key shape `sk-thoth-<random>` (CSPRNG, base62), recognisable and greppable for secret scanners.

### Fixed

- **Toggle switches**: render and animate correctly (state styling now matches the installed component library).
- **Recording stuck on "Processing"**: the pipeline now emits its final state after processing completes, so the UI returns to idle.
- **Right Shift hotkey**: modifier-only shortcuts (e.g. Right Shift) register correctly via the keyboard service.
- **About dialogue / dropdowns / history list**: fixed broken modal sizing, raw-value dropdowns, and overlapping history rows introduced by the component migration.
- **History rows**: left-click selects (shows detail); right-click opens the context menu. Selected-row hover stays legible.
- **Audio device & enhancement dropdowns**: show friendly device/model names instead of raw identifiers.
- **Control API**: out-of-range dictionary index returns 404 instead of 500.
- **Output filter persistence**: "Apply sentence case", "Normalise whitespace", and "Clean up punctuation" now persist and are applied at transcription time (previously reverted on return and were never applied).
- **Rogue recording indicator**: the floating indicator no longer appears mid-screen at launch or after the displays wake from sleep; it is now genuinely hidden until recording starts.

## [2026.4.1] - 2026-04-04

### Fixed

- **ObjC crash**: Use proper `block2::RcBlock` completion handler instead of null pointer in microphone permission request (caused SIGABRT crashes)
- **Keyboard service crash**: Use `DeviceState::checked_new()` with inner permission check to prevent process abort when Input Monitoring permission is revoked
- **Microphone status**: Distinguish `not_determined` from `denied` so first-launch users see correct state
- **Stale permission detection**: Replace racy push-based event (fired before webview listener exists) with reliable pull-based check on mount
- **Keyboard service restart**: Auto-start keyboard monitoring when Input Monitoring permission is newly granted (no app restart needed)
- **TCC reset**: Remove broken non-admin `reset_tcc_permission` command; use admin-elevated `reset_tcc_permissions` everywhere
- **Permission reset UX**: Stop unconditionally opening Accessibility pane after TCC reset; let setup card guide user to correct pane

## [2026.4.0] - 2026-04-03

### Added

- **macOS permission reset wizard**: Guided 4-step troubleshooting UI for quarantine, TCC, and accessibility permissions (PR #37)
- **MIT licence**
- **Pre-commit hooks**: gitleaks secret scanning
- **CI**: Reusable auto-label workflow

### Changed

- **Frontend scaffolding**: Migrated toast system to sonner, added lucide-svelte icons
- **Filler word removal**: Only unambiguous hesitation sounds (um, uh, er, ah) are removed; "like" and "you know" preserved
- **Sound feedback**: Replaced afplay subprocess with native NSSound for instant audio feedback on recording start/stop
- **Dev server ports**: Migrated from 1420/1421 to 1422/1423 to avoid conflicts

### Fixed

- **Trailing punctuation**: Consecutive transcriptions no longer run together; a period and trailing space are always appended when needed
- **Clipboard preservation**: Original clipboard contents saved before paste and restored after a configurable delay
- **Global shortcuts suppressed while screen is locked** (closes #23)
- **F14 copy-last-transcription shortcut** not working

## [2026.2.7] - 2026-02-22

### Added

- **Retranscribe recordings**: Re-process existing recordings from history with current model settings
- **Configurable recording indicator style**: Three visual styles (cursor-dot, fixed-float, pill) selectable in Settings
- **Stale TCC permission detection**: Automatically detects stale macOS accessibility/microphone permissions with guided reset flow

### Fixed

- Trailing silence now consistently padded across all transcription backends, preventing truncation at end of speech
- Recording indicator mouse tracker handles macOS sleep/wake cycles gracefully
- Updater shows actionable error states with retry and manual download fallback

## [2026.2.5] - 2026-02-20

### Added

- **Linux GPU acceleration**: CUDA (NVIDIA), HIP/ROCm (AMD), and Vulkan backend support via configurable Cargo features
- **Linux platform support**: GPU detection (`nvidia-smi`, `rocm-smi`, `hipconfig`, `vulkaninfo`), Wayland keyboard capture fallback, `wtype` text insertion for Sway/Hyprland, recording indicator positioning
- **Audio file import**: Drag-and-drop or file picker to transcribe existing audio files via symphonia decoder
- **Toast notification system**: Centralised, non-blocking toast notifications replacing alert dialogues
- **Redesigned history pane**: Select-all, inline search, clear-all, and improved layout
- **First-run onboarding**: Stepped checklist with permission explanations, guided setup state, and model download card
- **Release CI for Linux**: Ubuntu 22.04 added to release build matrix with CUDA dependencies

### Changed

- Visual consistency enforced across all settings panes
- Tray menu shortcuts and overview pane layout updated
- Release workflow: fixed pnpm cache mechanism, platform-conditional CFLAGS, removed signing key env vars
- GPU info displayed in Settings Overview pane

### Fixed

- Transcriptions no longer auto-copied to clipboard by default
- Mouse tracking reliability improved for recording indicator
- Recording blocked when no transcription model is available

## [2026.2.2] - 2026-02-16

### Added

- **AI Enhancement tray integration**: Quick access to AI enhancement toggle and all prompt templates from system tray
- **In-app prompt writing guide**: Dedicated window with comprehensive guidance on writing effective custom prompts
  - Core principles: task specificity, constraints, output directives
  - Template patterns for length-preserving, reducing, and expanding transformations
  - Colour-coded good/bad examples
  - Model-specific guidance (7B+ vs 1.5B-3B models)
  - Troubleshooting table and checklist
- **"Speak Like a Pirate" prompt**: Fun demonstration of creative transformation with proper constraints
- Permission auto-polling: After clicking Grant Access, polls every 2s for up to 30s until macOS reflects permission changes

### Changed

- **Improved all built-in prompts**: Added explicit length constraints, scope limitations, and clear output directives to prevent over-elaboration
- Enlarged app icon glyph 10% (0.60 → 0.66 scale) for better visibility
- Regenerated tray icons with solid silhouettes using flood-fill algorithm instead of outline rendering

### Fixed

- AI prompt over-elaboration issue with small models (e.g., Qwen 2.5 3B producing 5 paragraphs from 2 sentences)
- Config preservation for prompt selection when changed from tray menu

### Removed

- Trigger words system from prompts (unused feature)

## [2026.2.1] - 2026-02-16

### Added

- Scribe's Amber theme and 𓅝 ibis hieroglyph branding (app icon, tray icons, favicon)
- About dialogue with version info and project links
- Audio input source submenu in system tray for quick device switching
- Settings UI consolidated into Overview and History panes
- Eager background model warmup on startup for faster first transcription
- Local-time timestamps in debug logs for readability
- Tray-to-settings sync for audio device changes

### Fixed

- Audio device persistence: dedicated config path prevents accidental overwrite by other settings saves
- Sherpa-ONNX dylibs now included in macOS app bundle (fixes crash on Parakeet models)
- Recording-indicator window added to Tauri capabilities (was missing permissions)
- Replaced stale `@tauri-apps/plugin-global-shortcut` with missing `@tauri-apps/plugin-clipboard-manager` in frontend dependencies

## [2026.2.0] - 2026-02-14

First calendar-versioned release. Covers all work since the Tauri 2.0 migration.

### Added

- History window with detail view, audio playback with waveform, and metadata panel
- Bulk selection and operations in history (delete, export multiple)
- Performance Analysis dashboard for transcription metrics
- Transcription metadata stored in database (duration, model, word count)
- Apple-style compact mic icon indicator positioned above the text cursor
- macOS dictation tones for recording start/stop feedback
- whisper.cpp with Metal GPU acceleration as primary transcription backend
- Native keyboard capture for shortcut recording
- System tray with recording state and quick actions
- JSON/CSV/TXT export formats with shared export logic
- Stable audio device IDs and silence detection before transcription
- Separate display of original and enhanced text in history

### Fixed

- F13 key bounce with debouncing and device preference preservation
- Shortcut recording race conditions and resume overwrite bugs
- Recording indicator fallback when main window is unavailable
- Recording indicator show delay eliminated via pre-warming

### Changed

- Recording indicator redesigned from wide pill to compact rounded mic icon
- Audio capture module split into AudioRecorder and VadRecorder
- Shared component styles extracted to app.css

## [2.0.0] - Tauri Migration

Complete rewrite from Swift/SwiftUI to Tauri v2 + Svelte 5.

### Added

- Cross-platform foundation (macOS now, Linux planned)
- Recording indicator overlay positioned near text cursor
- Real-time audio level metering during recording
- Voice activity detection for speech boundaries
- Hands-free recording mode with configurable timeout
- Remote model manifest for automatic updates
- SQLite database with migrations
- JSON/CSV/TXT export formats
- Shortcut conflict detection

### Changed

- Framework: Swift/SwiftUI to Tauri 2.0 + Rust
- Frontend: SwiftUI to Svelte 5 with runes
- Audio: Core Audio to cpal (cross-platform)
- Transcription: whisper.cpp to Sherpa-ONNX with Parakeet models
- Database: SwiftData to SQLite with rusqlite
- AI: Multi-provider to Ollama-focused

### Removed

- Swift/SwiftUI codebase
- whisper.cpp/Metal GPU acceleration
- macOS-only features (Notch Recorder, etc.)

## [1.0.0] - Swift Version (Archived)

Original Swift/SwiftUI implementation. See `archive/swift-v1` branch.
