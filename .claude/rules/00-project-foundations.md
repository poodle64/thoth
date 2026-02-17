---
paths: '**/*'
---

# Thoth Project Foundations

## Purpose

Thoth is a privacy-first, offline-capable voice transcription application for macOS and Linux that converts speech to text with near-instantaneous processing, supporting multiple transcription engines and AI enhancement.

## Project Scope

### What This Project Does

- Records audio from any input device without changing system defaults
- Transcribes speech locally using whisper-rs (Metal GPU) or sherpa-rs (fallback)
- Enhances transcriptions with AI via Ollama (local LLM)
- Inserts transcribed text at cursor position in any application
- Provides a multi-window interface (main, history, recording indicator)

### What This Project Does NOT Do

- Does NOT sync data to cloud
- Does NOT modify system audio settings or default devices
- Does NOT require internet for basic transcription (local models)
- Does NOT store audio permanently by default (configurable retention)

## Authority Note

This rule documents project-specific practice and relies on master rules for requirements. Master rules define universal principles; this rule describes how Thoth implements them.

## Project Context

### Technology Stack

- **Framework**: Tauri 2.0
- **Backend**: Rust (2021 edition)
- **Frontend**: Svelte 5 + SvelteKit
- **UI**: Svelte 5 with runes ($state, $derived, $effect), Tailwind CSS
- **Build**: pnpm + cargo via tauri CLI

### Core Dependencies

#### Rust Backend (src-tauri/)

- **Audio**: cpal (capture), rubato (resampling), hound (WAV), webrtc-vad (voice activity detection)
- **Transcription**: whisper-rs (primary, Metal GPU acceleration), sherpa-rs (fallback for Parakeet models)
- **Persistence**: rusqlite (SQLite)
- **AI Enhancement**: reqwest (HTTP client for Ollama)
- **Utilities**: tokio (async runtime), tracing (logging), chrono (datetime)

#### Frontend (src/)

- **Framework**: SvelteKit with static adapter
- **UI Components**: Svelte 5, lucide-svelte (icons)
- **Tauri Plugins**: @tauri-apps/api, global-shortcut, dialogue, fs, autostart

### Architecture

```
+-------------------------------------------------------------+
|                     Thoth Application                        |
+-------------------------------------------------------------+
|  +-----------+  +-----------+  +---------------------+      |
|  |  Svelte   |  |   Tray    |  |   Recording         |      |
|  |  Windows  |  |   Menu    |  |   Indicator Panel   |      |
|  +-----+-----+  +-----+-----+  +----------+----------+      |
|        +---------------+------------------+                  |
|                        |                                     |
|  +---------------------v------------------------------------+|
|  |                  Tauri Commands                          ||
|  |     (IPC between frontend and Rust backend)              ||
|  +---------------------+------------------------------------+|
|        +---------------+------------------+                  |
|        v               v                  v                  |
|  +-----------+  +-----------+  +---------------------+      |
|  | Audio     |  |Transcr-   |  |  AI Enhancement     |      |
|  | Module    |  |iption     |  |  Module             |      |
|  | (cpal)    |  | Module    |  |  (Ollama)           |      |
|  +-----+-----+  +-----+-----+  +----------+----------+      |
|        |              |                   |                  |
|        v              v                   v                  |
|  +-----------+  +-----------+  +---------------------+      |
|  | WAV File  |  |whisper-rs |  |  External APIs      |      |
|  | (16kHz    |  |(Metal GPU)|  |  (local Ollama)     |      |
|  | mono)     |  |sherpa-rs  |  |                     |      |
|  +-----------+  +-----------+  +---------------------+      |
|                                                              |
|  +----------------------------------------------------------+|
|  |                    SQLite Database                       ||
|  |   Transcription history, dictionary, clipboard history  ||
|  +----------------------------------------------------------+|
+--------------------------------------------------------------+
```

### Project Structure

```
thoth/
+-- src/                    # Svelte frontend
|   +-- lib/
|   |   +-- components/     # Reusable components
|   |   +-- stores/         # Svelte stores (.svelte.ts)
|   |   +-- windows/        # Window-specific components
|   +-- routes/             # SvelteKit routes
+-- src-tauri/              # Rust backend
    +-- src/
        +-- audio/          # Audio capture, metering, VAD
        +-- database/       # SQLite persistence
        +-- enhancement/    # AI enhancement (Ollama)
        +-- transcription/  # whisper-rs, sherpa-rs integration
        +-- shortcuts/      # Global hotkey management
        +-- pipeline/       # Recording -> transcription -> output flow
        +-- config/         # JSON configuration
```

### Configuration

- **Config location**: `~/.thoth/config.json`
- **Model storage**: `~/.thoth/models/`
- **Database**: `~/.thoth/thoth.db`
- **Logs**: `~/.thoth/logs/`

### Core Philosophy

Thoth is designed around **privacy-first, local-first operation**.

- **Privacy**: All core functionality works offline; cloud features are opt-in
- **Speed**: Sub-second transcription with Metal GPU acceleration
- **Simplicity**: Single hotkey to record, automatic paste at cursor
- **Flexibility**: Multiple transcription engines, AI providers, and output modes

## Non-Negotiable Constraints

### Design Constraints

- Must work fully offline with local whisper models
- Must not change system default audio device
- Must preserve user clipboard contents after paste
- Must support multiple recording modes (toggle, push-to-talk, hands-free)

### Technology Constraints

- macOS 14.0+ required (Metal GPU support)
- Linux supported (without Metal acceleration)
- Rust 2021 edition required
- Node.js 18+ and pnpm required for frontend build

## Build Commands

```bash
pnpm install        # Install frontend dependencies
pnpm tauri dev      # Development build with hot reload
pnpm tauri build    # Production build
cargo test          # Run Rust tests (from src-tauri/)
cargo clippy        # Run Rust linter (from src-tauri/)
```

## Sources of Truth

- **Master rules**: `.claude/rules/master/` (symlinked) - Universal principles
- **Architecture docs**: `docs/architecture/` - System design and data flows
- **Product docs**: `docs/product/` - User workflows and design principles
- **GitHub Issues**: Task tracking and feature planning

## Rule Interpretation Notes

- Audio recording uses cpal directly for cross-platform support
- Transcription prefers whisper-rs with Metal GPU; falls back to sherpa-rs
- Project-specific behavioural rules are defined in numbered rule files (20+, 50+, etc.)
