//! Audio preview for device selection
//!
//! Provides audio level metering for previewing input devices in settings.
//! This is separate from recording; it only monitors levels for UI feedback.

use super::device::{get_device_display_name, get_recording_device};
use super::metering::AudioMeter;
use cpal::traits::{DeviceTrait, StreamTrait};
use parking_lot::Mutex;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Audio level event emitted to the frontend
#[derive(Debug, Clone, Serialize)]
pub struct AudioLevelEvent {
    /// RMS level, normalised 0.0-1.0
    pub rms: f32,
    /// Peak level, normalised 0.0-1.0
    pub peak: f32,
}

/// Shared state for an audio metering stream (preview or recording indicator)
struct MeteringState {
    stream: Option<cpal::Stream>,
    stop_flag: Arc<AtomicBool>,
    emit_handle: Option<std::thread::JoinHandle<()>>,
}

impl Default for MeteringState {
    fn default() -> Self {
        Self {
            stream: None,
            stop_flag: Arc::new(AtomicBool::new(false)),
            emit_handle: None,
        }
    }
}

/// Global preview state (only one preview can run at a time)
static PREVIEW_STATE: Mutex<Option<MeteringState>> = Mutex::new(None);

/// Start audio preview for a specific device
///
/// Emits `audio-level` events to the frontend with RMS and peak levels.
#[tauri::command]
pub fn start_audio_preview(app: AppHandle, device_id: Option<String>) -> Result<(), String> {
    // Stop any existing preview
    stop_audio_preview_inner();

    // Find the device using stable device IDs
    let device = get_recording_device(device_id.as_deref())
        .ok_or_else(|| "No audio input device available".to_string())?;

    let device_name = get_device_display_name(&device);
    tracing::info!("Starting audio preview for device: {}", device_name);

    let config = device.default_input_config().map_err(|e| e.to_string())?;
    let channels = config.channels() as usize;

    // Shared state for metering
    let meter = Arc::new(Mutex::new(AudioMeter::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Channel for audio data
    let (tx, rx) = crossbeam_channel::bounded::<Vec<f32>>(16);

    // Build the input stream
    let stream = {
        let tx = tx.clone();
        device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    // Mix to mono and send to emitter thread
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                        .collect();
                    let _ = tx.try_send(mono);
                },
                |err| {
                    tracing::error!("Audio preview stream error: {}", err);
                },
                None,
            )
            .map_err(|e| e.to_string())?
    };

    stream.play().map_err(|e| e.to_string())?;

    // Spawn emitter thread to send levels to frontend
    let emit_stop_flag = stop_flag.clone();
    let emit_handle = std::thread::spawn(move || {
        while !emit_stop_flag.load(Ordering::Relaxed) {
            while let Ok(samples) = rx.try_recv() {
                let mut meter = meter.lock();
                let level = meter.process(&samples);

                let event = AudioLevelEvent {
                    rms: level.rms,
                    peak: level.peak,
                };

                if let Err(e) = app.emit("audio-level", &event) {
                    tracing::warn!("Failed to emit audio level: {}", e);
                }
            }

            // Rate limit to ~30fps
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
    });

    // Store state
    let mut state_guard = PREVIEW_STATE.lock();
    *state_guard = Some(MeteringState {
        stream: Some(stream),
        stop_flag,
        emit_handle: Some(emit_handle),
    });

    tracing::info!("Audio preview started");
    Ok(())
}

/// Stop audio preview
#[tauri::command]
pub fn stop_audio_preview() {
    stop_audio_preview_inner();
}

/// Internal stop function (doesn't require Tauri command context)
fn stop_audio_preview_inner() {
    let mut state_guard = PREVIEW_STATE.lock();

    if let Some(mut state) = state_guard.take() {
        // Signal stop
        state.stop_flag.store(true, Ordering::Relaxed);

        // Drop stream to stop audio callback
        if let Some(stream) = state.stream.take() {
            drop(stream);
        }

        // Wait for emitter thread
        if let Some(handle) = state.emit_handle.take() {
            let _ = handle.join();
        }

        tracing::info!("Audio preview stopped");
    }
}

/// Check if audio preview is currently running
#[tauri::command]
pub fn is_audio_preview_running() -> bool {
    let state_guard = PREVIEW_STATE.lock();
    state_guard.is_some()
}

// =============================================================================
// Recording Metering (for the recording indicator overlay)
// =============================================================================

/// Global recording meter state
static RECORDING_METER_STATE: Mutex<Option<MeteringState>> = Mutex::new(None);

/// Start recording metering - emits `recording-audio-level` events
///
/// This runs alongside recording to provide real-time audio levels for the
/// recording indicator overlay. Uses the same device as recording.
pub fn start_recording_metering(app: AppHandle, device_id: Option<&str>) -> Result<(), String> {
    tracing::info!(
        "Recording metering: starting with device_id={:?}",
        device_id
    );

    // Stop any existing metering
    stop_recording_metering_inner();

    // Find the device using stable device IDs
    let device = get_recording_device(device_id)
        .ok_or_else(|| "No audio input device available".to_string())?;

    let device_name = get_device_display_name(&device);
    tracing::info!("Recording metering: using device '{}'", device_name);

    let config = device.default_input_config().map_err(|e| {
        tracing::error!("Recording metering: failed to get config: {}", e);
        e.to_string()
    })?;
    let channels = config.channels() as usize;
    tracing::info!(
        "Recording metering: config {}Hz, {} channels",
        config.sample_rate(),
        channels
    );

    // Shared state for metering
    let meter = Arc::new(Mutex::new(AudioMeter::new()));
    let stop_flag = Arc::new(AtomicBool::new(false));

    // Channel for audio data
    let (tx, rx) = crossbeam_channel::bounded::<Vec<f32>>(16);

    // Build the input stream
    let stream = {
        let tx = tx.clone();
        device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                        .collect();
                    let _ = tx.try_send(mono);
                },
                |err| {
                    tracing::error!("Recording meter stream error: {}", err);
                },
                None,
            )
            .map_err(|e| {
                tracing::error!("Recording metering: failed to build stream: {}", e);
                e.to_string()
            })?
    };

    stream.play().map_err(|e| {
        tracing::error!("Recording metering: failed to play stream: {}", e);
        e.to_string()
    })?;

    tracing::info!("Recording metering: audio stream active");

    // Spawn emitter thread to send levels to the recording-indicator window
    let emit_stop_flag = stop_flag.clone();
    let emit_handle = std::thread::spawn(move || {
        while !emit_stop_flag.load(Ordering::Relaxed) {
            while let Ok(samples) = rx.try_recv() {
                let mut meter = meter.lock();
                let level = meter.process(&samples);

                let event = AudioLevelEvent {
                    rms: level.rms,
                    peak: level.peak,
                };

                // Try to emit directly to the recording-indicator window, fall back to global
                let emitted =
                    if let Some(indicator_window) = app.get_webview_window("recording-indicator") {
                        indicator_window
                            .emit("recording-audio-level", &event)
                            .is_ok()
                    } else {
                        false
                    };

                if !emitted {
                    let _ = app.emit("recording-audio-level", &event);
                }
            }

            // Rate limit to ~30fps
            std::thread::sleep(std::time::Duration::from_millis(33));
        }
    });

    // Store state
    let mut state_guard = RECORDING_METER_STATE.lock();
    *state_guard = Some(MeteringState {
        stream: Some(stream),
        stop_flag,
        emit_handle: Some(emit_handle),
    });

    tracing::info!("Recording metering started");
    Ok(())
}

/// Stop recording metering
pub fn stop_recording_metering() {
    stop_recording_metering_inner();
}

/// Internal stop function
fn stop_recording_metering_inner() {
    let mut state_guard = RECORDING_METER_STATE.lock();

    if let Some(mut state) = state_guard.take() {
        // Signal stop
        state.stop_flag.store(true, Ordering::Relaxed);

        // Drop stream to stop audio callback
        if let Some(stream) = state.stream.take() {
            drop(stream);
        }

        // Wait for emitter thread
        if let Some(handle) = state.emit_handle.take() {
            let _ = handle.join();
        }

        tracing::debug!("Recording metering stopped");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_recording_device_nonexistent() {
        // Should fall back to default when device ID doesn't exist
        let device = get_recording_device(Some("Nonexistent Device 12345"));
        // May or may not have a default device, but shouldn't panic
        let _ = device;
    }

    #[test]
    fn test_metering_state_default() {
        let state = MeteringState::default();
        assert!(state.stream.is_none());
        assert!(!state.stop_flag.load(Ordering::Relaxed));
    }
}
