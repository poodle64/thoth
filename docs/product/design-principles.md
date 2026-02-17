# Design Principles

UX principles and constraints guiding all design decisions for Thoth.

## Core Principles

### 1. The menu bar is home

Thoth is a menu bar application. There is no dock icon. There is no persistent window. The menu bar icon is the only always-visible element.

**Implication**: Settings are accessed via a single settings window. No multi-window sprawl.

### 2. Show nothing until needed

The recording indicator appears only during recording. Notifications appear only for errors. Settings appear only when the user seeks them.

**Implication**: No dashboards, no metrics views, no "home screen". The absence of UI is the UI.

### 3. One level deep

Navigation should be flat. The user is never more than one tap from returning to the top level.

**Implication**: Settings are organised as a flat list of sections, not as a tree. Each section is a single scrollable view.

### 4. Native feel, cross-platform ready

Use platform-appropriate patterns: system fonts, system colours, standard controls. On macOS, the app should feel native and polished.

**Implication**: Use Tauri's native integrations. Follow platform conventions. Keep the UI minimal and unobtrusive.

### 5. Warm, not clinical

Despite minimalism, the app should feel warm and crafted. The gold accent colour and "scribe" personality are intentional.

**Implication**: Subtle animations for recording state. The app has personality expressed through restraint.

### 6. Progressive disclosure

Show the simple case first. Advanced options are available but not prominent. A new user sees: hotkey, go. A power user can find: enhancement prompts, custom vocabulary, audio settings.

**Implication**: Settings have clear defaults. AI enhancement is not front-and-centre for new users.

### 7. Errors are conversations, not alerts

When something goes wrong, tell the user what happened and what to do. No modal alert dialogues. Brief, actionable inline messages.

**Implication**: Use inline status indicators and non-modal notifications.

### 8. Respect the clipboard

Transcribed text is pasted via simulated Cmd+V. The user's clipboard is saved before and restored after. This is invisible and non-negotiable.

**Implication**: Text goes directly to the cursor. Clipboard preservation happens automatically.

## Design Constraints

### Technical Constraints

- macOS 14.0+ (Tauri 2.0 + Rust backend + Svelte 5 frontend)
- Menu bar app (Tauri system tray)
- Accessibility permission required for cursor pasting
- Microphone permission required for recording
- No sandbox (text insertion requires unsandboxed)

### User Constraints

- Single user, single machine
- User may be in any application when dictating
- User expects instant response

## Interaction Patterns

### Recording Indicator

A small, non-intrusive panel that shows:

- Recording state (listening / processing / done)
- Audio level visualisation (minimal)
- A way to cancel

### Settings Access

Settings window with flat list of sections, each section a single pane.

### Notifications

- Non-modal, brief, auto-dismissing
- Reserved for errors and important state changes only

## Terminology Standards

| Term          | Meaning                                    |
| ------------- | ------------------------------------------ |
| Dictation     | The act of speaking for transcription      |
| Transcription | The text output from speech                |
| Enhancement   | AI post-processing of transcription        |
| Hotkey        | The global keyboard shortcut for recording |
| Dictionary    | Custom vocabulary and text replacements    |

## Performance Principles

- App launch to ready: < 3 seconds
- Hotkey to recording: < 200ms
- Speech end to text pasted: As fast as possible
- Memory footprint: < 500MB during transcription
