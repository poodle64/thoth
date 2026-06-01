//! Audio subsystem for Thoth
//!
//! Handles audio device enumeration, recording, buffer management, and voice activity detection.

pub mod capture;
pub mod decode;
pub mod device;
pub mod format;
pub mod metering;
pub mod preview;
pub mod ring_buffer;
pub mod vad;

pub use capture::AudioRecorder;
pub use device::{get_device_display_name, get_recording_device, list_input_devices, AudioDevice};
pub use format::AudioConverter;
pub use metering::{AudioLevel, AudioMeter};
pub use preview::{start_recording_metering, stop_recording_metering};
pub use ring_buffer::AudioRingBuffer;

use parking_lot::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

/// How long to keep the warm stream alive after the last recording (seconds).
const IDLE_TEARDOWN_SECS: u64 = 45;

/// Global recorder instance
static RECORDER: OnceLock<Mutex<AudioRecorder>> = OnceLock::new();

/// Metering ring buffer for the current warm session (cleared on cool_down).
static METERING_BUFFER: OnceLock<Mutex<Option<Arc<AudioRingBuffer>>>> = OnceLock::new();

/// Display name of the device used for the most recent (or current) recording.
/// Set inside start_recording so pipeline.rs can read it without a duplicate
/// device resolution call.
static LAST_DEVICE_NAME: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn get_last_device_name() -> &'static Mutex<Option<String>> {
    LAST_DEVICE_NAME.get_or_init(|| Mutex::new(None))
}

/// Idle-teardown generation counter.
///
/// Every `stop_recording` call bumps this. The teardown thread captures the
/// generation it was spawned for and aborts if it has been superseded by a newer
/// recording (meaning the user recorded again before the 45s timer fired).
static IDLE_GENERATION: AtomicU64 = AtomicU64::new(0);

fn get_recorder() -> &'static Mutex<AudioRecorder> {
    RECORDER.get_or_init(|| Mutex::new(AudioRecorder::new()))
}

fn get_metering_buffer() -> &'static Mutex<Option<Arc<AudioRingBuffer>>> {
    METERING_BUFFER.get_or_init(|| Mutex::new(None))
}

/// Return a clone of the current recording's metering ring buffer, if one is active.
///
/// Used by the recording metering emitter to read samples without opening a second device stream.
pub fn current_metering_buffer() -> Option<Arc<AudioRingBuffer>> {
    get_metering_buffer().lock().clone()
}

/// Return the display name of the device used for the most recent recording.
///
/// Set atomically inside `start_recording` so callers do not need to perform
/// a separate (expensive) CoreAudio device resolution.
pub fn last_device_name() -> Option<String> {
    get_last_device_name().lock().clone()
}

/// Cool down the global recorder (close the warm stream).
///
/// Called on device change or sleep/wake. Safe to call when already cooled down.
pub fn cool_down_recording() {
    get_recorder().lock().cool_down();
    *get_metering_buffer().lock() = None;
    tracing::info!("Audio: warm stream cooled down");
}

/// Pre-warm the recorder on the configured device without arming.
///
/// Callers may invoke this proactively (e.g., at startup or after device
/// selection) to eliminate the first-record latency entirely.
#[tauri::command]
pub fn warm_up_recording() -> Result<(), String> {
    let config = crate::config::get_config().map_err(|e| format!("Failed to get config: {}", e))?;

    // Only warm if the feature is enabled.
    if !config.audio.warm_stream {
        return Ok(());
    }

    let device_id = config.audio.device_id.as_deref();
    let audio_device = device::get_recording_device(device_id)
        .ok_or_else(|| "No audio input device available".to_string())?;

    let mut recorder = get_recorder().lock();
    if recorder.is_warm() {
        return Ok(());
    }

    // Attach a fresh metering buffer so the indicator can show levels immediately.
    let metering_buf = Arc::new(AudioRingBuffer::new());
    recorder.set_metering_buffer(metering_buf.clone());
    *get_metering_buffer().lock() = Some(metering_buf);

    recorder.warm_up(&audio_device).map_err(|e| e.to_string())?;

    tracing::info!("Audio: pre-warm complete");
    Ok(())
}

/// Spawn the idle-teardown timer.
///
/// After IDLE_TEARDOWN_SECS of inactivity the warm stream is closed. Both
/// `start_recording` and `stop_recording` bump the generation, so any press
/// supersedes a pending teardown. As a hard safety net the timer also refuses
/// to tear down while a recording is actually armed — tearing down mid-capture
/// would silently kill the recording (data loss).
fn spawn_idle_teardown(generation: u64) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(IDLE_TEARDOWN_SECS));
        // If the generation has changed, a newer recording started/stopped — do nothing.
        if IDLE_GENERATION.load(Ordering::Relaxed) != generation {
            tracing::debug!("Idle teardown superseded (generation mismatch), skipping");
            return;
        }
        // Hard safety net: never tear down while capturing. If somehow still
        // recording, skip — the next stop will reschedule a teardown.
        if get_recorder().lock().is_recording() {
            tracing::warn!("Idle teardown fired while recording — skipping to avoid data loss");
            return;
        }
        tracing::info!("Audio: idle timeout — cooling down warm stream");
        cool_down_recording();
    });
}

/// Start recording audio to ~/.thoth/Recordings/
#[tauri::command]
pub fn start_recording() -> Result<String, String> {
    tracing::info!("Audio: start_recording called");
    let mut recorder = get_recorder().lock();

    if recorder.is_recording() {
        tracing::warn!("Audio: Recording already in progress");
        return Err("Recording already in progress".to_string());
    }

    // Bump the idle-teardown generation so any pending teardown timer (scheduled
    // by a previous stop) is invalidated. Without this, a teardown timer from an
    // earlier recording could fire DURING this new recording and tear down the
    // warm stream mid-capture — silently killing the recording (data loss).
    IDLE_GENERATION.fetch_add(1, Ordering::Relaxed);

    // Generate output path in ~/.thoth/Recordings/
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let recordings_dir = home.join(".thoth").join("Recordings");
    std::fs::create_dir_all(&recordings_dir)
        .map_err(|e| format!("Failed to create recordings directory: {}", e))?;

    let filename = format!(
        "thoth_recording_{}.wav",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let output_path = recordings_dir.join(&filename);

    tracing::info!("Recording will be saved to: {}", output_path.display());

    let config = crate::config::get_config().map_err(|e| format!("Failed to get config: {}", e))?;
    let use_warm = config.audio.warm_stream;
    let device_id = config.audio.device_id.clone();

    if use_warm {
        // Warm path: when the stream is already warm we do NOT resolve the
        // device again — the open stream already holds the correct device.
        // Device resolution calls default_input_config() on CoreAudio and costs
        // ~85-170ms; skipping it on the warm path is what makes repeat records
        // feel instant. The device is only resolved on the cold warm-up below.
        if !recorder.is_warm() {
            tracing::info!("Audio: stream not warm — opening device (first record after idle)");
            let audio_device = device::get_recording_device(device_id.as_deref())
                .ok_or_else(|| "No audio input device available".to_string())?;
            // Store device name for pipeline.rs to read without a second resolution.
            *get_last_device_name().lock() = Some(device::get_device_display_name(&audio_device));
            // Metering buffer must be set before warm_up so the callback captures it.
            let metering_buf = Arc::new(AudioRingBuffer::new());
            recorder.set_metering_buffer(metering_buf.clone());
            *get_metering_buffer().lock() = Some(metering_buf);

            recorder.warm_up(&audio_device).map_err(|e| e.to_string())?;
        } else {
            tracing::info!("Audio: stream already warm — instant start");
        }

        // Arm: instant flag flip + writer thread spawn.
        recorder.arm(&output_path).map_err(|e| e.to_string())?;
    } else {
        // Cold path (warm_stream disabled): open/close on every record.
        let audio_device = device::get_recording_device(device_id.as_deref())
            .ok_or_else(|| "No audio input device available".to_string())?;
        // Store device name for pipeline.rs to read without a second resolution.
        *get_last_device_name().lock() = Some(device::get_device_display_name(&audio_device));
        let metering_buf = Arc::new(AudioRingBuffer::new());
        recorder.set_metering_buffer(metering_buf.clone());
        *get_metering_buffer().lock() = Some(metering_buf);

        recorder
            .start(&audio_device, &output_path)
            .map_err(|e| e.to_string())?;
    }

    Ok(output_path.to_string_lossy().to_string())
}

/// Stop recording and return the path to the recorded file
#[tauri::command]
pub fn stop_recording() -> Result<String, String> {
    let mut recorder = get_recorder().lock();

    if !recorder.is_recording() {
        return Err("No recording in progress".to_string());
    }

    let config = crate::config::get_config().map_err(|e| format!("Failed to get config: {}", e))?;
    let use_warm = config.audio.warm_stream;

    // Determine whether the device we ACTUALLY recorded from is Bluetooth, by
    // checking the transport type of the device named in LAST_DEVICE_NAME. This
    // is correct even when the system default differs from the recording device
    // — e.g. when the default input is AirPods but recording was redirected to
    // the built-in mic (get_recording_device's Bluetooth-avoidance). Querying
    // the *default* input here would wrongly report Bluetooth and cool down the
    // built-in stream, losing its warm-stream latency benefit.
    let recording_is_bluetooth = get_last_device_name()
        .lock()
        .as_deref()
        .map(crate::platform::device_name_is_bluetooth)
        .unwrap_or(false);

    let path = if use_warm {
        let p = recorder.disarm().map_err(|e| e.to_string())?;

        if recording_is_bluetooth {
            // Never hold a Bluetooth input stream warm — that pins the
            // device in HFP call mode and degrades the user's audio.
            tracing::info!("Audio: recording device is Bluetooth — closing stream immediately instead of warming");
            recorder.cool_down();
            *get_metering_buffer().lock() = None;
            // Bump generation so any pre-existing teardown timer aborts.
            IDLE_GENERATION.fetch_add(1, Ordering::Relaxed);
        } else {
            // Built-in or USB device: keep warm for IDLE_TEARDOWN_SECS.
            let gen = IDLE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
            spawn_idle_teardown(gen);
        }
        p
    } else {
        // Cold path: close the device immediately regardless of transport.
        recorder.clear_metering_buffer();
        *get_metering_buffer().lock() = None;
        recorder.stop().map_err(|e| e.to_string())?
    };

    // In both paths the metering buffer is no longer needed by the pipeline
    // caller — the preview emitter has its own reference and will stop when
    // stop_recording_metering() is called by the pipeline.

    Ok(path.to_string_lossy().to_string())
}

/// Check if recording is in progress
#[tauri::command]
pub fn is_recording() -> bool {
    get_recorder().lock().is_recording()
}
