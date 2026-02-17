//! Hands-free recording manager
//!
//! Provides thread-safe management of the hands-free state machine,
//! Tauri command handlers, and event emission to the frontend.

use super::state::{
    HandsfreeEvent, HandsfreeState, HandsfreeStateMachine, TransitionData, TransitionReason,
    TransitionResult,
};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter, Runtime};

/// Default timeout for listening mode (in seconds)
const DEFAULT_LISTENING_TIMEOUT_SECONDS: u64 = 30;

/// Global hands-free manager instance
static MANAGER: OnceLock<Mutex<HandsfreeManager>> = OnceLock::new();

/// Global flag for whether hands-free mode is enabled
static HANDSFREE_ENABLED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn get_manager() -> &'static Mutex<HandsfreeManager> {
    MANAGER.get_or_init(|| Mutex::new(HandsfreeManager::new()))
}

fn get_enabled_flag() -> &'static Arc<AtomicBool> {
    HANDSFREE_ENABLED.get_or_init(|| Arc::new(AtomicBool::new(false)))
}

/// Hands-free manager
///
/// Manages the hands-free recording state machine and coordinates
/// with the audio and transcription subsystems.
pub struct HandsfreeManager {
    /// The state machine
    state_machine: HandsfreeStateMachine,
    /// Listening timeout in seconds
    listening_timeout_seconds: u64,
    /// Last transcription result (for retrieval)
    last_transcription: Option<TranscriptionResult>,
}

impl HandsfreeManager {
    /// Creates a new hands-free manager
    pub fn new() -> Self {
        Self {
            state_machine: HandsfreeStateMachine::new(),
            listening_timeout_seconds: DEFAULT_LISTENING_TIMEOUT_SECONDS,
            last_transcription: None,
        }
    }

    /// Returns the current state
    pub fn state(&self) -> HandsfreeState {
        self.state_machine.state()
    }

    /// Sets the listening timeout
    pub fn set_listening_timeout(&mut self, seconds: u64) {
        self.listening_timeout_seconds = seconds;
    }

    /// Gets the listening timeout
    pub fn listening_timeout(&self) -> u64 {
        self.listening_timeout_seconds
    }

    /// Checks if listening has timed out
    pub fn is_listening_timed_out(&self) -> bool {
        self.state_machine
            .check_listening_timeout(self.listening_timeout_seconds)
    }

    /// Sets the current audio path
    pub fn set_audio_path(&mut self, path: String) {
        self.state_machine.set_audio_path(path);
    }

    /// Gets the current audio path
    pub fn audio_path(&self) -> Option<&str> {
        self.state_machine.current_audio_path()
    }

    /// Gets the last transcription result
    pub fn last_transcription(&self) -> Option<&TranscriptionResult> {
        self.last_transcription.as_ref()
    }

    /// Process an event and return the transition result
    pub fn process_event(&mut self, event: HandsfreeEvent) -> Option<TransitionResult> {
        let result = self.state_machine.process_event(event);

        // Store transcription result if transitioning to Output
        if let Some(ref r) = result {
            if r.new_state == HandsfreeState::Output {
                if let Some(TransitionData::Transcription {
                    ref text,
                    ref audio_path,
                }) = r.data
                {
                    self.last_transcription = Some(TranscriptionResult {
                        text: text.clone(),
                        audio_path: audio_path.clone(),
                    });
                }
            }
        }

        result
    }

    /// Reset the state machine
    pub fn reset(&mut self) {
        self.state_machine.reset();
    }
}

impl Default for HandsfreeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Transcription result stored by the manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    /// The transcribed text
    pub text: String,
    /// Path to the audio file
    pub audio_path: String,
}

/// Event payload emitted to the frontend on state changes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandsfreeStateChangeEvent {
    /// The previous state
    pub previous_state: HandsfreeState,
    /// The new state
    pub new_state: HandsfreeState,
    /// Reason for the transition
    pub reason: TransitionReason,
    /// Additional data (transcription text, error message, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<TransitionData>,
    /// State description for UI display
    pub description: String,
    /// Whether the current state is cancellable
    pub is_cancellable: bool,
}

/// Current status of the hands-free mode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandsfreeStatus {
    /// Whether hands-free mode is enabled
    pub enabled: bool,
    /// Current state
    pub state: HandsfreeState,
    /// State description
    pub description: String,
    /// Whether the state is cancellable
    pub is_cancellable: bool,
    /// Listening timeout in seconds
    pub listening_timeout_seconds: u64,
    /// Last transcription if available
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transcription: Option<TranscriptionResult>,
}

// =============================================================================
// Event emission helpers
// =============================================================================

/// Emit a state change event to the frontend
fn emit_state_change<R: Runtime>(
    app: &AppHandle<R>,
    previous_state: HandsfreeState,
    result: &TransitionResult,
) -> Result<(), String> {
    let event = HandsfreeStateChangeEvent {
        previous_state,
        new_state: result.new_state,
        reason: result.reason.clone(),
        data: result.data.clone(),
        description: result.new_state.description().to_string(),
        is_cancellable: result.new_state.is_cancellable(),
    };

    app.emit("handsfree-state-change", &event)
        .map_err(|e| format!("Failed to emit state change event: {}", e))?;

    tracing::debug!(
        "Emitted handsfree-state-change: {:?} -> {:?}",
        previous_state,
        result.new_state
    );

    Ok(())
}

// =============================================================================
// Public API (non-command functions for use by other modules)
// =============================================================================

/// Check if hands-free mode is enabled
pub fn is_enabled() -> bool {
    get_enabled_flag().load(Ordering::Relaxed)
}

/// Get the current hands-free state
pub fn current_state() -> HandsfreeState {
    get_manager().lock().state()
}

/// Process a VAD voice detected event
///
/// Called by the audio module when VAD detects speech start.
pub fn on_voice_detected<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }

    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if let Some(result) = manager.process_event(HandsfreeEvent::VoiceDetected) {
        drop(manager); // Release lock before emitting
        emit_state_change(app, previous_state, &result)?;
    }

    Ok(())
}

/// Process a VAD silence detected event
///
/// Called by the audio module when VAD detects speech end.
pub fn on_silence_detected<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    if !is_enabled() {
        return Ok(());
    }

    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if let Some(result) = manager.process_event(HandsfreeEvent::SilenceDetected) {
        drop(manager);
        emit_state_change(app, previous_state, &result)?;
    }

    Ok(())
}

/// Process transcription completion
///
/// Called when transcription finishes successfully.
pub fn on_transcription_complete<R: Runtime>(
    app: &AppHandle<R>,
    text: String,
    audio_path: String,
) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if let Some(result) =
        manager.process_event(HandsfreeEvent::TranscriptionComplete { text, audio_path })
    {
        drop(manager);
        emit_state_change(app, previous_state, &result)?;
    }

    Ok(())
}

/// Process transcription failure
///
/// Called when transcription fails.
pub fn on_transcription_failed<R: Runtime>(
    app: &AppHandle<R>,
    error: String,
) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if let Some(result) = manager.process_event(HandsfreeEvent::TranscriptionFailed { error }) {
        drop(manager);
        emit_state_change(app, previous_state, &result)?;
    }

    Ok(())
}

/// Set the audio path for the current recording
pub fn set_current_audio_path(path: String) {
    get_manager().lock().set_audio_path(path);
}

/// Get the current audio path
pub fn get_current_audio_path() -> Option<String> {
    get_manager().lock().audio_path().map(|s| s.to_string())
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// Enable or disable hands-free mode
#[tauri::command]
pub fn set_handsfree_enabled(enabled: bool) {
    let previous = get_enabled_flag().swap(enabled, Ordering::SeqCst);
    if previous != enabled {
        tracing::info!(
            "Hands-free mode {}",
            if enabled { "enabled" } else { "disabled" }
        );

        // Reset state machine when disabling
        if !enabled {
            get_manager().lock().reset();
        }
    }
}

/// Check if hands-free mode is enabled
#[tauri::command]
pub fn is_handsfree_enabled() -> bool {
    is_enabled()
}

/// Get the current hands-free status
#[tauri::command]
pub fn get_handsfree_status() -> HandsfreeStatus {
    let manager = get_manager().lock();
    HandsfreeStatus {
        enabled: is_enabled(),
        state: manager.state(),
        description: manager.state().description().to_string(),
        is_cancellable: manager.state().is_cancellable(),
        listening_timeout_seconds: manager.listening_timeout(),
        last_transcription: manager.last_transcription().cloned(),
    }
}

/// Get the current hands-free state
#[tauri::command]
pub fn get_handsfree_state() -> HandsfreeState {
    current_state()
}

/// Activate hands-free mode (start listening)
///
/// This transitions from IDLE to LISTENING state.
#[tauri::command]
pub fn handsfree_activate(app: AppHandle) -> Result<(), String> {
    if !is_enabled() {
        return Err("Hands-free mode is not enabled".to_string());
    }

    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    match manager.process_event(HandsfreeEvent::Activate) {
        Some(result) => {
            drop(manager);
            emit_state_change(&app, previous_state, &result)?;

            // Start audio capture for VAD monitoring
            let path = crate::audio::start_recording()?;
            set_current_audio_path(path);

            Ok(())
        }
        None => Err(format!(
            "Cannot activate from current state: {:?}",
            previous_state
        )),
    }
}

/// Cancel the current hands-free operation
///
/// Returns to IDLE state from any cancellable state.
#[tauri::command]
pub fn handsfree_cancel(app: AppHandle) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if !previous_state.is_cancellable() {
        return Err(format!(
            "Cannot cancel from current state: {:?}",
            previous_state
        ));
    }

    match manager.process_event(HandsfreeEvent::Cancel) {
        Some(result) => {
            drop(manager);

            // Stop any ongoing recording
            if previous_state.is_capturing_audio() {
                let _ = crate::audio::stop_recording();
            }

            emit_state_change(&app, previous_state, &result)?;

            // Emit cancellation event for frontend
            app.emit("handsfree-cancelled", ())
                .map_err(|e| format!("Failed to emit cancellation event: {}", e))?;

            Ok(())
        }
        None => Err("Cancel event was not processed".to_string()),
    }
}

/// Acknowledge output (after viewing transcription result)
///
/// Transitions from OUTPUT back to IDLE.
#[tauri::command]
pub fn handsfree_acknowledge(app: AppHandle) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if previous_state != HandsfreeState::Output {
        return Err(format!(
            "Cannot acknowledge from current state: {:?}",
            previous_state
        ));
    }

    match manager.process_event(HandsfreeEvent::OutputAcknowledged) {
        Some(result) => {
            drop(manager);
            emit_state_change(&app, previous_state, &result)?;
            Ok(())
        }
        None => Err("Acknowledge event was not processed".to_string()),
    }
}

/// Set the listening timeout in seconds
#[tauri::command]
pub fn set_handsfree_timeout(seconds: u64) -> Result<(), String> {
    if seconds == 0 {
        return Err("Timeout must be greater than 0".to_string());
    }
    if seconds > 300 {
        return Err("Timeout cannot exceed 300 seconds (5 minutes)".to_string());
    }

    get_manager().lock().set_listening_timeout(seconds);
    tracing::info!("Hands-free listening timeout set to {} seconds", seconds);
    Ok(())
}

/// Get the listening timeout in seconds
#[tauri::command]
pub fn get_handsfree_timeout() -> u64 {
    get_manager().lock().listening_timeout()
}

/// Process a timeout event (called by frontend or timeout checker)
#[tauri::command]
pub fn handsfree_timeout(app: AppHandle) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    if previous_state != HandsfreeState::Listening {
        return Err(format!(
            "Timeout only applies in Listening state, current: {:?}",
            previous_state
        ));
    }

    match manager.process_event(HandsfreeEvent::Timeout) {
        Some(result) => {
            drop(manager);

            // Stop audio capture
            let _ = crate::audio::stop_recording();

            emit_state_change(&app, previous_state, &result)?;

            // Emit timeout event for frontend
            app.emit("handsfree-timeout", ())
                .map_err(|e| format!("Failed to emit timeout event: {}", e))?;

            Ok(())
        }
        None => Err("Timeout event was not processed".to_string()),
    }
}

/// Get the last transcription result
#[tauri::command]
pub fn get_last_handsfree_transcription() -> Option<TranscriptionResult> {
    get_manager().lock().last_transcription().cloned()
}

/// Reset the hands-free state machine to IDLE
#[tauri::command]
pub fn reset_handsfree_state(app: AppHandle) -> Result<(), String> {
    let mut manager = get_manager().lock();
    let previous_state = manager.state();

    // Stop any ongoing recording
    if previous_state.is_capturing_audio() {
        let _ = crate::audio::stop_recording();
    }

    manager.reset();

    let result = TransitionResult {
        new_state: HandsfreeState::Idle,
        reason: TransitionReason::UserCancellation,
        data: None,
    };

    drop(manager);
    emit_state_change(&app, previous_state, &result)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manager_initial_state() {
        let manager = HandsfreeManager::new();
        assert_eq!(manager.state(), HandsfreeState::Idle);
    }

    #[test]
    fn test_manager_default_timeout() {
        let manager = HandsfreeManager::new();
        assert_eq!(
            manager.listening_timeout(),
            DEFAULT_LISTENING_TIMEOUT_SECONDS
        );
    }

    #[test]
    fn test_manager_set_timeout() {
        let mut manager = HandsfreeManager::new();
        manager.set_listening_timeout(60);
        assert_eq!(manager.listening_timeout(), 60);
    }

    #[test]
    fn test_manager_process_activate() {
        let mut manager = HandsfreeManager::new();
        let result = manager.process_event(HandsfreeEvent::Activate);

        assert!(result.is_some());
        assert_eq!(manager.state(), HandsfreeState::Listening);
    }

    #[test]
    fn test_manager_stores_transcription() {
        let mut manager = HandsfreeManager::new();

        // Progress through states
        manager.process_event(HandsfreeEvent::Activate);
        manager.process_event(HandsfreeEvent::VoiceDetected);
        manager.process_event(HandsfreeEvent::SilenceDetected);
        manager.process_event(HandsfreeEvent::TranscriptionComplete {
            text: "Hello world".to_string(),
            audio_path: "/tmp/test.wav".to_string(),
        });

        let transcription = manager.last_transcription();
        assert!(transcription.is_some());
        assert_eq!(transcription.unwrap().text, "Hello world");
    }

    #[test]
    fn test_status_serialisation() {
        let status = HandsfreeStatus {
            enabled: true,
            state: HandsfreeState::Listening,
            description: "Listening for speech".to_string(),
            is_cancellable: true,
            listening_timeout_seconds: 30,
            last_transcription: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        let deserialised: HandsfreeStatus = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialised.enabled, status.enabled);
        assert_eq!(deserialised.state, status.state);
    }
}
