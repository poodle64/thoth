//! Transcription pipeline orchestration
//!
//! Wires together the complete flow from recording to output:
//! 1. Recording (start/stop via audio module)
//! 2. Transcription (via transcription module)
//! 3. Filtering (dictionary replacements + output filtering)
//! 4. Enhancement (optional AI enhancement via Ollama)
//! 5. Output (clipboard copy and/or paste at cursor)
//! 6. History (save to database)

use crate::clipboard;
use crate::database;
use crate::dictionary;
use crate::enhancement;
use crate::transcription;
use crate::tray;
use cpal::traits::DeviceTrait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter};

/// Pipeline execution state
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineState {
    /// Pipeline is idle, ready for recording
    #[default]
    Idle,
    /// Recording audio
    Recording,
    /// Transcribing audio to text
    Transcribing,
    /// Applying dictionary replacements and filtering
    Filtering,
    /// Enhancing text with AI
    Enhancing,
    /// Converting imported audio to 16kHz mono WAV
    Converting,
    /// Outputting result (clipboard/paste)
    Outputting,
    /// Pipeline completed successfully
    Completed,
    /// Pipeline failed with error
    Failed,
}

/// Pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineConfig {
    /// Whether to apply dictionary replacements
    pub apply_dictionary: bool,
    /// Whether to apply output filtering (filler words, formatting)
    pub apply_filtering: bool,
    /// Whether AI enhancement is enabled
    pub enhancement_enabled: bool,
    /// Ollama model for enhancement
    pub enhancement_model: String,
    /// Enhancement prompt template
    pub enhancement_prompt: String,
    /// Whether to auto-copy to clipboard
    pub auto_copy: bool,
    /// Whether to auto-paste at cursor
    pub auto_paste: bool,
    /// Insertion method: "typing" or "paste"
    pub insertion_method: String,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            apply_dictionary: true,
            apply_filtering: true,
            enhancement_enabled: false,
            enhancement_model: "llama3.2".to_string(),
            enhancement_prompt: DEFAULT_ENHANCEMENT_PROMPT.to_string(),
            auto_copy: true,
            auto_paste: true,
            insertion_method: "paste".to_string(),
        }
    }
}

/// Default enhancement prompt
const DEFAULT_ENHANCEMENT_PROMPT: &str = r#"Fix grammar and punctuation in the following text.
Keep the original meaning and tone. Output only the corrected text, nothing else.

Text: {text}"#;

/// Pipeline execution result
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineResult {
    /// Whether the pipeline completed successfully
    pub success: bool,
    /// Final transcribed text (after all processing)
    pub text: String,
    /// Raw transcription text (before filtering/enhancement)
    pub raw_text: String,
    /// Whether the text was enhanced by AI
    pub is_enhanced: bool,
    /// Duration of the audio in seconds
    pub duration_seconds: Option<f64>,
    /// Path to the audio file
    pub audio_path: Option<String>,
    /// Error message if the pipeline failed
    pub error: Option<String>,
    /// ID of the saved transcription record
    pub transcription_id: Option<String>,
    /// Name of the transcription model used
    pub transcription_model_name: Option<String>,
    /// Time taken to transcribe in seconds
    pub transcription_duration_seconds: Option<f64>,
    /// Name of the enhancement model used
    pub enhancement_model_name: Option<String>,
    /// Time taken for enhancement in seconds
    pub enhancement_duration_seconds: Option<f64>,
}

/// Progress event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineProgress {
    /// Current pipeline state
    pub state: PipelineState,
    /// Progress message for display
    pub message: String,
    /// Audio device name (only present when state is Recording)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
}

/// Track if pipeline is currently running
static PIPELINE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Cancellation signal for file import operations
static IMPORT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// RAII guard that resets PIPELINE_RUNNING to false on drop.
/// Prevents the pipeline from being permanently locked if a command
/// panics or returns early without explicit cleanup.
struct PipelineGuard;

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        PIPELINE_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Start the recording phase of the pipeline
///
/// Emits `pipeline-progress` event with state updates.
/// Also shows the recording indicator overlay and starts audio metering.
#[tauri::command]
pub fn pipeline_start_recording(app: AppHandle) -> Result<String, String> {
    tracing::info!("Pipeline: pipeline_start_recording called");

    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        tracing::warn!("Pipeline: Already running, rejecting start request");
        return Err("Pipeline is already running".to_string());
    }

    // Check transcription model is available before capturing audio
    if !transcription::is_transcription_ready() {
        PIPELINE_RUNNING.store(false, Ordering::SeqCst);
        tracing::warn!("Pipeline: No transcription model available, blocking recording");
        // Hide the recording indicator (shortcut handler may have shown it)
        let _ = crate::recording_indicator::hide_recording_indicator(app.clone());
        return Err(
            "No transcription model downloaded. Open Settings \u{2192} Models to get started."
                .to_string(),
        );
    }

    // Get device ID from config
    let config = crate::config::get_config().ok();
    let device_id = config.as_ref().and_then(|c| c.audio.device_id.clone());

    // Get the device name for display
    let resolved_device = crate::audio::get_recording_device(device_id.as_deref());
    let device_name = resolved_device
        .as_ref()
        .map(crate::audio::device::get_device_display_name);

    // Detect device fallback: configured device didn't match the resolved one
    if let Some(ref configured_id) = device_id {
        let resolved_id = resolved_device
            .as_ref()
            .and_then(|d| d.id().ok())
            .map(|id| id.to_string());
        if resolved_id.as_deref() != Some(configured_id.as_str()) {
            tracing::warn!(
                "Device fallback: configured '{}', using '{}'",
                configured_id,
                device_name.as_deref().unwrap_or("unknown")
            );
            let _ = app.emit(
                "device-fallback-warning",
                serde_json::json!({
                    "configuredId": configured_id,
                    "actualName": device_name.as_deref().unwrap_or("System Default"),
                }),
            );
        }
    }

    // Emit recording state with device name
    emit_progress_with_device(
        &app,
        PipelineState::Recording,
        "Recording audio...",
        device_name.clone(),
    );

    tracing::info!("Pipeline: Calling audio::start_recording");
    // Start recording
    match crate::audio::start_recording() {
        Ok(path) => {
            tracing::info!("Pipeline: Recording started at {}", path);

            // Update tray to show recording state
            tray::set_recording_state(&app, true);

            // NOTE: Recording indicator is shown instantly from the shortcut handler
            // (show_indicator_instant) - no need to show it here again.
            // The indicator window is pre-warmed at startup so no JS init wait needed.

            // Start recording metering AFTER the indicator is visible
            if let Err(e) =
                crate::audio::start_recording_metering(app, device_id.as_deref())
            {
                tracing::warn!("Pipeline: Failed to start recording metering: {}", e);
            }

            Ok(path)
        }
        Err(e) => {
            PIPELINE_RUNNING.store(false, Ordering::SeqCst);
            emit_progress(
                &app,
                PipelineState::Failed,
                &format!("Recording failed: {}", e),
            );
            Err(e)
        }
    }
}

/// Stop recording and run the full transcription pipeline
///
/// Emits `pipeline-progress` events as each stage completes.
/// Emits `pipeline-complete` when finished with the final result.
/// Hides the recording indicator overlay when recording stops.
#[tauri::command]
pub async fn pipeline_stop_and_process(
    app: AppHandle,
    config: Option<PipelineConfig>,
) -> Result<PipelineResult, String> {
    tracing::info!("Pipeline: stop_and_process called");
    let config = config.unwrap_or_default();

    // RAII guard ensures PIPELINE_RUNNING is reset even on early return
    let _guard = PipelineGuard;

    // Stop recording metering
    crate::audio::stop_recording_metering();

    // Update tray to show idle state
    tray::set_recording_state(&app, false);

    // Hide the recording indicator (but keep it visible during processing - it will show spinner)
    // Actually, let's hide it when recording stops since we have processing state in the main window
    if let Err(e) = crate::recording_indicator::hide_recording_indicator(app.clone()) {
        tracing::warn!("Pipeline: Failed to hide recording indicator: {}", e);
    }

    // Stop recording
    let audio_path = match crate::audio::stop_recording() {
        Ok(path) => path,
        Err(e) => {
            emit_progress(
                &app,
                PipelineState::Failed,
                &format!("Stop recording failed: {}", e),
            );
            return Err(e);
        }
    };

    tracing::info!("Pipeline: Recording stopped, processing {}", audio_path);

    // Run the processing pipeline
    let result = process_audio(&app, &audio_path, &config).await;

    // Emit completion event
    match &result {
        Ok(r) => {
            tracing::info!("Pipeline: Emitting pipeline-complete event");
            if let Err(e) = app.emit("pipeline-complete", r) {
                tracing::error!("Pipeline: Failed to emit pipeline-complete: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("Pipeline: Processing failed: {}", e);
            emit_progress(&app, PipelineState::Failed, e);
        }
    }

    tracing::info!("Pipeline: Returning result from stop_and_process");
    result
}

/// Cancel the current pipeline execution
#[tauri::command]
pub fn pipeline_cancel(app: AppHandle) -> Result<(), String> {
    if !PIPELINE_RUNNING.load(Ordering::SeqCst) {
        return Ok(()); // Nothing to cancel
    }

    // Stop recording metering and hide indicator
    crate::audio::stop_recording_metering();
    if let Err(e) = crate::recording_indicator::hide_recording_indicator(app.clone()) {
        tracing::warn!("Pipeline: Failed to hide recording indicator on cancel: {}", e);
    }

    // Signal cancellation for file import operations
    IMPORT_CANCELLED.store(true, Ordering::SeqCst);

    // Stop recording if in progress
    if crate::audio::is_recording() {
        let _ = crate::audio::stop_recording();
    }

    // Reset tray state
    tray::set_recording_state(&app, false);

    PIPELINE_RUNNING.store(false, Ordering::SeqCst);
    emit_progress(&app, PipelineState::Idle, "Pipeline cancelled");
    app.emit("pipeline-cancelled", ()).ok();

    tracing::info!("Pipeline: Cancelled");
    Ok(())
}

/// Check if the pipeline is currently running
#[tauri::command]
pub fn is_pipeline_running() -> bool {
    PIPELINE_RUNNING.load(Ordering::SeqCst)
}

/// Get the current pipeline state
#[tauri::command]
pub fn get_pipeline_state() -> PipelineState {
    if !PIPELINE_RUNNING.load(Ordering::SeqCst) {
        PipelineState::Idle
    } else if crate::audio::is_recording() {
        PipelineState::Recording
    } else {
        // Could be transcribing, filtering, enhancing, or outputting
        // The exact state is tracked by the async processing
        PipelineState::Transcribing
    }
}

/// Process audio through the transcription pipeline
async fn process_audio(
    app: &AppHandle,
    audio_path: &str,
    config: &PipelineConfig,
) -> Result<PipelineResult, String> {
    // Get audio duration (if available)
    let duration_seconds = get_audio_duration(audio_path);

    // Get transcription model name and backend
    let transcription_model_name = get_transcription_model_name();

    // 1. Transcribe (with timing)
    tracing::info!("Pipeline: Starting transcription of {}", audio_path);
    emit_progress(app, PipelineState::Transcribing, "Transcribing audio...");
    let transcription_start = std::time::Instant::now();
    let raw_text = transcription::transcribe_file(audio_path.to_string())?;
    let transcription_duration_seconds = transcription_start.elapsed().as_secs_f64();

    tracing::info!(
        "Pipeline: Transcription took {:.2}s",
        transcription_duration_seconds
    );

    if raw_text.trim().is_empty() {
        tracing::warn!("Pipeline: Transcription produced no text");
        return Err("Transcription produced no text".to_string());
    }

    tracing::info!(
        "Pipeline: Transcribed {} characters: '{}'",
        raw_text.len(),
        raw_text.chars().take(100).collect::<String>()
    );

    // 2. Apply filtering
    let mut text = raw_text.clone();

    if config.apply_filtering || config.apply_dictionary {
        tracing::info!(
            "Pipeline: Applying filters (filtering={}, dictionary={})",
            config.apply_filtering,
            config.apply_dictionary
        );
        emit_progress(app, PipelineState::Filtering, "Applying filters...");

        if config.apply_filtering {
            text = transcription::filter_transcription(text, None);
            tracing::debug!("Pipeline: After filtering: {} chars", text.len());
        }

        if config.apply_dictionary {
            text = dictionary::apply_dictionary(&text);
            tracing::debug!("Pipeline: After dictionary: {} chars", text.len());
        }

        tracing::info!("Pipeline: Filtered text to {} characters", text.len());
    }

    // 3. AI Enhancement (optional, with timing)
    let mut enhancement_model_name: Option<String> = None;
    let mut enhancement_duration_seconds: Option<f64> = None;

    let is_enhanced = if config.enhancement_enabled && !config.enhancement_model.is_empty() {
        emit_progress(app, PipelineState::Enhancing, "Enhancing with AI...");

        let enhancement_start = std::time::Instant::now();
        match enhancement::enhance_text(
            text.clone(),
            config.enhancement_model.clone(),
            config.enhancement_prompt.clone(),
        )
        .await
        {
            Ok(enhanced) => {
                let elapsed = enhancement_start.elapsed().as_secs_f64();
                text = enhanced;
                enhancement_model_name = Some(config.enhancement_model.clone());
                enhancement_duration_seconds = Some(elapsed);
                tracing::info!(
                    "Pipeline: Enhanced text to {} characters in {:.2}s",
                    text.len(),
                    elapsed
                );
                true
            }
            Err(e) => {
                tracing::warn!("Pipeline: Enhancement failed, using original text: {}", e);
                false
            }
        }
    } else {
        false
    };

    // 4. Output (clipboard/paste)
    tracing::info!(
        "Pipeline: Starting output (copy={}, paste={})",
        config.auto_copy,
        config.auto_paste
    );
    emit_progress(app, PipelineState::Outputting, "Outputting text...");

    if config.auto_copy {
        tracing::debug!("Pipeline: Copying to clipboard...");
        if let Err(e) = clipboard::copy_transcription(app.clone(), text.clone(), is_enhanced).await
        {
            tracing::warn!("Pipeline: Failed to copy to clipboard: {}", e);
        } else {
            tracing::debug!("Pipeline: Copied to clipboard successfully");
        }
    }

    if config.auto_paste {
        tracing::debug!("Pipeline: Pasting text...");
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let insert_result = if config.insertion_method == "typing" {
            crate::text_insert::insert_text_by_typing(text.clone(), None, None)
        } else {
            crate::text_insert::insert_text_by_paste(text.clone(), None)
        };

        if let Err(e) = insert_result {
            tracing::warn!("Pipeline: Failed to insert text: {}", e);
        } else {
            tracing::debug!("Pipeline: Pasted text successfully");
        }
    }

    // 5. Save to history
    tracing::info!("Pipeline: Saving to history...");
    let transcription_id = save_to_history(
        &text,
        &raw_text,
        duration_seconds,
        audio_path,
        is_enhanced,
        if is_enhanced {
            Some(&config.enhancement_prompt)
        } else {
            None
        },
        transcription_model_name.as_deref(),
        Some(transcription_duration_seconds),
        enhancement_model_name.as_deref(),
        enhancement_duration_seconds,
    );
    tracing::info!("Pipeline: Saved to history, id={:?}", transcription_id);

    // Update tray with latest transcription
    tray::set_last_transcription(app, Some(text.clone()));

    tracing::info!("Pipeline: Processing complete, emitting Completed state");
    emit_progress(app, PipelineState::Completed, "Done");

    Ok(PipelineResult {
        success: true,
        text,
        raw_text,
        is_enhanced,
        duration_seconds,
        audio_path: Some(audio_path.to_string()),
        error: None,
        transcription_id,
        transcription_model_name,
        transcription_duration_seconds: Some(transcription_duration_seconds),
        enhancement_model_name,
        enhancement_duration_seconds,
    })
}

/// Save transcription to history database
#[allow(clippy::too_many_arguments)]
fn save_to_history(
    text: &str,
    raw_text: &str,
    duration_seconds: Option<f64>,
    audio_path: &str,
    is_enhanced: bool,
    enhancement_prompt: Option<&str>,
    transcription_model_name: Option<&str>,
    transcription_duration_seconds: Option<f64>,
    enhancement_model_name: Option<&str>,
    enhancement_duration_seconds: Option<f64>,
) -> Option<String> {
    // Ensure database is initialised
    if database::transcription::get_transcription("test").is_err() {
        tracing::warn!("Pipeline: Database not initialised, skipping history save");
        return None;
    }

    let transcription = database::transcription::Transcription::with_details(
        text.to_string(),
        if is_enhanced {
            Some(raw_text.to_string())
        } else {
            None
        },
        duration_seconds,
        Some(audio_path.to_string()),
        is_enhanced,
        enhancement_prompt.map(|s| s.to_string()),
        transcription_model_name.map(|s| s.to_string()),
        transcription_duration_seconds,
        enhancement_model_name.map(|s| s.to_string()),
        enhancement_duration_seconds,
    );

    match database::transcription::create_transcription(&transcription) {
        Ok(()) => {
            tracing::info!("Pipeline: Saved transcription {}", transcription.id);
            Some(transcription.id)
        }
        Err(e) => {
            tracing::warn!("Pipeline: Failed to save transcription: {}", e);
            None
        }
    }
}

/// Get the name of the currently active transcription model.
fn get_transcription_model_name() -> Option<String> {
    // Try to get the selected model ID from config
    let model_id = crate::config::get_config()
        .ok()
        .and_then(|c| c.transcription.model_id.clone());

    // Fall back to backend name if no model ID configured
    model_id.or_else(transcription::get_transcription_backend)
}

/// Get audio file duration (placeholder - returns None for now)
fn get_audio_duration(audio_path: &str) -> Option<f64> {
    // Try to read WAV file header to get duration
    let path = PathBuf::from(audio_path);
    if !path.exists() {
        return None;
    }

    // Read WAV header for duration calculation
    match std::fs::File::open(&path) {
        Ok(file) => {
            use std::io::Read;
            let mut reader = std::io::BufReader::new(file);

            // WAV format: bytes 24-27 = sample rate, bytes 28-31 = byte rate
            // Total samples = (file_size - 44) / (bits_per_sample / 8 * num_channels)
            // Duration = total_samples / sample_rate

            let mut header = [0u8; 44];
            if reader.read_exact(&mut header).is_ok() {
                // Get sample rate from bytes 24-27 (little-endian)
                let sample_rate =
                    u32::from_le_bytes([header[24], header[25], header[26], header[27]]);
                // Get byte rate from bytes 28-31 (little-endian)
                let byte_rate =
                    u32::from_le_bytes([header[28], header[29], header[30], header[31]]);

                if byte_rate > 0 {
                    // Get file size
                    if let Ok(metadata) = std::fs::metadata(&path) {
                        let data_size = metadata.len().saturating_sub(44) as f64;
                        let duration = data_size / byte_rate as f64;
                        tracing::debug!(
                            "Audio duration: {:.2}s (sample_rate={}, byte_rate={})",
                            duration,
                            sample_rate,
                            byte_rate
                        );
                        return Some(duration);
                    }
                }
            }
            None
        }
        Err(_) => None,
    }
}

/// Transcribe an imported audio file through the full pipeline.
///
/// Decodes the input file (WAV, MP3, M4A, OGG, FLAC) to 16kHz mono WAV,
/// then runs the standard transcription pipeline (transcribe → filter → enhance → save).
/// Does NOT auto-copy or auto-paste (the user is already in the app).
#[tauri::command]
pub async fn pipeline_transcribe_file(
    app: AppHandle,
    file_path: String,
    config: Option<PipelineConfig>,
) -> Result<PipelineResult, String> {
    tracing::info!("Pipeline: transcribe_file called for {}", file_path);

    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Pipeline is already running".to_string());
    }

    // RAII guard ensures PIPELINE_RUNNING is reset even on early return
    let _guard = PipelineGuard;

    // Check transcription model is available
    if !transcription::is_transcription_ready() {
        return Err(
            "No transcription model downloaded. Open Settings \u{2192} Models to get started."
                .to_string(),
        );
    }

    // Reset cancellation signal
    IMPORT_CANCELLED.store(false, Ordering::SeqCst);

    // Build config with auto_copy and auto_paste disabled (manual copy from UI)
    let mut config = config.unwrap_or_default();
    config.auto_copy = false;
    config.auto_paste = false;

    // Generate output path for the decoded WAV
    let home = dirs::home_dir().ok_or("Could not find home directory")?;
    let recordings_dir = home.join(".thoth").join("Recordings");
    std::fs::create_dir_all(&recordings_dir)
        .map_err(|e| format!("Failed to create recordings directory: {}", e))?;

    let filename = format!(
        "thoth_import_{}.wav",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let output_wav = recordings_dir.join(&filename);

    // Decode the audio file to 16kHz mono WAV (CPU-bound, run off async runtime)
    emit_progress(
        &app,
        PipelineState::Converting,
        "Converting audio format...",
    );

    let input_path = PathBuf::from(&file_path);
    let output_path = output_wav.clone();
    let decode_result = tokio::task::spawn_blocking(move || {
        crate::audio::decode::decode_audio_to_wav(&input_path, &output_path, &IMPORT_CANCELLED)
    })
    .await
    .map_err(|e| format!("Decode task failed: {}", e))?;

    let _duration = decode_result?;

    let wav_path = output_wav.to_string_lossy().to_string();
    tracing::info!("Pipeline: Decoded to {}", wav_path);

    // Run the standard processing pipeline
    let result = process_audio(&app, &wav_path, &config).await;

    // Emit completion event
    match &result {
        Ok(r) => {
            tracing::info!("Pipeline: Emitting pipeline-complete event");
            if let Err(e) = app.emit("pipeline-complete", r) {
                tracing::error!("Pipeline: Failed to emit pipeline-complete: {}", e);
            }
        }
        Err(e) => {
            tracing::error!("Pipeline: File transcription failed: {}", e);
            emit_progress(&app, PipelineState::Failed, e);
        }
    }

    result
}

/// Emit a pipeline progress event
fn emit_progress(app: &AppHandle, state: PipelineState, message: &str) {
    emit_progress_with_device(app, state, message, None);
}

/// Emit a pipeline progress event with optional device name
fn emit_progress_with_device(
    app: &AppHandle,
    state: PipelineState,
    message: &str,
    device_name: Option<String>,
) {
    let progress = PipelineProgress {
        state,
        message: message.to_string(),
        device_name,
    };
    if let Err(e) = app.emit("pipeline-progress", &progress) {
        tracing::warn!("Failed to emit pipeline progress: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipeline_config_default() {
        let config = PipelineConfig::default();
        assert!(config.apply_dictionary);
        assert!(config.apply_filtering);
        assert!(!config.enhancement_enabled);
        assert!(config.auto_copy);
        assert!(config.auto_paste);
    }

    #[test]
    fn test_pipeline_state_serialisation() {
        let state = PipelineState::Recording;
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(json, "\"recording\"");

        let deserialised: PipelineState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialised, PipelineState::Recording);

        // Verify new Converting state serialises correctly
        let converting = PipelineState::Converting;
        let json = serde_json::to_string(&converting).unwrap();
        assert_eq!(json, "\"converting\"");
    }

    #[test]
    fn test_pipeline_result_serialisation() {
        let result = PipelineResult {
            success: true,
            text: "Hello world".to_string(),
            raw_text: "hello world".to_string(),
            is_enhanced: false,
            duration_seconds: Some(5.5),
            audio_path: Some("/tmp/test.wav".to_string()),
            error: None,
            transcription_id: Some("abc123".to_string()),
            transcription_model_name: Some("ggml-large-v3-turbo".to_string()),
            transcription_duration_seconds: Some(1.2),
            enhancement_model_name: None,
            enhancement_duration_seconds: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"text\":\"Hello world\""));
        assert!(json.contains("\"transcriptionModelName\""));
    }
}
