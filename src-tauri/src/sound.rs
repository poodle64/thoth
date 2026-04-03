//! Sound feedback module for Thoth
//!
//! Provides audio feedback sounds for recording events.
//! Uses macOS dictation-style tones (dt-begin, dt-confirm) for recording
//! start/stop, and standard system sounds for other events.
//! Playback uses NSSound on macOS for instant, zero-latency feedback.

use crate::config;

/// Sound event types for different application states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundEvent {
    /// Recording has started
    RecordingStart,
    /// Recording has stopped
    RecordingStop,
    /// Transcription completed successfully
    TranscriptionComplete,
    /// An error occurred
    Error,
}

impl SoundEvent {
    /// Get the macOS sound file path for this event
    ///
    /// Recording start/stop use the dictation tones from AssistantServices
    /// (the same sounds macOS plays for dictation on/off).
    /// Other events use standard system sounds from /System/Library/Sounds/.
    fn sound_path(&self) -> &'static str {
        match self {
            SoundEvent::RecordingStart => "/System/Library/PrivateFrameworks/AssistantServices.framework/Versions/A/Resources/dt-begin.caf",
            SoundEvent::RecordingStop => "/System/Library/PrivateFrameworks/AssistantServices.framework/Versions/A/Resources/dt-confirm.caf",
            SoundEvent::TranscriptionComplete => "/System/Library/Sounds/Glass.aiff",
            SoundEvent::Error => "/System/Library/Sounds/Basso.aiff",
        }
    }
}

/// Play a sound for the given event if sounds are enabled in config
pub fn play_sound(event: SoundEvent) {
    // Check if sounds are enabled in config
    let sounds_enabled = match config::get_config() {
        Ok(cfg) => cfg.audio.play_sounds,
        Err(e) => {
            tracing::warn!("Failed to get config for sound check: {}", e);
            true // Default to playing sounds if config fails
        }
    };

    if !sounds_enabled {
        tracing::debug!("Sound disabled, skipping {:?}", event);
        return;
    }

    let path = event.sound_path();

    #[cfg(target_os = "macos")]
    {
        play_macos_sound(path);
    }

    #[cfg(not(target_os = "macos"))]
    {
        tracing::debug!(
            "System sounds not implemented for this platform, skipping {:?}",
            event
        );
        let _ = path; // Suppress unused warning
    }
}

/// Play a macOS sound file using NSSound (instant, no subprocess overhead).
#[cfg(target_os = "macos")]
fn play_macos_sound(path: &str) {
    use objc2::AnyThread;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSString;

    let ns_path = NSString::from_str(path);
    let sound = NSSound::initWithContentsOfFile_byReference(NSSound::alloc(), &ns_path, true);
    match sound {
        Some(s) => {
            s.play();
            // NSSound plays asynchronously. We intentionally leak the reference
            // so the sound finishes playing. The OS reclaims it when playback ends.
            // At ~0.5s per sound and ~4 events max per recording cycle, this is
            // negligible memory (a few KB held briefly).
            std::mem::forget(s);
            tracing::debug!("Playing sound: {}", path);
        }
        None => {
            tracing::warn!("Failed to load sound: {}", path);
        }
    }
}

/// Play a sound for recording start
#[tauri::command]
pub fn play_recording_start_sound() {
    play_sound(SoundEvent::RecordingStart);
}

/// Play a sound for recording stop
#[tauri::command]
pub fn play_recording_stop_sound() {
    play_sound(SoundEvent::RecordingStop);
}

/// Play a sound for transcription complete
#[tauri::command]
pub fn play_transcription_complete_sound() {
    play_sound(SoundEvent::TranscriptionComplete);
}

/// Play a sound for error
#[tauri::command]
pub fn play_error_sound() {
    play_sound(SoundEvent::Error);
}

/// Check if sounds are enabled
#[tauri::command]
pub fn are_sounds_enabled() -> Result<bool, String> {
    let cfg = config::get_config()?;
    Ok(cfg.audio.play_sounds)
}

/// Set sounds enabled state
#[tauri::command]
pub fn set_sounds_enabled(enabled: bool) -> Result<(), String> {
    let mut cfg = config::get_config()?;
    cfg.audio.play_sounds = enabled;
    config::set_config(cfg)?;
    tracing::info!("Sounds enabled: {}", enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sound_event_paths() {
        assert!(SoundEvent::RecordingStart.sound_path().contains("dt-begin"));
        assert!(SoundEvent::RecordingStop
            .sound_path()
            .contains("dt-confirm"));
        assert!(SoundEvent::TranscriptionComplete
            .sound_path()
            .contains("Glass"));
        assert!(SoundEvent::Error.sound_path().contains("Basso"));
    }

    #[test]
    fn test_sound_event_equality() {
        assert_eq!(SoundEvent::RecordingStart, SoundEvent::RecordingStart);
        assert_ne!(SoundEvent::RecordingStart, SoundEvent::RecordingStop);
    }
}
