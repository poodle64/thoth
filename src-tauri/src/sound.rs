//! Sound feedback module for Thoth
//!
//! Provides audio feedback sounds for recording events.
//! Uses macOS dictation-style tones (dt-begin, dt-confirm) for recording
//! start/stop, and standard system sounds for other events. On Linux the
//! equivalent cues come from the freedesktop XDG sound theme (see
//! [`SoundEvent::theme_sound_names`]).
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
    #[cfg(target_os = "macos")]
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

    /// The tone sequence for this event on Linux, as `(frequency Hz, duration ms)`
    /// segments.
    ///
    /// Linux has no system cue equivalent to the macOS dictation tones, and no
    /// asset or sound-theme package is guaranteed to be installed on an arbitrary
    /// distribution, so the cues are synthesised (see [`render_cue`]). Start and
    /// stop are the same interval inverted — rising for "on", falling for "off" —
    /// which is the semantic the macOS `dt-begin` / `dt-confirm` pair carries.
    #[cfg(target_os = "linux")]
    fn cue(&self) -> &'static [(f32, u32)] {
        match self {
            SoundEvent::RecordingStart => &[(660.0, 80), (990.0, 90)],
            SoundEvent::RecordingStop => &[(990.0, 80), (660.0, 90)],
            SoundEvent::TranscriptionComplete => &[(880.0, 70), (1174.0, 70), (1318.0, 110)],
            SoundEvent::Error => &[(320.0, 110), (240.0, 160)],
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

    #[cfg(target_os = "macos")]
    {
        play_macos_sound(event.sound_path());
    }

    #[cfg(target_os = "linux")]
    {
        play_linux_sound(event);
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        tracing::debug!(
            "System sounds not implemented for this platform, skipping {:?}",
            event
        );
    }
}

/// Peak amplitude of a rendered cue. Quiet enough not to startle over headphones,
/// loud enough to hear over typing.
#[cfg(target_os = "linux")]
const CUE_AMPLITUDE: f32 = 0.22;

/// Attack/release ramp applied to each cue segment, in seconds. Without it the
/// waveform starts and stops at a non-zero value and the speaker clicks.
#[cfg(target_os = "linux")]
const CUE_RAMP_SECS: f32 = 0.006;

/// Render a `(frequency Hz, duration ms)` cue to mono `f32` samples at
/// `sample_rate`.
///
/// Each segment is an enveloped sine: a linear fade in and out so segment edges
/// (and the cue's start and end) are click-free.
#[cfg(target_os = "linux")]
fn render_cue(segments: &[(f32, u32)], sample_rate: f32) -> Vec<f32> {
    let mut samples = Vec::new();

    for &(frequency, duration_ms) in segments {
        let frames = ((sample_rate * duration_ms as f32) / 1000.0).round() as usize;
        if frames == 0 {
            continue;
        }
        // Never let attack and release overlap on a very short segment.
        let ramp = ((sample_rate * CUE_RAMP_SECS) as usize).clamp(1, (frames / 2).max(1));

        for i in 0..frames {
            let envelope = if i < ramp {
                i as f32 / ramp as f32
            } else if i + ramp >= frames {
                (frames - i) as f32 / ramp as f32
            } else {
                1.0
            };
            let phase = std::f32::consts::TAU * frequency * (i as f32 / sample_rate);
            samples.push(phase.sin() * envelope * CUE_AMPLITUDE);
        }
    }

    samples
}

/// Build an output stream that plays `samples` (mono) across every channel,
/// then silence.
#[cfg(target_os = "linux")]
fn build_cue_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    samples: Vec<f32>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;

    let channels = config.channels as usize;
    let mut cursor = 0usize;

    device
        .build_output_stream::<T, _, _>(
            *config,
            move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
                for frame in out.chunks_mut(channels) {
                    let value = samples.get(cursor).copied().unwrap_or(0.0);
                    cursor = cursor.saturating_add(1);
                    // Mono cue duplicated to every channel.
                    for slot in frame.iter_mut() {
                        *slot = T::from_sample(value);
                    }
                }
            },
            |e| tracing::warn!("Audio cue output stream error: {}", e),
            None,
        )
        .map_err(|e| format!("failed to build output stream: {e}"))
}

/// Play a UI cue on Linux by synthesising it and writing it to the default
/// output device via cpal.
///
/// Deliberately not the freedesktop sound theme or a `paplay`/`canberra` style
/// helper: both require a package that plenty of distributions and minimal
/// window-manager setups do not ship, which would make the cue silent on exactly
/// the systems least likely to have it. cpal is already a dependency (it drives
/// capture) and speaks ALSA/PulseAudio/PipeWire/JACK, so synthesising in-process
/// is the one path that behaves identically on every Linux install and needs no
/// bundled asset.
#[cfg(target_os = "linux")]
fn play_cue(segments: &'static [(f32, u32)]) -> Result<(), String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use std::time::Duration;

    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no default audio output device".to_string())?;
    let supported = device
        .default_output_config()
        .map_err(|e| format!("no default output config: {e}"))?;

    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();
    let sample_rate = config.sample_rate as f32;

    let samples = render_cue(segments, sample_rate);
    if samples.is_empty() {
        return Ok(());
    }
    let duration = Duration::from_secs_f32(samples.len() as f32 / sample_rate);

    let stream = match sample_format {
        cpal::SampleFormat::F32 => build_cue_stream::<f32>(&device, &config, samples),
        cpal::SampleFormat::I16 => build_cue_stream::<i16>(&device, &config, samples),
        cpal::SampleFormat::U16 => build_cue_stream::<u16>(&device, &config, samples),
        other => Err(format!("unsupported output sample format {other:?}")),
    }?;

    stream
        .play()
        .map_err(|e| format!("failed to start cue playback: {e}"))?;

    // Dropping the stream stops playback, so hold it open for the cue's length.
    // The tail lets the device drain its last buffer before the drop.
    std::thread::sleep(duration + Duration::from_millis(60));
    Ok(())
}

/// Play a UI cue on Linux.
///
/// Runs on a detached thread: building the stream and holding it open for the
/// cue's duration both block, and the start cue fires on the hotkey path where
/// any delay is audible as lag.
#[cfg(target_os = "linux")]
fn play_linux_sound(event: SoundEvent) {
    let segments = event.cue();
    std::thread::spawn(move || {
        if let Err(e) = play_cue(segments) {
            tracing::warn!("Failed to play {:?} cue: {}", event, e);
        }
    });
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

    #[cfg(target_os = "macos")]
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

    /// Start and stop must not sound the same, or the user cannot tell by ear
    /// whether recording began or ended — the whole point of the pair. They are
    /// the same interval inverted: rising for on, falling for off.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_start_and_stop_cues_are_inverses() {
        let start = SoundEvent::RecordingStart.cue();
        let stop = SoundEvent::RecordingStop.cue();

        assert_ne!(start, stop);
        assert!(
            start[0].0 < start[1].0,
            "start cue should rise, got {start:?}"
        );
        assert!(stop[0].0 > stop[1].0, "stop cue should fall, got {stop:?}");
        // Same two pitches, opposite order.
        assert_eq!(start[0].0, stop[1].0);
        assert_eq!(start[1].0, stop[0].0);
    }

    /// Every event must produce an audible, non-empty cue at any plausible
    /// device sample rate — a silent cue is indistinguishable from the bug this
    /// replaced.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_every_event_renders_audible_samples() {
        for rate in [8_000.0, 44_100.0, 48_000.0, 192_000.0] {
            for event in [
                SoundEvent::RecordingStart,
                SoundEvent::RecordingStop,
                SoundEvent::TranscriptionComplete,
                SoundEvent::Error,
            ] {
                let samples = render_cue(event.cue(), rate);
                assert!(!samples.is_empty(), "{event:?} rendered nothing at {rate}");

                let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
                assert!(peak > 0.05, "{event:?} is inaudible at {rate}: peak {peak}");
                assert!(
                    peak <= CUE_AMPLITUDE + 1e-6,
                    "{event:?} exceeds the amplitude ceiling at {rate}: peak {peak}"
                );
                assert!(
                    samples.iter().all(|s| s.is_finite()),
                    "{event:?} produced a non-finite sample at {rate}"
                );
            }
        }
    }

    /// The envelope must start and end at silence, or the speaker clicks.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_cue_fades_in_and_out() {
        let samples = render_cue(SoundEvent::RecordingStart.cue(), 48_000.0);

        assert!(
            samples[0].abs() < 1e-6,
            "cue does not start at silence: {}",
            samples[0]
        );
        let last = samples[samples.len() - 1];
        assert!(last.abs() < 0.02, "cue does not fade out: {last}");
    }

    /// Duration must track the requested segment lengths, so a cue can't
    /// silently become a click or run long enough to overlap the next one.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_cue_duration_matches_segments() {
        const RATE: f32 = 48_000.0;

        for event in [SoundEvent::RecordingStart, SoundEvent::RecordingStop] {
            let expected_ms: u32 = event.cue().iter().map(|(_, ms)| ms).sum();
            let frames = render_cue(event.cue(), RATE).len() as f32;
            let actual_ms = (frames / RATE * 1000.0).round() as u32;
            assert!(
                actual_ms.abs_diff(expected_ms) <= 1,
                "{event:?}: expected ~{expected_ms}ms, rendered {actual_ms}ms"
            );
        }
    }

    /// A zero-length segment must not panic or emit samples.
    #[cfg(target_os = "linux")]
    #[test]
    fn test_render_cue_handles_degenerate_input() {
        assert!(render_cue(&[], 48_000.0).is_empty());
        assert!(render_cue(&[(440.0, 0)], 48_000.0).is_empty());
        // A one-frame segment is inaudible but must not panic on the ramp maths.
        let _ = render_cue(&[(440.0, 1)], 1_000.0);
    }

    /// Audible end-to-end check against the real default output device.
    /// Ignored by default: CI has no audio server.
    ///
    /// Run with: `cargo test --lib sound -- --ignored --nocapture`
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a running audio server and an output device"]
    fn manual_play_recording_cues() {
        for event in [SoundEvent::RecordingStart, SoundEvent::RecordingStop] {
            println!("playing {event:?}: {:?}", event.cue());
            play_cue(event.cue()).unwrap_or_else(|e| panic!("{event:?} failed to play: {e}"));
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
}
