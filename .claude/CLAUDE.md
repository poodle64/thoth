# Thoth

Privacy-first, offline-capable voice transcription application.

## Dev Environment

- Environment: `thoth` (conda)
- Framework: Tauri 2.0 (Rust backend, Svelte 5 frontend)
- Minimum macOS: 14.0 (Sonoma)
- Dev server: port 1422, HMR: port 1423

## Stack

- **UI**: Svelte 5 with runes, Tailwind CSS
- **Audio**: cpal for cross-platform capture
- **Transcription**: whisper.cpp with Metal GPU (primary), Sherpa-ONNX with Parakeet (fallback)
- **AI Enhancement**: Ollama (local LLM)
- **Persistence**: SQLite with rusqlite
- **Windows**: Multi-window (main, history, recording indicator)

## Commands

```bash
pnpm install        # Install dependencies
pnpm tauri dev      # Development build
pnpm tauri build    # Production build
cargo test          # Run Rust tests (from src-tauri/)
```

## Key Reminders

- Use Svelte 5 runes ($state, $derived, $effect) not stores
- Rust backend handles all audio and transcription
- Frontend communicates via Tauri commands and events

## Sources of Truth

- **Rules**: `.claude/rules/`
- **Development docs**: `docs/development/`
- **Product docs**: `docs/product/`
