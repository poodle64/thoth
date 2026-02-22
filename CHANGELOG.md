# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

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
