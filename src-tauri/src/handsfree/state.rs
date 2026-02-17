//! Hands-free recording state machine
//!
//! Defines the states and transitions for hands-free (VAD-based) recording mode.
//! The state machine automatically transitions based on voice activity detection.

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Hands-free recording state
///
/// Represents the five states in the hands-free recording workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HandsfreeState {
    /// Waiting for activation (shortcut press)
    #[default]
    Idle,
    /// VAD waiting for speech to begin
    Listening,
    /// Actively recording speech
    Recording,
    /// Transcribing the recorded audio
    Processing,
    /// Displaying/outputting the transcription result
    Output,
}

impl HandsfreeState {
    /// Returns a human-readable description of the state
    pub fn description(&self) -> &'static str {
        match self {
            HandsfreeState::Idle => "Waiting for activation",
            HandsfreeState::Listening => "Listening for speech",
            HandsfreeState::Recording => "Recording speech",
            HandsfreeState::Processing => "Transcribing audio",
            HandsfreeState::Output => "Transcription complete",
        }
    }

    /// Returns whether this state can be cancelled
    pub fn is_cancellable(&self) -> bool {
        matches!(
            self,
            HandsfreeState::Listening | HandsfreeState::Recording | HandsfreeState::Processing
        )
    }

    /// Returns whether audio capture is active in this state
    pub fn is_capturing_audio(&self) -> bool {
        matches!(self, HandsfreeState::Listening | HandsfreeState::Recording)
    }
}

/// Events that can trigger state transitions
#[derive(Debug, Clone)]
pub enum HandsfreeEvent {
    /// User activated hands-free mode (via shortcut)
    Activate,
    /// Voice detected by VAD
    VoiceDetected,
    /// Silence detected by VAD (speech ended)
    SilenceDetected,
    /// Transcription completed successfully
    TranscriptionComplete {
        /// The transcribed text
        text: String,
        /// Path to the audio file (for reference)
        audio_path: String,
    },
    /// Transcription failed
    TranscriptionFailed {
        /// Error message
        error: String,
    },
    /// User cancelled the operation
    Cancel,
    /// Timeout occurred (no speech detected within limit)
    Timeout,
    /// Output acknowledged (ready for next recording)
    OutputAcknowledged,
}

/// Reason for entering a state
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionReason {
    /// User initiated activation
    UserActivation,
    /// VAD detected voice activity
    VoiceActivity,
    /// VAD detected end of speech
    EndOfSpeech,
    /// Transcription processing started
    ProcessingStarted,
    /// Transcription completed successfully
    TranscriptionSuccess,
    /// Operation was cancelled by user
    UserCancellation,
    /// Operation timed out
    Timeout,
    /// Error occurred during operation
    Error { message: String },
    /// User acknowledged the output
    Acknowledged,
}

/// Result of a state transition
#[derive(Debug, Clone)]
pub struct TransitionResult {
    /// The new state after the transition
    pub new_state: HandsfreeState,
    /// Reason for the transition
    pub reason: TransitionReason,
    /// Optional data associated with the transition (e.g., transcription text)
    pub data: Option<TransitionData>,
}

/// Additional data from a state transition
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TransitionData {
    /// Transcription result
    Transcription { text: String, audio_path: String },
    /// Error information
    Error { message: String },
}

/// Hands-free state machine
///
/// Manages the state transitions for hands-free recording mode.
/// Thread-safe operations are handled externally by the HandsfreeManager.
pub struct HandsfreeStateMachine {
    /// Current state
    state: HandsfreeState,
    /// Timestamp when the current state was entered
    state_entered_at: Instant,
    /// Timestamp when listening started (for timeout tracking)
    listening_started_at: Option<Instant>,
    /// Path to the current recording file
    current_audio_path: Option<String>,
}

impl HandsfreeStateMachine {
    /// Creates a new state machine in the Idle state
    pub fn new() -> Self {
        Self {
            state: HandsfreeState::Idle,
            state_entered_at: Instant::now(),
            listening_started_at: None,
            current_audio_path: None,
        }
    }

    /// Returns the current state
    pub fn state(&self) -> HandsfreeState {
        self.state
    }

    /// Returns how long the machine has been in the current state
    pub fn time_in_state(&self) -> std::time::Duration {
        self.state_entered_at.elapsed()
    }

    /// Returns the current audio path if any
    pub fn current_audio_path(&self) -> Option<&str> {
        self.current_audio_path.as_deref()
    }

    /// Sets the current audio path
    pub fn set_audio_path(&mut self, path: String) {
        self.current_audio_path = Some(path);
    }

    /// Process an event and return the transition result if a transition occurred
    ///
    /// Returns `None` if the event is not valid for the current state.
    pub fn process_event(&mut self, event: HandsfreeEvent) -> Option<TransitionResult> {
        let transition = match (&self.state, event) {
            // IDLE state transitions
            (HandsfreeState::Idle, HandsfreeEvent::Activate) => Some(TransitionResult {
                new_state: HandsfreeState::Listening,
                reason: TransitionReason::UserActivation,
                data: None,
            }),

            // LISTENING state transitions
            (HandsfreeState::Listening, HandsfreeEvent::VoiceDetected) => Some(TransitionResult {
                new_state: HandsfreeState::Recording,
                reason: TransitionReason::VoiceActivity,
                data: None,
            }),
            (HandsfreeState::Listening, HandsfreeEvent::Cancel) => Some(TransitionResult {
                new_state: HandsfreeState::Idle,
                reason: TransitionReason::UserCancellation,
                data: None,
            }),
            (HandsfreeState::Listening, HandsfreeEvent::Timeout) => Some(TransitionResult {
                new_state: HandsfreeState::Idle,
                reason: TransitionReason::Timeout,
                data: None,
            }),

            // RECORDING state transitions
            (HandsfreeState::Recording, HandsfreeEvent::SilenceDetected) => {
                Some(TransitionResult {
                    new_state: HandsfreeState::Processing,
                    reason: TransitionReason::EndOfSpeech,
                    data: None,
                })
            }
            (HandsfreeState::Recording, HandsfreeEvent::Cancel) => Some(TransitionResult {
                new_state: HandsfreeState::Idle,
                reason: TransitionReason::UserCancellation,
                data: None,
            }),

            // PROCESSING state transitions
            (
                HandsfreeState::Processing,
                HandsfreeEvent::TranscriptionComplete { text, audio_path },
            ) => Some(TransitionResult {
                new_state: HandsfreeState::Output,
                reason: TransitionReason::TranscriptionSuccess,
                data: Some(TransitionData::Transcription { text, audio_path }),
            }),
            (HandsfreeState::Processing, HandsfreeEvent::TranscriptionFailed { error }) => {
                Some(TransitionResult {
                    new_state: HandsfreeState::Idle,
                    reason: TransitionReason::Error {
                        message: error.clone(),
                    },
                    data: Some(TransitionData::Error { message: error }),
                })
            }
            (HandsfreeState::Processing, HandsfreeEvent::Cancel) => Some(TransitionResult {
                new_state: HandsfreeState::Idle,
                reason: TransitionReason::UserCancellation,
                data: None,
            }),

            // OUTPUT state transitions
            (HandsfreeState::Output, HandsfreeEvent::OutputAcknowledged) => {
                Some(TransitionResult {
                    new_state: HandsfreeState::Idle,
                    reason: TransitionReason::Acknowledged,
                    data: None,
                })
            }
            // Also allow reactivation from Output state
            (HandsfreeState::Output, HandsfreeEvent::Activate) => Some(TransitionResult {
                new_state: HandsfreeState::Listening,
                reason: TransitionReason::UserActivation,
                data: None,
            }),

            // Invalid transitions
            _ => None,
        };

        if let Some(ref result) = transition {
            self.apply_transition(result);
        }

        transition
    }

    /// Apply a transition, updating internal state
    fn apply_transition(&mut self, result: &TransitionResult) {
        let previous_state = self.state;
        self.state = result.new_state;
        self.state_entered_at = Instant::now();

        // Track when we start listening for timeout purposes
        match result.new_state {
            HandsfreeState::Listening => {
                self.listening_started_at = Some(Instant::now());
            }
            HandsfreeState::Idle => {
                // Clear state on return to idle
                self.listening_started_at = None;
                self.current_audio_path = None;
            }
            _ => {}
        }

        tracing::info!(
            "Handsfree state transition: {:?} -> {:?} (reason: {:?})",
            previous_state,
            result.new_state,
            result.reason
        );
    }

    /// Reset the state machine to Idle
    pub fn reset(&mut self) {
        self.state = HandsfreeState::Idle;
        self.state_entered_at = Instant::now();
        self.listening_started_at = None;
        self.current_audio_path = None;
        tracing::info!("Handsfree state machine reset to Idle");
    }

    /// Check if the listening timeout has been exceeded
    ///
    /// Returns true if in Listening state and the timeout has been exceeded.
    pub fn check_listening_timeout(&self, timeout_seconds: u64) -> bool {
        if self.state != HandsfreeState::Listening {
            return false;
        }

        if let Some(started) = self.listening_started_at {
            started.elapsed().as_secs() >= timeout_seconds
        } else {
            false
        }
    }
}

impl Default for HandsfreeStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_idle() {
        let sm = HandsfreeStateMachine::new();
        assert_eq!(sm.state(), HandsfreeState::Idle);
    }

    #[test]
    fn test_activate_transitions_to_listening() {
        let mut sm = HandsfreeStateMachine::new();
        let result = sm.process_event(HandsfreeEvent::Activate);

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.new_state, HandsfreeState::Listening);
        assert_eq!(sm.state(), HandsfreeState::Listening);
    }

    #[test]
    fn test_voice_detected_transitions_to_recording() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        let result = sm.process_event(HandsfreeEvent::VoiceDetected);

        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Recording);
        assert_eq!(sm.state(), HandsfreeState::Recording);
    }

    #[test]
    fn test_silence_detected_transitions_to_processing() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        let result = sm.process_event(HandsfreeEvent::SilenceDetected);

        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Processing);
        assert_eq!(sm.state(), HandsfreeState::Processing);
    }

    #[test]
    fn test_transcription_complete_transitions_to_output() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        sm.process_event(HandsfreeEvent::SilenceDetected);
        let result = sm.process_event(HandsfreeEvent::TranscriptionComplete {
            text: "Hello world".to_string(),
            audio_path: "/tmp/test.wav".to_string(),
        });

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.new_state, HandsfreeState::Output);
        assert!(result.data.is_some());
    }

    #[test]
    fn test_output_acknowledged_returns_to_idle() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        sm.process_event(HandsfreeEvent::SilenceDetected);
        sm.process_event(HandsfreeEvent::TranscriptionComplete {
            text: "Hello world".to_string(),
            audio_path: "/tmp/test.wav".to_string(),
        });
        let result = sm.process_event(HandsfreeEvent::OutputAcknowledged);

        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Idle);
        assert_eq!(sm.state(), HandsfreeState::Idle);
    }

    #[test]
    fn test_cancel_from_listening_returns_to_idle() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        let result = sm.process_event(HandsfreeEvent::Cancel);

        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Idle);
    }

    #[test]
    fn test_cancel_from_recording_returns_to_idle() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        let result = sm.process_event(HandsfreeEvent::Cancel);

        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Idle);
    }

    #[test]
    fn test_timeout_from_listening_returns_to_idle() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        let result = sm.process_event(HandsfreeEvent::Timeout);

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.new_state, HandsfreeState::Idle);
        assert!(matches!(result.reason, TransitionReason::Timeout));
    }

    #[test]
    fn test_transcription_failed_returns_to_idle() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        sm.process_event(HandsfreeEvent::SilenceDetected);
        let result = sm.process_event(HandsfreeEvent::TranscriptionFailed {
            error: "Model not loaded".to_string(),
        });

        assert!(result.is_some());
        let result = result.unwrap();
        assert_eq!(result.new_state, HandsfreeState::Idle);
        assert!(matches!(result.reason, TransitionReason::Error { .. }));
    }

    #[test]
    fn test_invalid_transition_returns_none() {
        let mut sm = HandsfreeStateMachine::new();
        // Try to detect voice without activating first
        let result = sm.process_event(HandsfreeEvent::VoiceDetected);
        assert!(result.is_none());
        assert_eq!(sm.state(), HandsfreeState::Idle);
    }

    #[test]
    fn test_state_descriptions() {
        assert_eq!(HandsfreeState::Idle.description(), "Waiting for activation");
        assert_eq!(
            HandsfreeState::Listening.description(),
            "Listening for speech"
        );
        assert_eq!(HandsfreeState::Recording.description(), "Recording speech");
        assert_eq!(
            HandsfreeState::Processing.description(),
            "Transcribing audio"
        );
        assert_eq!(
            HandsfreeState::Output.description(),
            "Transcription complete"
        );
    }

    #[test]
    fn test_cancellable_states() {
        assert!(!HandsfreeState::Idle.is_cancellable());
        assert!(HandsfreeState::Listening.is_cancellable());
        assert!(HandsfreeState::Recording.is_cancellable());
        assert!(HandsfreeState::Processing.is_cancellable());
        assert!(!HandsfreeState::Output.is_cancellable());
    }

    #[test]
    fn test_reset() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        assert_eq!(sm.state(), HandsfreeState::Recording);

        sm.reset();
        assert_eq!(sm.state(), HandsfreeState::Idle);
    }

    #[test]
    fn test_reactivate_from_output() {
        let mut sm = HandsfreeStateMachine::new();
        sm.process_event(HandsfreeEvent::Activate);
        sm.process_event(HandsfreeEvent::VoiceDetected);
        sm.process_event(HandsfreeEvent::SilenceDetected);
        sm.process_event(HandsfreeEvent::TranscriptionComplete {
            text: "Hello".to_string(),
            audio_path: "/tmp/test.wav".to_string(),
        });
        assert_eq!(sm.state(), HandsfreeState::Output);

        // Can reactivate directly from Output
        let result = sm.process_event(HandsfreeEvent::Activate);
        assert!(result.is_some());
        assert_eq!(result.unwrap().new_state, HandsfreeState::Listening);
    }
}
