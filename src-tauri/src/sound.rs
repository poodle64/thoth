//! Sound feedback module for Thoth
//!
//! Provides audio feedback sounds for recording events.
//! Uses macOS dictation-style tones (dt-begin, dt-confirm) for recording
//! start/stop, and standard system sounds for other events.
//!
//! Playback uses the macOS System Sound server (`AudioServicesPlaySystemSound`),
//! not NSSound. The system-sound server runs in a SEPARATE process with its own
//! output path, so a cue is never clipped or swallowed when the app
//! simultaneously opens a CoreAudio input device to record — the failure mode
//! NSSound had (the start tone went silent or "cut in half" on the first record
//! after the warm audio stream had torn down). This is the same mechanism macOS
//! dictation itself uses for its on/off cues, so playback is decoupled from when
//! or how recently the user pressed record.

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

/// Cache of registered System Sound IDs, keyed by file path.
///
/// Each sound file is registered with the system-sound server exactly once
/// (`AudioServicesCreateSystemSoundID` does real work) and the ID reused for
/// every subsequent play. IDs are never disposed — Thoth has a small fixed set
/// of cues and the OS reclaims them at exit; disposing would also race the
/// asynchronous playback.
#[cfg(target_os = "macos")]
static SOUND_CACHE: std::sync::OnceLock<
    std::sync::Mutex<std::collections::HashMap<&'static str, u32>>,
> = std::sync::OnceLock::new();

/// Play a short UI sound via the macOS System Sound server (fire-and-forget).
///
/// Playback runs in a separate process, so it is NOT clipped when cpal holds a
/// CoreAudio *input* device open — the reason NSSound's start tone failed on a
/// cold record.
#[cfg(target_os = "macos")]
fn play_macos_sound(path: &'static str) {
    use objc2_audio_toolbox::{AudioServicesCreateSystemSoundID, AudioServicesPlaySystemSound};
    use objc2_core_foundation::{CFString, CFURLPathStyle, CFURL};

    let cache = SOUND_CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()));
    let mut map = match cache.lock() {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Sound cache lock poisoned: {}", e);
            return;
        }
    };

    let id = match map.get(path).copied() {
        Some(id) => id,
        None => {
            let cf_path = CFString::from_str(path);
            let Some(url) = CFURL::with_file_system_path(
                None,
                Some(&cf_path),
                CFURLPathStyle::CFURLPOSIXPathStyle,
                false,
            ) else {
                tracing::warn!("Failed to build CFURL for sound: {}", path);
                return;
            };

            let mut sound_id: u32 = 0;
            // SAFETY: `&mut sound_id` is a valid non-null out-param pointer.
            let status = unsafe {
                AudioServicesCreateSystemSoundID(&url, std::ptr::NonNull::from(&mut sound_id))
            };
            if status != 0 {
                tracing::warn!(
                    "AudioServicesCreateSystemSoundID failed ({}) for {}",
                    status,
                    path
                );
                return;
            }
            map.insert(path, sound_id);
            sound_id
        }
    };

    // SAFETY: `id` is a live SystemSoundID created above; play is async/fire-and-forget.
    unsafe { AudioServicesPlaySystemSound(id) };
    tracing::debug!("Playing system sound: {}", path);
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
