# Product Intent

## Problem Statement

Voice input on macOS is either cloud-dependent (Apple Dictation, cloud APIs) or requires complex setup. Users who care about privacy, work offline, or need reliable low-latency transcription have no polished native option.

## User Need

A single-purpose tool that converts speech to text and places it wherever the cursor is. No friction, no configuration sprawl, no cloud dependency. Press a key, speak, see text.

## Value Proposition

Thoth is the fastest path from thought to text. It runs entirely on-device, respects privacy by default, and stays out of the way. It is a scribe: it listens, it writes, it disappears.

## Success Definition

Success is measured by invisibility. The user forgets Thoth is a separate application. They press a key, they speak, text appears. The tool does not demand attention, configuration, or maintenance.

## Core Principles

### 1. Invisible by default

The app lives in the menu bar. There is no dock icon, no main window demanding attention. The primary interaction is: hotkey, speak, done.

### 2. Privacy is not a feature, it is the architecture

All core functionality works offline. No data leaves the machine unless the user explicitly configures local AI enhancement (Ollama). Local-first is not a mode; it is the default and only mode for transcription.

### 3. Speed over features

Sub-second transcription latency matters more than a rich feature set. Every feature must justify its existence against the question: "does this make speaking-to-text faster or more accurate?"

### 4. Opinionated defaults, minimal configuration

The app should work well out of the box with a sensible default model and one hotkey. Power users can tune, but the defaults must be excellent.

### 5. One job, done well

Thoth transcribes voice to text. It is not a note-taking app, not a meeting recorder, not a podcast tool. Features that drift from the core job are removed.

## Non-Goals

- **Cloud transcription services**: Local models only (Whisper, Parakeet).
- **Meeting/conversation recording**: Not a recorder. Single-speaker dictation.
- **Note organisation or storage**: Transcription history exists for reference, not as a primary workflow.
- **Usage analytics**: No tracking, telemetry, or usage statistics.

## Success Indicators

- **Time from hotkey to text appearing**: Near-instant. Speed is the core value proposition.
- **Daily usage**: The user chooses voice input over typing for suitable content
- **Cognitive load**: The user never thinks about Thoth's configuration
- **Reliability**: Transcription works every time without intervention
