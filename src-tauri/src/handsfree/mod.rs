//! Hands-free recording mode
//!
//! Provides VAD-based automatic recording control where the user activates
//! listening mode and the system automatically starts/stops recording based
//! on detected speech.
//!
//! ## States
//!
//! The hands-free mode operates as a state machine with five states:
//!
//! 1. **IDLE** - Waiting for user activation via shortcut
//! 2. **LISTENING** - VAD is monitoring for speech start
//! 3. **RECORDING** - Actively recording detected speech
//! 4. **PROCESSING** - Transcribing the recorded audio
//! 5. **OUTPUT** - Displaying the transcription result
//!
//! ## State Transitions
//!
//! ```text
//!                    ┌─────────────────────────────────────────────────┐
//!                    │                                                 │
//!                    ▼                                                 │
//! ┌──────┐  activate  ┌───────────┐  voice   ┌───────────┐  silence  ┌───────────┐
//! │ IDLE │───────────►│ LISTENING │─────────►│ RECORDING │──────────►│PROCESSING │
//! └──────┘            └───────────┘          └───────────┘           └───────────┘
//!    ▲                     │                      │                        │
//!    │                     │                      │                        │
//!    │    timeout/cancel   │      cancel          │      complete          │
//!    │◄────────────────────┴──────────────────────┤                        │
//!    │                                            │                        ▼
//!    │                                            │                   ┌─────────┐
//!    │                                            │                   │ OUTPUT  │
//!    │                                            │                   └─────────┘
//!    │                                            │                        │
//!    │              cancel/error                  │                        │
//!    │◄───────────────────────────────────────────┴────────────────────────┤
//!    │                                                                     │
//!    │                           acknowledge                               │
//!    │◄────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Events
//!
//! The module emits the following events to the frontend:
//!
//! - `handsfree-state-change` - Emitted on every state transition with full details
//! - `handsfree-cancelled` - Emitted when the operation is cancelled
//! - `handsfree-timeout` - Emitted when listening times out
//!
//! ## Usage
//!
//! 1. Enable hands-free mode: `set_handsfree_enabled(true)`
//! 2. Activate via shortcut: `handsfree_activate()`
//! 3. System listens for speech (emits state changes)
//! 4. Speech detected: recording starts automatically
//! 5. Silence detected: recording stops, transcription begins
//! 6. Transcription complete: result shown to user
//! 7. User acknowledges: returns to idle

pub mod manager;
pub mod state;

pub use manager::{
    // Public API for other modules
    current_state,
    get_current_audio_path,
    // Tauri commands
    get_handsfree_state,
    get_handsfree_status,
    get_handsfree_timeout,
    get_last_handsfree_transcription,
    handsfree_acknowledge,
    handsfree_activate,
    handsfree_cancel,
    handsfree_timeout,
    is_enabled,
    is_handsfree_enabled,
    on_silence_detected,
    on_transcription_complete,
    on_transcription_failed,
    on_voice_detected,
    reset_handsfree_state,
    set_current_audio_path,
    set_handsfree_enabled,
    set_handsfree_timeout,
    HandsfreeStatus,
    TranscriptionResult,
};

pub use state::{HandsfreeEvent, HandsfreeState, TransitionData, TransitionReason};
