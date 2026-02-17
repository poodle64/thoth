//! Push-to-talk (PTT) mode management
//!
//! Provides push-to-talk recording functionality where holding a key starts
//! recording and releasing it stops recording and triggers transcription.

use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Tracks whether PTT is currently active (key held down)
static PTT_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Tracks whether PTT mode is enabled (vs toggle mode)
static PTT_MODE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Handle PTT key being pressed down
///
/// Starts recording if PTT mode is enabled and not already recording.
/// Emits `ptt-started` event to the frontend.
#[tauri::command]
pub fn ptt_key_down(app: AppHandle) -> Result<String, String> {
    if !PTT_MODE_ENABLED.load(Ordering::SeqCst) {
        return Err("PTT mode is not enabled".to_string());
    }

    if PTT_ACTIVE.load(Ordering::SeqCst) {
        // Already recording, ignore duplicate key down events
        return Ok(String::new());
    }

    PTT_ACTIVE.store(true, Ordering::SeqCst);
    tracing::info!("PTT key pressed - starting recording");

    // Start recording
    let path = crate::audio::start_recording()?;

    // Emit event to frontend
    app.emit("ptt-started", &path)
        .map_err(|e| format!("Failed to emit ptt-started event: {}", e))?;

    Ok(path)
}

/// Handle PTT key being released
///
/// Stops recording and returns the audio file path for transcription.
/// Emits `ptt-stopped` event to the frontend.
#[tauri::command]
pub fn ptt_key_up(app: AppHandle) -> Result<String, String> {
    if !PTT_ACTIVE.load(Ordering::SeqCst) {
        // Not recording, ignore
        return Ok(String::new());
    }

    PTT_ACTIVE.store(false, Ordering::SeqCst);
    tracing::info!("PTT key released - stopping recording");

    // Stop recording and get audio file path
    let path = crate::audio::stop_recording()?;

    // Emit event to frontend with the audio path
    app.emit("ptt-stopped", &path)
        .map_err(|e| format!("Failed to emit ptt-stopped event: {}", e))?;

    Ok(path)
}

/// Check if PTT is currently active (key held down, recording in progress)
#[tauri::command]
pub fn is_ptt_active() -> bool {
    PTT_ACTIVE.load(Ordering::SeqCst)
}

/// Enable or disable PTT mode
///
/// When PTT mode is enabled, the recording shortcut acts as push-to-talk.
/// When disabled, it acts as a toggle (press to start, press again to stop).
#[tauri::command]
pub fn set_ptt_mode_enabled(enabled: bool) -> Result<(), String> {
    let previous = PTT_MODE_ENABLED.swap(enabled, Ordering::SeqCst);
    if previous != enabled {
        tracing::info!("PTT mode {}", if enabled { "enabled" } else { "disabled" });
    }
    Ok(())
}

/// Check if PTT mode is enabled
#[tauri::command]
pub fn is_ptt_mode_enabled() -> bool {
    PTT_MODE_ENABLED.load(Ordering::SeqCst)
}

/// Cancel PTT recording without transcribing
///
/// Useful if the user wants to abort the current recording.
#[tauri::command]
pub fn ptt_cancel(app: AppHandle) -> Result<(), String> {
    if !PTT_ACTIVE.load(Ordering::SeqCst) {
        return Ok(());
    }

    PTT_ACTIVE.store(false, Ordering::SeqCst);
    tracing::info!("PTT recording cancelled");

    // Stop recording but discard the result
    let _ = crate::audio::stop_recording();

    // Emit cancellation event
    app.emit("ptt-cancelled", ())
        .map_err(|e| format!("Failed to emit ptt-cancelled event: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ptt_mode_toggle() {
        // Reset state
        PTT_MODE_ENABLED.store(false, Ordering::SeqCst);

        assert!(!is_ptt_mode_enabled());

        set_ptt_mode_enabled(true).unwrap();
        assert!(is_ptt_mode_enabled());

        set_ptt_mode_enabled(false).unwrap();
        assert!(!is_ptt_mode_enabled());
    }

    #[test]
    fn test_ptt_active_state() {
        // Reset state
        PTT_ACTIVE.store(false, Ordering::SeqCst);

        assert!(!is_ptt_active());

        // Simulate key down
        PTT_ACTIVE.store(true, Ordering::SeqCst);
        assert!(is_ptt_active());

        // Simulate key up
        PTT_ACTIVE.store(false, Ordering::SeqCst);
        assert!(!is_ptt_active());
    }
}
