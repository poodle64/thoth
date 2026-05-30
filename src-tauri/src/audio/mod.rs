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
pub mod vad_recorder;

pub use capture::AudioRecorder;
pub use device::{get_device_display_name, get_recording_device, list_input_devices, AudioDevice};
pub use format::AudioConverter;
pub use metering::{AudioLevel, AudioMeter};
pub use preview::{start_recording_metering, stop_recording_metering};
pub use ring_buffer::AudioRingBuffer;
pub use vad::{
    VadAggressiveness, VadConfig, VadError, VadEvent, VadFrameDuration, VadSpeechState, VadStatus,
    VoiceActivityDetector,
};
pub use vad_recorder::{VadEventReceiver, VadRecorder};

use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use tauri::{AppHandle, Emitter};

/// How long to keep the warm stream alive after the last recording (seconds).
const IDLE_TEARDOWN_SECS: u64 = 45;

/// Global recorder instance
static RECORDER: OnceLock<Mutex<AudioRecorder>> = OnceLock::new();

/// Metering ring buffer for the current warm session (cleared on cool_down).
static METERING_BUFFER: OnceLock<Mutex<Option<Arc<AudioRingBuffer>>>> = OnceLock::new();

/// Global VAD-enabled recorder instance
static VAD_RECORDER: OnceLock<Mutex<VadRecorder>> = OnceLock::new();

/// Global VAD configuration (thread-safe)
static VAD_CONFIG: OnceLock<Mutex<VadConfig>> = OnceLock::new();

/// Global VAD enabled flag (atomic for lock-free access)
static VAD_ENABLED: OnceLock<Arc<AtomicBool>> = OnceLock::new();

/// Output path for current VAD recording
static VAD_RECORDING_PATH: OnceLock<Mutex<Option<std::path::PathBuf>>> = OnceLock::new();

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

fn get_vad_recorder() -> &'static Mutex<VadRecorder> {
    VAD_RECORDER.get_or_init(|| Mutex::new(VadRecorder::default()))
}

fn get_vad_recording_path() -> &'static Mutex<Option<std::path::PathBuf>> {
    VAD_RECORDING_PATH.get_or_init(|| Mutex::new(None))
}

fn get_vad_config() -> &'static Mutex<VadConfig> {
    VAD_CONFIG.get_or_init(|| Mutex::new(VadConfig::default()))
}

fn get_vad_enabled() -> &'static Arc<AtomicBool> {
    VAD_ENABLED.get_or_init(|| Arc::new(AtomicBool::new(false)))
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

    let path = if use_warm {
        // Disarm only — stream stays warm for IDLE_TEARDOWN_SECS.
        let p = recorder.disarm().map_err(|e| e.to_string())?;

        // Bump the generation and schedule an idle teardown.
        let gen = IDLE_GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        spawn_idle_teardown(gen);
        p
    } else {
        // Cold path: close the device immediately.
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

// =============================================================================
// Voice Activity Detection (VAD) Commands
// =============================================================================

/// Enable or disable VAD
#[tauri::command]
pub fn set_vad_enabled(enabled: bool) {
    get_vad_enabled().store(enabled, Ordering::Relaxed);
    tracing::info!("VAD enabled: {}", enabled);
}

/// Check if VAD is enabled
#[tauri::command]
pub fn is_vad_enabled() -> bool {
    get_vad_enabled().load(Ordering::Relaxed)
}

/// Get the current VAD configuration
#[tauri::command]
pub fn get_vad_config_cmd() -> VadConfig {
    get_vad_config().lock().clone()
}

/// Update VAD configuration
#[tauri::command]
pub fn set_vad_config_cmd(config: VadConfig) -> Result<(), String> {
    // Validate configuration
    if config.speech_start_frames == 0 {
        return Err("speech_start_frames must be at least 1".to_string());
    }
    if config.speech_end_frames == 0 {
        return Err("speech_end_frames must be at least 1".to_string());
    }

    *get_vad_config().lock() = config;
    tracing::info!("VAD configuration updated");
    Ok(())
}

/// Update VAD aggressiveness mode
#[tauri::command]
pub fn set_vad_aggressiveness(aggressiveness: VadAggressiveness) {
    get_vad_config().lock().aggressiveness = aggressiveness;
    tracing::info!("VAD aggressiveness set to: {:?}", aggressiveness);
}

/// Update VAD frame duration
#[tauri::command]
pub fn set_vad_frame_duration(frame_duration: VadFrameDuration) {
    get_vad_config().lock().frame_duration = frame_duration;
    tracing::info!("VAD frame duration set to: {:?}", frame_duration);
}

/// Update speech start threshold (consecutive speech frames required)
#[tauri::command]
pub fn set_vad_speech_start_frames(frames: u32) -> Result<(), String> {
    if frames == 0 {
        return Err("speech_start_frames must be at least 1".to_string());
    }
    get_vad_config().lock().speech_start_frames = frames;
    tracing::info!("VAD speech start frames set to: {}", frames);
    Ok(())
}

/// Update speech end threshold (consecutive silence frames required)
#[tauri::command]
pub fn set_vad_speech_end_frames(frames: u32) -> Result<(), String> {
    if frames == 0 {
        return Err("speech_end_frames must be at least 1".to_string());
    }
    get_vad_config().lock().speech_end_frames = frames;
    tracing::info!("VAD speech end frames set to: {}", frames);
    Ok(())
}

/// Update pre-speech padding duration in milliseconds
#[tauri::command]
pub fn set_vad_pre_speech_padding(padding_ms: u32) {
    get_vad_config().lock().pre_speech_padding_ms = padding_ms;
    tracing::info!("VAD pre-speech padding set to: {}ms", padding_ms);
}

/// Update post-speech padding duration in milliseconds
#[tauri::command]
pub fn set_vad_post_speech_padding(padding_ms: u32) {
    get_vad_config().lock().post_speech_padding_ms = padding_ms;
    tracing::info!("VAD post-speech padding set to: {}ms", padding_ms);
}

/// Get comprehensive VAD status
#[tauri::command]
pub fn get_vad_status() -> VadStatus {
    VadStatus {
        enabled: get_vad_enabled().load(Ordering::Relaxed),
        state: VadSpeechState::Silence, // Actual state is tracked in the VAD processor thread
        config: get_vad_config().lock().clone(),
    }
}

/// Returns a reference to the VAD enabled flag for use in other modules
pub fn vad_enabled_handle() -> Arc<AtomicBool> {
    get_vad_enabled().clone()
}

/// Returns a clone of the current VAD configuration for use in other modules
pub fn current_vad_config() -> VadConfig {
    get_vad_config().lock().clone()
}

// =============================================================================
// VAD-Enabled Recording Commands
// =============================================================================

/// Start recording with VAD processing enabled
///
/// This command starts recording and processes audio through VAD in real-time.
/// VAD events (speech_start, speech_end, auto_stop_triggered) are emitted to
/// the frontend via Tauri events.
///
/// If auto-stop is enabled in VAD config, recording will automatically stop
/// after the configured silence duration following detected speech.
#[tauri::command]
pub fn start_recording_with_vad(app: AppHandle) -> Result<String, String> {
    // Check if VAD is enabled
    if !get_vad_enabled().load(Ordering::Relaxed) {
        return Err("VAD is not enabled. Enable VAD first with set_vad_enabled(true)".to_string());
    }

    let mut recorder = get_vad_recorder().lock();

    if recorder.is_recording() {
        return Err("Recording already in progress".to_string());
    }

    // Update recorder with current VAD config
    recorder.set_config(get_vad_config().lock().clone());

    // Generate temp file path
    let temp_dir = std::env::temp_dir();
    let filename = format!(
        "thoth_vad_recording_{}.wav",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let output_path = temp_dir.join(&filename);

    // Store the output path
    *get_vad_recording_path().lock() = Some(output_path.clone());

    // Get the configured device from config
    let config = crate::config::get_config().map_err(|e| format!("Failed to get config: {}", e))?;
    let device_id = config.audio.device_id.as_deref();

    let audio_device = device::get_recording_device(device_id)
        .ok_or_else(|| "No audio input device available".to_string())?;

    recorder
        .start(&audio_device, &output_path)
        .map_err(|e| e.to_string())?;

    // Get the event receiver and spawn a thread to forward events to Tauri
    let event_rx = recorder.event_receiver();
    let app_handle = app.clone();

    std::thread::spawn(move || {
        while let Ok(event) = event_rx.recv() {
            tracing::debug!("Forwarding VAD event to frontend: {:?}", event);
            if let Err(e) = app_handle.emit("vad-event", &event) {
                tracing::warn!("Failed to emit VAD event: {}", e);
            }

            // If auto-stop triggered, also emit a specific event for easier handling
            if matches!(event, VadEvent::AutoStopTriggered { .. }) {
                if let Err(e) = app_handle.emit("vad-auto-stop", &event) {
                    tracing::warn!("Failed to emit VAD auto-stop event: {}", e);
                }
            }
        }
    });

    tracing::info!("VAD-enabled recording started: {}", output_path.display());
    Ok(output_path.to_string_lossy().to_string())
}

/// Stop VAD-enabled recording and return the path to the recorded file
#[tauri::command]
pub fn stop_recording_with_vad() -> Result<String, String> {
    let mut recorder = get_vad_recorder().lock();

    if !recorder.is_recording() {
        return Err("No VAD recording in progress".to_string());
    }

    let path = recorder.stop().map_err(|e| e.to_string())?;

    // Clear the stored path
    *get_vad_recording_path().lock() = None;

    Ok(path.to_string_lossy().to_string())
}

/// Check if VAD-enabled recording is in progress
#[tauri::command]
pub fn is_recording_with_vad() -> bool {
    get_vad_recorder().lock().is_recording()
}

/// Check if auto-stop was triggered for the current VAD recording
#[tauri::command]
pub fn was_vad_auto_stop_triggered() -> bool {
    get_vad_recorder().lock().auto_stop_triggered()
}

/// Set auto-stop silence duration in milliseconds
///
/// Set to None to disable auto-stop.
#[tauri::command]
pub fn set_vad_auto_stop_silence(silence_ms: Option<u32>) {
    get_vad_config().lock().auto_stop_silence_ms = silence_ms;
    tracing::info!("VAD auto-stop silence set to: {:?}ms", silence_ms);
}
