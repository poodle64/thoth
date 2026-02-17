# Thoth Project Instructions

A privacy-first, offline-capable voice transcription application.

## Development Environment

- **Framework**: Tauri 2.0
- **Backend**: Rust
- **Frontend**: Svelte 5 + SvelteKit
- **Minimum macOS**: 14.0 (Sonoma)

## Technology Stack

- **UI**: Svelte 5 with runes, Tailwind CSS
- **Audio**: cpal for cross-platform capture
- **Transcription**: whisper.cpp with Metal GPU (primary), Sherpa-ONNX with Parakeet (fallback)
- **AI Enhancement**: Ollama (local LLM)
- **Persistence**: SQLite with rusqlite
- **Windows**: Multi-window (main, history, recording indicator)

## Build Commands

```bash
pnpm install        # Install dependencies
pnpm tauri dev      # Development build
pnpm tauri build    # Production build
cargo test          # Run Rust tests (from src-tauri/)
```

## Key Reminders

- Do NOT create summary markdown documents
- Use Svelte 5 runes ($state, $derived, $effect) not stores
- Rust backend handles all audio and transcription
- Frontend communicates via Tauri commands and events

## Project Structure

```
├── src/                    # Svelte frontend
│   ├── lib/
│   │   ├── components/     # Reusable components
│   │   ├── stores/         # Svelte stores (.svelte.ts)
│   │   └── windows/        # Window-specific components
│   └── routes/             # SvelteKit routes
└── src-tauri/              # Rust backend
    └── src/
        ├── audio/          # Audio capture, metering, VAD
        ├── commands/       # Miscellaneous Tauri commands
        ├── database/       # SQLite persistence
        ├── enhancement/    # AI enhancement (Ollama)
        ├── handsfree/      # Hands-free VAD-based recording
        ├── platform/       # Platform-specific code (macOS)
        ├── shortcuts/      # Global hotkey management
        └── transcription/  # whisper.cpp + Sherpa-ONNX integration
```

## Sources of Truth

- **Master rules**: `.claude/rules/master/` (symlinked from master project)
- **Architecture docs**: `docs/architecture/`
- **Product docs**: `docs/product/`
