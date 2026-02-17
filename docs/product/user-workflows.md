# User Workflows

Workflows describe what the user does, independent of specific UI implementation.

## Primary Flow: Dictation

**Trigger**: User presses the global hotkey.

**Context**: User is in any application with a text input focused.

**Steps**:

1. User presses hotkey
2. A minimal recording indicator appears
3. User speaks
4. User presses hotkey again (or VAD detects silence)
5. Transcription processes via local model (Whisper or Parakeet)
6. Final transcribed text is inserted at cursor position
7. Recording indicator disappears

**Outputs**: Transcribed text inserted at cursor. Clipboard preserved.

**Edge Cases**:

- No microphone available: show notification
- Accessibility not granted: direct user to permissions
- Empty transcription (silence): dismiss quietly
- Model not downloaded: prompt user to download in settings

**Success Criteria**: Text appears at cursor within 2 seconds of speech ending.

---

## Secondary Flow: AI Enhancement

**Trigger**: User enables AI enhancement in settings.

**Context**: After transcription completes, before text is pasted.

**Steps**:

1. Transcription completes
2. Raw text is sent to Ollama (or custom API endpoint)
3. Enhanced text replaces raw text at cursor
4. If AI is unavailable, raw text is pasted (graceful fallback)

**Edge Cases**:

- AI provider unreachable: paste raw text, notify briefly
- Enhancement takes too long: paste raw text

**Success Criteria**: Enhancement adds value (grammar, formatting) without noticeably increasing latency.

---

## Setup Flow: First Launch

**Trigger**: First application launch after install.

**Context**: New user, no permissions granted.

**Steps**:

1. App appears in menu bar
2. User clicks menu bar icon
3. App presents setup:
   a. Grant microphone permission
   b. Grant accessibility permission
   c. Set a recording hotkey
4. Setup completes; user can immediately dictate

**Edge Cases**:

- User skips permissions: app functions limited
- Model not downloaded: app prompts for download

**Success Criteria**: User goes from install to first successful transcription in under 2 minutes.

---

## Maintenance Flow: Personal Dictionary

**Trigger**: Thoth consistently mis-transcribes a word or phrase.

**Context**: User wants to teach Thoth domain-specific vocabulary.

**Steps**:

1. User opens settings from menu bar
2. User navigates to dictionary settings
3. User adds a vocabulary word or a text replacement rule
4. Changes apply to the next transcription

**Edge Cases**:

- Conflicting replacements: last-added wins
- Import from file: supported for bulk vocabulary

**Success Criteria**: Custom vocabulary improves transcription accuracy for domain-specific terms.
