//! Sound feedback module for Thoth
//!
//! Provides audio feedback sounds for recording events.
//! Uses macOS dictation-style tones (dt-begin, dt-confirm) for recording
//! start/stop, and standard system sounds for other events.
//!
//! Playback uses `AVAudioPlayer` on macOS. This is the path that satisfies both
//! constraints a recording cue has:
//!   - It plays as an ordinary mixable CoreAudio client, so it does NOT duck or
//!     pause the user's music (the System Sound server does — it routes through
//!     the single-slot system-alert path with no mix control on macOS, and
//!     NSSound on the shared output got clipped when the mic opened).
//!   - Each cue is an independent output stream, so opening the microphone to
//!     record does not clip or swallow it (the failure NSSound had: the start
//!     tone went silent or "cut in half" on the first record after the warm
//!     audio stream had torn down).
//!
//! A fresh player is created per cue and leaked for its short lifetime; the OS
//! reclaims it when playback ends.

use crate::config;
use crate::error::Error;

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
            SoundEvent::RecordingStart => {
                "/System/Library/PrivateFrameworks/AssistantServices.framework/Versions/A/Resources/dt-begin.caf"
            }
            SoundEvent::RecordingStop => {
                "/System/Library/PrivateFrameworks/AssistantServices.framework/Versions/A/Resources/dt-confirm.caf"
            }
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

/// Play a short UI sound via `AVAudioPlayer`.
///
/// `AVAudioPlayer` plays as an ordinary mixable CoreAudio client: it does not
/// duck or pause other apps' audio (so the cue no longer interferes with music),
/// and it is an independent output stream so opening the microphone to record
/// does not clip it. A fresh player is created per cue, prepared, played, and
/// leaked for its short lifetime; the OS reclaims it once playback ends.
#[cfg(target_os = "macos")]
fn play_macos_sound(path: &'static str) {
    use objc2::AnyThread;
    use objc2_avf_audio::AVAudioPlayer;
    use objc2_foundation::{NSString, NSURL};

    let ns_path = NSString::from_str(path);
    let url = NSURL::fileURLWithPath(&ns_path);

    // SAFETY: `url` is a valid file URL; init returns None (Err) if the file
    // can't be opened as audio, which we handle.
    let player =
        match unsafe { AVAudioPlayer::initWithContentsOfURL_error(AVAudioPlayer::alloc(), &url) } {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("Failed to load sound {} into AVAudioPlayer: {:?}", path, e);
                return;
            }
        };

    // SAFETY: standard AVAudioPlayer calls; safe to call from any thread.
    unsafe {
        player.prepareToPlay();
        if !player.play() {
            tracing::warn!("AVAudioPlayer failed to start playing {}", path);
            return;
        }
    }

    // Keep the player alive until playback finishes. AVAudioPlayer stops if it
    // is deallocated mid-play, so we leak this short-lived instance (a few KB,
    // reclaimed by the OS when the ~0.5s cue ends), matching the prior cue model.
    std::mem::forget(player);
    tracing::debug!("Playing sound via AVAudioPlayer: {}", path);
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
pub fn are_sounds_enabled() -> Result<bool, Error> {
    let cfg = config::get_config()?;
    Ok(cfg.audio.play_sounds)
}

/// Set sounds enabled state
#[tauri::command]
pub fn set_sounds_enabled(enabled: bool) -> Result<(), Error> {
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
        assert!(
            SoundEvent::RecordingStop
                .sound_path()
                .contains("dt-confirm")
        );
        assert!(
            SoundEvent::TranscriptionComplete
                .sound_path()
                .contains("Glass")
        );
        assert!(SoundEvent::Error.sound_path().contains("Basso"));
    }

    #[test]
    fn test_sound_event_equality() {
        assert_eq!(SoundEvent::RecordingStart, SoundEvent::RecordingStart);
        assert_ne!(SoundEvent::RecordingStart, SoundEvent::RecordingStop);
    }
}
