//! Transcription pipeline orchestration
//!
//! Wires together the complete flow from recording to output:
//! 1. Recording (start/stop via audio module)
//! 2. Transcription (via transcription module)
//! 3. Filtering (dictionary replacements + output filtering)
//! 4. Enhancement (optional AI enhancement via Ollama)
//! 5. Output (clipboard copy and/or paste at cursor)
//! 6. History (save to database)

use crate::canonical;
use crate::clipboard;
use crate::database;
use crate::dictionary;
use crate::enhancement;
use crate::error::Error;
use crate::transcription;
use crate::tray;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
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
    /// Whether to apply output filtering (formatting, whitespace)
    pub apply_filtering: bool,
    /// Whether to remove hesitation sounds (um, uh, er, ah)
    pub remove_fillers: bool,
    /// Whether to convert US spellings to Australian/British equivalents
    pub australian_spelling: bool,
    /// Whether to convert spoken number words to digits
    pub spoken_numbers_to_digits: bool,
    /// Whether to collapse runs of whitespace and trim leading/trailing spaces
    pub normalise_whitespace: bool,
    /// Whether to fix spacing around punctuation marks
    pub cleanup_punctuation: bool,
    /// Whether to capitalise the first word of each sentence
    pub sentence_case: bool,
    /// Whether to convert spoken formatting commands ("new paragraph" / "new
    /// line") into line breaks
    pub voice_formatting_commands: bool,
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
            remove_fillers: true,
            australian_spelling: false,
            spoken_numbers_to_digits: false,
            normalise_whitespace: true,
            cleanup_punctuation: true,
            sentence_case: false,
            voice_formatting_commands: true,
            enhancement_enabled: false,
            enhancement_model: "llama3.2".to_string(),
            enhancement_prompt: DEFAULT_ENHANCEMENT_PROMPT.to_string(),
            auto_copy: false,
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

/// True only while a capture stream is open (arm → disarm/stop).
/// A new recording is rejected only when this is true.
static PIPELINE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Counts how many detached process_audio tasks are in-flight.
/// Used by get_pipeline_state to distinguish Recording vs Transcribing vs Idle.
static PROCESSING_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Cancellation signal for file import operations
static IMPORT_CANCELLED: AtomicBool = AtomicBool::new(false);

/// Error message emitted when transcription produces no text (silent recording).
///
/// Used as a typed sentinel: callers that need to distinguish "nothing was said"
/// from a genuine failure check via [`is_no_speech_error`]. Centralised here so
/// the comparison is never a fragile inline string match.
const NO_SPEECH_ERROR: &str = "Transcription produced no text";

/// Returns true when the pipeline error string indicates a silent recording rather
/// than a genuine failure. Used to suppress error UI and silently delete orphan WAVs.
fn is_no_speech_error(e: &str) -> bool {
    e == NO_SPEECH_ERROR
}

/// Deletes `audio_path` when `result` is the no-speech sentinel.
///
/// Returns `true` if the file was discarded (caller should suppress error UI),
/// `false` if the result was either success or a genuine error (caller handles
/// normally). Both the recording-path arm and the import-path arm delegate here
/// so the discard decision is tested once in one place.
fn discard_silent_wav(result: &Result<PipelineResult, String>, audio_path: &str) -> bool {
    let Err(e) = result else { return false };
    if !is_no_speech_error(e) {
        return false;
    }
    tracing::info!(
        "Pipeline: Silent recording, deleting orphan WAV: {}",
        audio_path
    );
    tracing::info!(target: "telemetry", event = "recording_silent_dropped", "recording_silent_dropped");
    if let Err(del_err) = std::fs::remove_file(audio_path) {
        tracing::warn!(
            "Pipeline: Failed to delete silent WAV {}: {}",
            audio_path,
            del_err
        );
    }
    true
}

/// Serialises clipboard-save → paste → clipboard-restore across concurrent
/// detached process_audio tasks. Without this, two jobs could race the system
/// clipboard and corrupt the restored content.
static OUTPUT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// RAII guard that resets PIPELINE_RUNNING to false on drop.
/// Used for recording capture only (not for processing).
struct PipelineGuard;

impl Drop for PipelineGuard {
    fn drop(&mut self) {
        PIPELINE_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// RAII guard that decrements PROCESSING_COUNT on drop.
/// Ensures PROCESSING_COUNT stays balanced even if process_audio panics.
struct ProcessingGuard;

impl ProcessingGuard {
    fn new() -> Self {
        PROCESSING_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for ProcessingGuard {
    fn drop(&mut self) {
        PROCESSING_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Start the recording phase of the pipeline
///
/// Emits `pipeline-progress` event with state updates.
/// Also shows the recording indicator overlay and starts audio metering.
#[tauri::command]
pub fn pipeline_start_recording(app: AppHandle) -> Result<String, Error> {
    tracing::info!("Pipeline: pipeline_start_recording called");

    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        tracing::warn!("Pipeline: Already running, rejecting start request");
        return Err("Pipeline is already running".to_string().into());
    }

    // If the transcription model isn't loaded yet, try to load it in the
    // background while we record.  This avoids blocking the user — the model
    // will (hopefully) be ready by the time they stop speaking.
    if !transcription::is_transcription_ready() {
        if !transcription::download::check_model_downloaded(None) {
            PIPELINE_RUNNING.store(false, Ordering::SeqCst);
            tracing::warn!("Pipeline: No transcription model downloaded, blocking recording");
            tracing::warn!(target: "telemetry", reason = "no_model_downloaded", "model_load_failure");
            let _ = crate::recording_indicator::hide_recording_indicator(app.clone());
            return Err(
                "No transcription model downloaded. Open Settings \u{2192} Models to get started."
                    .to_string()
                    .into(),
            );
        }
        tracing::info!("Pipeline: Model not loaded yet, starting eager background load");
        std::thread::spawn(|| {
            transcription::warmup_transcription();
        });
    }

    // Emit recording state early so the UI updates before the device opens.
    // Device name will be filled from audio::last_device_name() after start_recording
    // returns; we emit a second progress event with the name then.
    // Emitting the device name later avoids blocking on the ~90ms CoreAudio device-resolution call before the UI updates.
    emit_progress(&app, PipelineState::Recording, "Recording audio...");

    tracing::info!("Pipeline: Calling audio::start_recording");
    match crate::audio::start_recording() {
        Ok(path) => {
            tracing::info!("Pipeline: Recording started at {}", path);
            tracing::info!(target: "telemetry", event = "recording_started", "recording_started");

            // Now that start_recording has resolved (and stored) the device name,
            // emit a follow-up progress event that includes it for the UI.
            let device_name = crate::audio::last_device_name();
            emit_progress_with_device(
                &app,
                PipelineState::Recording,
                "Recording audio...",
                device_name,
            );

            // Emit authoritative state: is_recording() is now true so
            // get_pipeline_state() returns Recording.
            emit_recording_state(&app);

            // Update tray to show recording state
            tray::set_recording_state(&app, true);

            // NOTE: Recording indicator is shown instantly from the shortcut handler
            // (show_indicator_instant) - no need to show it here again.
            // The indicator window is pre-warmed at startup so no JS init wait needed.

            // Start recording metering AFTER the indicator is visible
            if let Err(e) = crate::audio::start_recording_metering(app) {
                tracing::warn!("Pipeline: Failed to start recording metering: {}", e);
            }

            Ok(path)
        }
        Err(e) => {
            PIPELINE_RUNNING.store(false, Ordering::SeqCst);
            tracing::warn!(target: "telemetry", reason = "audio_start_failed", "audio_device_failure");
            emit_progress(
                &app,
                PipelineState::Failed,
                &format!("Recording failed: {}", e),
            );
            Err(e)
        }
    }
}

/// Stop recording and kick off the transcription pipeline asynchronously.
///
/// Returns immediately after capture has stopped so a new recording can start
/// without waiting for transcription to finish. The actual result is delivered
/// via the `pipeline-complete` event (which the frontend already handles).
///
/// Emits `pipeline-progress` events as each stage completes.
/// Emits `pipeline-complete` when finished with the final result.
/// Hides the recording indicator overlay when recording stops.
#[tauri::command]
pub async fn pipeline_stop_and_process(
    app: AppHandle,
    config: Option<PipelineConfig>,
) -> Result<(), Error> {
    tracing::info!("Pipeline: stop_and_process called");
    let config = config.unwrap_or_default();

    // Stop recording metering
    crate::audio::stop_recording_metering();

    // Update tray to show idle state
    tray::set_recording_state(&app, false);

    // Hide the recording indicator
    if let Err(e) = crate::recording_indicator::hide_recording_indicator(app.clone()) {
        tracing::warn!("Pipeline: Failed to hide recording indicator: {}", e);
    }

    // Stop recording — releases the capture lock so a new recording can start.
    let audio_path = match crate::audio::stop_recording() {
        Ok(path) => path,
        Err(e) => {
            emit_progress(
                &app,
                PipelineState::Failed,
                &format!("Stop recording failed: {}", e),
            );
            // Release capture flag so the next start is not blocked.
            PIPELINE_RUNNING.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };

    // Capture has stopped — release PIPELINE_RUNNING so a new recording can start
    // immediately while this task processes.
    PIPELINE_RUNNING.store(false, Ordering::SeqCst);

    // Recording duration from the WAV header; 0.0 if unavailable.
    let rec_duration = get_audio_duration(&audio_path).unwrap_or(0.0);
    tracing::info!(
        target: "telemetry",
        duration_seconds = rec_duration,
        "recording_stopped"
    );

    tracing::info!(
        "Pipeline: Recording stopped, spawning detached process task for {}",
        audio_path
    );

    // Emit authoritative state: capture ended and processing is starting.
    // We emit Transcribing directly here rather than calling get_pipeline_state()
    // because the ProcessingGuard is not yet acquired (it runs inside the spawned
    // task), so get_pipeline_state() would incorrectly return Idle at this point.
    if let Err(e) = app.emit("recording-state", PipelineState::Transcribing) {
        tracing::warn!("Failed to emit recording-state (stop): {}", e);
    }

    // Detach processing: transcription, filtering, enhancement, output and history
    // run in a separate task. PROCESSING_COUNT tracks in-flight tasks so
    // get_pipeline_state can report Transcribing when appropriate.
    tokio::spawn(async move {
        // Run processing under the guard in an inner scope so PROCESSING_COUNT
        // is decremented BEFORE we emit the final authoritative state. Otherwise
        // get_pipeline_state() would still see the guard alive and report
        // Transcribing, leaving the UI stuck on "Processing" forever.
        let result = {
            let _processing_guard = ProcessingGuard::new();
            process_audio(&app, &audio_path, &config).await
        };
        match &result {
            Ok(r) => {
                tracing::info!("Pipeline: Emitting pipeline-complete event");
                if let Err(e) = app.emit("pipeline-complete", r) {
                    tracing::error!("Pipeline: Failed to emit pipeline-complete: {}", e);
                }
            }
            Err(_) if discard_silent_wav(&result, &audio_path) => {
                // Silent recording suppressed — discard_silent_wav already deleted the WAV.
            }
            Err(e) => {
                tracing::error!("Pipeline: Processing failed: {}", e);
                emit_progress(&app, PipelineState::Failed, e);
            }
        }
        // Emit authoritative state after the guard has dropped. get_pipeline_state()
        // returns Recording if a new clip started while this task ran, so this can
        // never clobber an active recording with Idle.
        emit_recording_state(&app);
    });

    Ok(())
}

/// Cancel the current pipeline execution
#[tauri::command]
pub fn pipeline_cancel(app: AppHandle) -> Result<(), Error> {
    if !PIPELINE_RUNNING.load(Ordering::SeqCst) {
        return Ok(()); // Nothing to cancel
    }

    // Stop recording metering and hide indicator
    crate::audio::stop_recording_metering();
    if let Err(e) = crate::recording_indicator::hide_recording_indicator(app.clone()) {
        tracing::warn!(
            "Pipeline: Failed to hide recording indicator on cancel: {}",
            e
        );
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
    emit_recording_state(&app);
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
    if crate::audio::is_recording() {
        PipelineState::Recording
    } else if PROCESSING_COUNT.load(Ordering::SeqCst) > 0 {
        // Capture has stopped but at least one detached process_audio task is running.
        PipelineState::Transcribing
    } else {
        PipelineState::Idle
    }
}

/// Output of the core transcription pipeline (transcribe + filter + enhance).
///
/// Shared by [`process_audio`] (recordings/imports) and [`pipeline_retranscribe`].
struct TranscriptionPipelineOutput {
    text: String,
    raw_text: String,
    is_enhanced: bool,
    transcription_model_name: Option<String>,
    transcription_duration_seconds: f64,
    enhancement_model_name: Option<String>,
    enhancement_duration_seconds: Option<f64>,
}

/// Core pipeline: wait for model, transcribe audio, apply filters, optionally enhance.
///
/// Does NOT handle output (clipboard/paste), saving to history, or tray updates;
/// callers are responsible for those steps.
async fn run_transcription_pipeline(
    app: &AppHandle,
    audio_path: &str,
    config: &PipelineConfig,
) -> Result<TranscriptionPipelineOutput, String> {
    let transcription_model_name = get_transcription_model_name();

    // 1. Transcribe (with timing)
    // Wait for the model to finish loading if eager background load is in progress.
    tracing::info!("Pipeline: Starting transcription of {}", audio_path);
    if !transcription::is_transcription_ready() {
        emit_progress(
            app,
            PipelineState::Transcribing,
            "Loading transcription model...",
        );
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !transcription::is_transcription_ready() {
            if std::time::Instant::now() > deadline {
                tracing::warn!(target: "telemetry", reason = "load_timeout_60s", "model_load_failure");
                return Err("Transcription model failed to load within 60 seconds".to_string());
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        tracing::info!("Pipeline: Model loaded, proceeding with transcription");
    }
    emit_progress(app, PipelineState::Transcribing, "Transcribing audio...");
    let transcription_start = std::time::Instant::now();
    // transcribe_file is CPU-bound (whisper/sherpa inference). Running it on a
    // dedicated blocking thread avoids starving the shared async worker pool,
    // which matters now that process_audio runs as a detached task.
    let audio_path_owned = audio_path.to_string();
    let raw_text =
        tokio::task::spawn_blocking(move || transcription::transcribe_file(audio_path_owned))
            .await
            .map_err(|e| format!("Transcription task panicked: {}", e))?
            .map_err(|e| e.to_string())?;
    let transcription_duration_seconds = transcription_start.elapsed().as_secs_f64();

    tracing::info!(
        "Pipeline: Transcription took {:.2}s",
        transcription_duration_seconds
    );

    if raw_text.trim().is_empty() {
        tracing::warn!("Pipeline: Transcription produced no text");
        return Err(NO_SPEECH_ERROR.to_string());
    }

    // Content-free telemetry — no transcript text, only metrics.
    {
        let audio_secs = audio_path
            .parse::<f64>()
            .ok()
            .or_else(|| get_audio_duration(audio_path))
            .unwrap_or(0.0);
        let speed_factor = if transcription_duration_seconds > 0.0 {
            audio_secs / transcription_duration_seconds
        } else {
            0.0
        };
        let model_label = transcription_model_name.as_deref().unwrap_or("unknown");
        tracing::info!(
            target: "telemetry",
            backend = %model_label,
            audio_seconds = audio_secs,
            processing_seconds = transcription_duration_seconds,
            speed_factor = speed_factor,
            char_count = raw_text.len(),
            "transcription_complete"
        );
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
            let filter_opts = transcription::FilterOptions {
                remove_fillers: config.remove_fillers,
                australian_spelling: config.australian_spelling,
                spoken_numbers_to_digits: config.spoken_numbers_to_digits,
                normalise_whitespace: config.normalise_whitespace,
                cleanup_punctuation: config.cleanup_punctuation,
                sentence_case: config.sentence_case,
                voice_formatting_commands: config.voice_formatting_commands,
                // The dictionary is applied separately below, gated by
                // config.apply_dictionary. Disable it inside the filter so it
                // runs exactly once and honours the user's dictionary setting
                // (FilterOptions::default() would otherwise turn it on here and
                // apply it a second time, ignoring config.apply_dictionary).
                apply_dictionary: false,
            };
            text = transcription::filter_transcription(text, Some(filter_opts));
            tracing::debug!("Pipeline: After filtering: {} chars", text.len());
        }

        if config.apply_dictionary {
            text = dictionary::apply_dictionary(&text);
            tracing::debug!("Pipeline: After dictionary: {} chars", text.len());
            text = canonical::apply_canonical(&text);
            tracing::debug!("Pipeline: After canonical: {} chars", text.len());
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
                // Deliberately no prompt text/id: the telemetry stream is
                // content-free, and enhancement-by-prompt analytics already
                // live in the Insights dashboard (from the DB column).
                tracing::info!(
                    target: "telemetry",
                    model = %config.enhancement_model,
                    duration_seconds = elapsed,
                    ok = true,
                    "enhancement_complete"
                );
                true
            }
            Err(e) => {
                tracing::warn!("Pipeline: Enhancement failed, using original text: {}", e);
                tracing::warn!(
                    target: "telemetry",
                    model = %config.enhancement_model,
                    ok = false,
                    "enhancement_complete"
                );
                false
            }
        }
    } else {
        false
    };

    Ok(TranscriptionPipelineOutput {
        text,
        raw_text,
        is_enhanced,
        transcription_model_name,
        transcription_duration_seconds,
        enhancement_model_name,
        enhancement_duration_seconds,
    })
}

/// Process audio through the transcription pipeline
async fn process_audio(
    app: &AppHandle,
    audio_path: &str,
    config: &PipelineConfig,
) -> Result<PipelineResult, String> {
    let duration_seconds = get_audio_duration(audio_path);

    // Run core transcription pipeline (transcribe + filter + enhance)
    let output = run_transcription_pipeline(app, audio_path, config).await?;

    // 4. Output (clipboard/paste)
    // The filtered text already carries any spoken-command line breaks (applied
    // in OutputFilter so history and the pasted text stay consistent).
    let mut output_text = output.text.clone();

    // Ensure consecutive transcriptions don't run together when inserted at
    // the cursor. Add a sentence-ending period if the text has no trailing
    // punctuation, then always ensure there is a trailing space so that the
    // next paste doesn't glue directly onto this one.
    //
    // Examples:
    //   "Hello world"  → "Hello world. "
    //   "Hello world." → "Hello world. "
    //   "Hello world," → "Hello world, "
    {
        let last_meaningful = output_text.trim_end().chars().last().unwrap_or('.');
        if !last_meaningful.is_ascii_punctuation() {
            output_text = output_text.trim_end().to_string();
            output_text.push('.');
        }
        output_text.push(' ');
    }

    tracing::info!(
        "Pipeline: Starting output (copy={}, paste={})",
        config.auto_copy,
        config.auto_paste
    );
    emit_progress(app, PipelineState::Outputting, "Outputting text...");

    // Serialise clipboard-save → paste → clipboard-restore across concurrent
    // detached process_audio tasks. Without this, two overlapping recordings
    // can race the system clipboard and corrupt the restored content.
    {
        let _output_guard = OUTPUT_LOCK.lock().await;

        let uses_clipboard_paste = config.auto_paste && config.insertion_method != "typing";

        // Save the user's original clipboard BEFORE any modification.
        // This must happen before copy_transcription or insert_text_by_paste,
        // both of which overwrite the clipboard.
        let saved_clipboard = if uses_clipboard_paste {
            arboard::Clipboard::new()
                .ok()
                .and_then(|mut cb| cb.get_text().ok())
        } else {
            None
        };

        if config.auto_copy {
            tracing::debug!("Pipeline: Copying to clipboard...");
            if let Err(e) =
                clipboard::copy_transcription(app.clone(), output_text.clone(), output.is_enhanced)
                    .await
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
                crate::text_insert::insert_text_by_typing(output_text.clone(), None, None)
            } else {
                crate::text_insert::insert_text_by_paste(output_text.clone(), None)
            };

            if let Err(e) = insert_result {
                tracing::warn!("Pipeline: Failed to insert text: {}", e);
            } else {
                tracing::debug!("Pipeline: Pasted text successfully");
            }
        }

        // Restore the user's original clipboard after paste completes.
        // Uses the configurable restore delay from clipboard settings to give
        // the target application time to process the paste before we overwrite
        // the clipboard again.
        if let Some(original) = saved_clipboard {
            let restore_delay = clipboard::get_restore_delay();
            tracing::debug!("Pipeline: Restoring clipboard in {}ms", restore_delay);
            tokio::time::sleep(tokio::time::Duration::from_millis(restore_delay)).await;
            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(original)) {
                Ok(()) => tracing::debug!("Pipeline: Clipboard restored"),
                Err(e) => tracing::warn!("Pipeline: Failed to restore clipboard: {}", e),
            }
        }
    } // OUTPUT_LOCK released

    // 5. Save to history
    tracing::info!("Pipeline: Saving to history...");
    let transcription_id = save_to_history(
        &output.text,
        &output.raw_text,
        duration_seconds,
        audio_path,
        output.is_enhanced,
        if output.is_enhanced {
            Some(&config.enhancement_prompt)
        } else {
            None
        },
        output.transcription_model_name.as_deref(),
        Some(output.transcription_duration_seconds),
        output.enhancement_model_name.as_deref(),
        output.enhancement_duration_seconds,
    );
    tracing::info!("Pipeline: Saved to history, id={:?}", transcription_id);

    // Update tray with latest transcription
    tray::set_last_transcription(app, Some(output.text.clone()));

    tracing::info!("Pipeline: Processing complete, emitting Completed state");
    emit_progress(app, PipelineState::Completed, "Done");

    Ok(PipelineResult {
        success: true,
        text: output.text,
        raw_text: output.raw_text,
        is_enhanced: output.is_enhanced,
        duration_seconds,
        audio_path: Some(audio_path.to_string()),
        error: None,
        transcription_id,
        transcription_model_name: output.transcription_model_name,
        transcription_duration_seconds: Some(output.transcription_duration_seconds),
        enhancement_model_name: output.enhancement_model_name,
        enhancement_duration_seconds: output.enhancement_duration_seconds,
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
) -> Result<PipelineResult, Error> {
    tracing::info!("Pipeline: transcribe_file called for {}", file_path);

    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Pipeline is already running".to_string().into());
    }

    // RAII guard ensures PIPELINE_RUNNING is reset even on early return
    let _guard = PipelineGuard;

    // If the model isn't loaded yet but is downloaded, start eager loading.
    // The file decode step below takes time, so the model may be ready by
    // the time we need it.
    if !transcription::is_transcription_ready() {
        if !transcription::download::check_model_downloaded(None) {
            return Err(
                "No transcription model downloaded. Open Settings \u{2192} Models to get started."
                    .to_string()
                    .into(),
            );
        }
        tracing::info!("Pipeline: Model not loaded yet, starting eager background load for import");
        std::thread::spawn(|| {
            transcription::warmup_transcription();
        });
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
        Err(_) if discard_silent_wav(&result, &wav_path) => {
            // Silent import suppressed — discard_silent_wav already deleted the WAV.
        }
        Err(e) => {
            tracing::error!("Pipeline: File transcription failed: {}", e);
            emit_progress(&app, PipelineState::Failed, e);
        }
    }

    result.map_err(Into::into)
}

/// Re-transcribe an existing history record using the current model.
///
/// Looks up the audio file from the DB record, re-runs the transcription
/// pipeline, and updates the record in place. Does not copy/paste output.
#[tauri::command]
pub async fn pipeline_retranscribe(
    app: AppHandle,
    transcription_id: String,
    config: Option<PipelineConfig>,
) -> Result<PipelineResult, Error> {
    tracing::info!("Pipeline: retranscribe called for id={}", transcription_id);

    // Look up the existing record from the database
    let existing = database::transcription::get_transcription(&transcription_id)
        .map_err(|e| format!("Failed to read transcription: {}", e))?
        .ok_or_else(|| format!("Transcription '{}' not found", transcription_id))?;

    let audio_path = existing
        .audio_path
        .as_deref()
        .ok_or("This transcription has no associated audio file")?;

    // Check the file still exists on disk
    if !std::path::Path::new(audio_path).exists() {
        return Err(
            "Audio file no longer available. It may have been deleted via Storage cleanup."
                .to_string()
                .into(),
        );
    }

    if PIPELINE_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Pipeline is already running".to_string().into());
    }

    // RAII guard ensures PIPELINE_RUNNING is reset even on early return
    let _guard = PipelineGuard;

    // Ensure model is loaded
    if !transcription::is_transcription_ready() {
        if !transcription::download::check_model_downloaded(None) {
            return Err(
                "No transcription model downloaded. Open Settings \u{2192} Models to get started."
                    .to_string()
                    .into(),
            );
        }
        tracing::info!(
            "Pipeline: Model not loaded, starting eager background load for retranscribe"
        );
        std::thread::spawn(|| {
            transcription::warmup_transcription();
        });
    }

    // Build config with output disabled (retranscribe from history, not at cursor)
    let mut config = config.unwrap_or_default();
    config.auto_copy = false;
    config.auto_paste = false;

    // Run the core transcription pipeline
    let output = run_transcription_pipeline(&app, audio_path, &config).await?;

    // Read-modify-write: update only the fields that changed
    let mut updated = existing;
    updated.text = output.text.clone();
    updated.raw_text = if output.is_enhanced {
        Some(output.raw_text.clone())
    } else {
        None
    };
    updated.is_enhanced = output.is_enhanced;
    updated.enhancement_prompt = if output.is_enhanced {
        Some(config.enhancement_prompt.clone())
    } else {
        None
    };
    updated.transcription_model_name = output.transcription_model_name.clone();
    updated.transcription_duration_seconds = Some(output.transcription_duration_seconds);
    updated.enhancement_model_name = output.enhancement_model_name.clone();
    updated.enhancement_duration_seconds = output.enhancement_duration_seconds;

    // Persist to database
    database::transcription::update_transcription(&updated)
        .map_err(|e| format!("Failed to update transcription: {}", e))?;

    tracing::info!("Pipeline: Retranscribed and updated id={}", updated.id);

    emit_progress(&app, PipelineState::Completed, "Done");

    let result = PipelineResult {
        success: true,
        text: output.text,
        raw_text: output.raw_text,
        is_enhanced: output.is_enhanced,
        duration_seconds: updated.duration_seconds,
        audio_path: updated.audio_path,
        error: None,
        transcription_id: Some(updated.id),
        transcription_model_name: output.transcription_model_name,
        transcription_duration_seconds: Some(output.transcription_duration_seconds),
        enhancement_model_name: output.enhancement_model_name,
        enhancement_duration_seconds: output.enhancement_duration_seconds,
    };

    if let Err(e) = app.emit("pipeline-complete", &result) {
        tracing::error!("Pipeline: Failed to emit pipeline-complete: {}", e);
    }

    Ok(result)
}

/// Toggle recording from the single source of truth: the armed flag.
///
/// Reads `crate::audio::is_recording()` — the authority — and either starts or
/// stops.  The frontend must NOT decide start vs stop independently; it calls
/// this command and updates its display from the returned `ToggleOutcome`.
///
/// Sound is decided here too:
/// - Start path: indicator + BING have already been played by the shortcut
///   handler the instant the key was pressed (instant, before IPC round-trip).
///   We do NOT play them again here.
/// - Stop path: BONG is played here, immediately before capture disarms, so it
///   is always matched to the decided action.
#[tauri::command]
pub async fn pipeline_toggle_recording(
    app: AppHandle,
    config: Option<PipelineConfig>,
) -> Result<ToggleOutcome, Error> {
    if crate::audio::is_recording() {
        // --- STOP ---
        // Play BONG now, before disarming, so the sound is always paired with
        // the action decided from the authority.
        crate::sound::play_sound(crate::sound::SoundEvent::RecordingStop);

        pipeline_stop_and_process(app, config).await?;
        Ok(ToggleOutcome::Stopped)
    } else {
        // --- START ---
        // BING and indicator are played by the shortcut handler on keypress
        // (keyboard_service.rs / manager.rs / tray.rs) so they fire before the
        // IPC round-trip.  pipeline_start_recording does not duplicate them.
        let path = pipeline_start_recording(app)?;
        Ok(ToggleOutcome::Started { path })
    }
}

/// The outcome of a `pipeline_toggle_recording` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ToggleOutcome {
    /// A new recording was started; contains the WAV path.
    Started { path: String },
    /// An active recording was stopped; processing is detached.
    Stopped,
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

/// Emit the authoritative system state on the `recording-state` channel.
///
/// The payload is always derived from `get_pipeline_state()` so that a
/// completion event from a detached task cannot report `Idle` or `Completed`
/// while a new recording is already active (because `get_pipeline_state()`
/// prioritises `Recording` via `is_recording()`).
fn emit_recording_state(app: &AppHandle) {
    let current = get_pipeline_state();
    if let Err(e) = app.emit("recording-state", current) {
        tracing::warn!("Failed to emit recording-state: {}", e);
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
        assert!(config.remove_fillers);
        assert!(!config.australian_spelling);
        assert!(!config.spoken_numbers_to_digits);
        assert!(!config.enhancement_enabled);
        assert!(!config.auto_copy);
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

    /// What a toggle-recording press will do.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ToggleAction {
        Start,
        Stop,
    }

    /// Decide the toggle action from the single source of truth: the armed flag.
    ///
    /// Pure function so it can be unit-tested without touching global `is_recording()` state.
    #[inline]
    fn toggle_action_from_capturing(is_capturing: bool) -> ToggleAction {
        if is_capturing {
            ToggleAction::Stop
        } else {
            ToggleAction::Start
        }
    }

    #[test]
    fn test_toggle_action_from_capturing() {
        // The one invariant: is_capturing is the single authority.
        // IDLE (not capturing) → start; pressing while background processing
        // is in-flight must also → start (processing does NOT gate the action).
        assert_eq!(
            toggle_action_from_capturing(false),
            ToggleAction::Start,
            "not capturing → start"
        );
        assert_eq!(
            toggle_action_from_capturing(true),
            ToggleAction::Stop,
            "capturing → stop"
        );
    }

    #[test]
    fn test_processing_guard_increments_and_decrements() {
        // Baseline: whatever value is in the static before this test.
        let before = PROCESSING_COUNT.load(Ordering::SeqCst);

        {
            let _g1 = ProcessingGuard::new();
            assert_eq!(PROCESSING_COUNT.load(Ordering::SeqCst), before + 1);
            {
                let _g2 = ProcessingGuard::new();
                assert_eq!(PROCESSING_COUNT.load(Ordering::SeqCst), before + 2);
            }
            // _g2 dropped
            assert_eq!(PROCESSING_COUNT.load(Ordering::SeqCst), before + 1);
        }
        // _g1 dropped
        assert_eq!(PROCESSING_COUNT.load(Ordering::SeqCst), before);
    }

    // ── No-speech / silent-recording tests ─────────────────────────────────

    /// `is_no_speech_error` must return true only for the exact sentinel string.
    /// Any other message — including a prefix-match — must return false so we
    /// never suppress a genuine failure.
    #[test]
    fn test_is_no_speech_error_exact_match_only() {
        assert!(
            is_no_speech_error(NO_SPEECH_ERROR),
            "sentinel should match itself"
        );
        assert!(
            !is_no_speech_error(""),
            "empty string must not match sentinel"
        );
        assert!(
            !is_no_speech_error("Transcription failed"),
            "unrelated error must not match"
        );
        assert!(
            !is_no_speech_error("Transcription produced no text: extra detail"),
            "a message that merely starts with the sentinel must not match"
        );
    }

    /// `discard_silent_wav` must delete the file and return `true` when given the
    /// no-speech sentinel, exercising the real helper that both dispatch arms call.
    #[test]
    fn test_no_speech_discard_deletes_wav_file() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let wav_path = tmp_dir.path().join("thoth_recording_test.wav");
        std::fs::write(&wav_path, b"RIFF").expect("failed to write temp file");
        assert!(wav_path.exists(), "temp WAV must exist before discard");

        let result: Result<PipelineResult, String> = Err(NO_SPEECH_ERROR.to_string());
        let discarded = discard_silent_wav(&result, wav_path.to_str().unwrap());

        assert!(
            discarded,
            "helper must return true for the no-speech sentinel"
        );
        assert!(
            !wav_path.exists(),
            "WAV file must be gone after silent-recording discard"
        );
    }

    /// `discard_silent_wav` must return `false` and leave the file intact when
    /// given a genuine error — both the recording-path and import-path arms rely
    /// on this to preserve diagnostic artefacts.
    #[test]
    fn test_genuine_error_retains_wav_file() {
        let tmp_dir = tempfile::tempdir().expect("failed to create temp dir");
        let wav_path = tmp_dir.path().join("thoth_recording_real_error.wav");
        std::fs::write(&wav_path, b"RIFF").expect("failed to write temp file");

        let result: Result<PipelineResult, String> = Err("Transcription model crashed".to_string());
        let discarded = discard_silent_wav(&result, wav_path.to_str().unwrap());

        assert!(!discarded, "helper must return false for a genuine error");
        assert!(
            wav_path.exists(),
            "WAV file must be retained after a genuine pipeline error"
        );
    }
}
