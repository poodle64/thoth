//! Transcription subsystem with dual backends
//!
//! Primary: whisper.cpp with Metal GPU acceleration (fastest)
//! Fallback: Sherpa-ONNX with Parakeet models (cross-platform)

mod au_spelling_map;
pub mod download;
pub mod filter;
#[cfg(all(target_os = "macos", feature = "fluidaudio"))]
pub mod fluidaudio;
pub mod manifest;
#[cfg(feature = "parakeet")]
pub mod parakeet;
pub mod whisper;

pub use filter::{FilterOptions, OutputFilter};
pub use manifest::{ModelInfo, fetch_model_manifest, get_manifest_update_time};

use crate::error::Error;
use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// Transcription backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TranscriptionBackend {
    /// Whisper with Metal GPU acceleration (primary, fastest)
    #[default]
    Whisper,
    /// Sherpa-ONNX with Parakeet models (fallback)
    Parakeet,
    /// FluidAudio with Apple Neural Engine via CoreML (fastest on Apple Silicon)
    FluidAudio,
}

/// Unified transcription service that can use either backend
pub enum TranscriptionService {
    Whisper(whisper::WhisperTranscriptionService),
    #[cfg(feature = "parakeet")]
    Parakeet(parakeet::TranscriptionService),
    #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
    FluidAudio(fluidaudio::TranscriptionService),
}

impl TranscriptionService {
    /// Create a new transcription service with the whisper backend
    pub fn new_whisper(model_path: &std::path::Path) -> anyhow::Result<Self> {
        let service = whisper::WhisperTranscriptionService::new(model_path)?;
        Ok(Self::Whisper(service))
    }

    /// Create a new transcription service with the parakeet backend
    #[cfg(feature = "parakeet")]
    pub fn new_parakeet(model_dir: &std::path::Path) -> anyhow::Result<Self> {
        let service = parakeet::TranscriptionService::new(model_dir)?;
        Ok(Self::Parakeet(service))
    }

    /// Create a new transcription service with the FluidAudio backend (Apple Neural Engine)
    #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
    pub fn new_fluidaudio() -> anyhow::Result<Self> {
        let service = fluidaudio::TranscriptionService::new()?;
        Ok(Self::FluidAudio(service))
    }

    /// Transcribe audio from a WAV file
    pub fn transcribe(&mut self, audio_path: &std::path::Path) -> anyhow::Result<String> {
        match self {
            Self::Whisper(service) => service.transcribe(audio_path),
            #[cfg(feature = "parakeet")]
            Self::Parakeet(service) => service.transcribe(audio_path),
            #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
            Self::FluidAudio(service) => service.transcribe(audio_path),
        }
    }

    /// Get the backend type
    pub fn backend(&self) -> TranscriptionBackend {
        match self {
            Self::Whisper(_) => TranscriptionBackend::Whisper,
            #[cfg(feature = "parakeet")]
            Self::Parakeet(_) => TranscriptionBackend::Parakeet,
            #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
            Self::FluidAudio(_) => TranscriptionBackend::FluidAudio,
        }
    }
}

/// Global transcription service instance
static TRANSCRIPTION_SERVICE: OnceLock<Mutex<Option<TranscriptionService>>> = OnceLock::new();

fn get_service() -> &'static Mutex<Option<TranscriptionService>> {
    TRANSCRIPTION_SERVICE.get_or_init(|| Mutex::new(None))
}

/// Whether the most recent warmup attempt finished without loading any usable
/// model (the selected backend *and* the Whisper fallback both failed).
///
/// This is the difference between "a model is configured / on disk" and "a model
/// can actually transcribe". The record guard and the pipeline's load-wait read
/// it so they can block (or bail fast) instead of recording into a void and then
/// hanging for 60 s on a model that will never load.
static WARMUP_FAILED: AtomicBool = AtomicBool::new(false);

/// Returns `true` when the last warmup attempt loaded no usable transcription model.
pub fn warmup_failed() -> bool {
    WARMUP_FAILED.load(Ordering::SeqCst)
}

/// Initialise the transcription service with whisper backend (primary)
#[tauri::command]
pub fn init_whisper_transcription(model_path: String) -> Result<(), Error> {
    let service =
        TranscriptionService::new_whisper(&PathBuf::from(model_path)).map_err(|e| e.to_string())?;

    let mut guard = get_service().lock();
    *guard = Some(service);

    tracing::info!(
        "Whisper transcription service initialised ({} backend)",
        crate::platform::GpuBackendType::compiled()
    );
    Ok(())
}

/// Initialise the transcription service with parakeet backend (fallback)
#[tauri::command]
pub fn init_parakeet_transcription(_model_dir: String) -> Result<(), Error> {
    #[cfg(feature = "parakeet")]
    {
        let service = TranscriptionService::new_parakeet(&PathBuf::from(_model_dir))
            .map_err(|e| e.to_string())?;

        let mut guard = get_service().lock();
        *guard = Some(service);

        tracing::info!("Parakeet transcription service initialised");
        Ok(())
    }

    #[cfg(not(feature = "parakeet"))]
    Err("Parakeet backend not available in this build"
        .to_string()
        .into())
}

/// Initialise the transcription service with FluidAudio backend (Apple Neural Engine)
#[tauri::command]
pub fn init_fluidaudio_transcription() -> Result<(), Error> {
    #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
    {
        let service = TranscriptionService::new_fluidaudio().map_err(|e| e.to_string())?;

        let mut guard = get_service().lock();
        *guard = Some(service);

        // Write sentinel marker so check_model_downloaded() returns true
        if let Err(e) = fluidaudio::write_ready_marker() {
            tracing::warn!("Failed to write FluidAudio ready marker: {}", e);
        }

        // Persist the manifest version for update-available comparisons
        let fa_id = "fluidaudio-parakeet-tdt-coreml";
        let manifest = manifest::get_fallback_manifest();
        if let Some(fa_model) = manifest.models.iter().find(|m| m.id == fa_id) {
            let version_path = manifest::get_model_directory(fa_id).join(".version");
            if let Err(e) = std::fs::write(&version_path, fa_model.version.trim()) {
                tracing::warn!("Failed to write .version sidecar for FluidAudio: {}", e);
            } else {
                tracing::debug!(
                    "Wrote .version sidecar for FluidAudio ({})",
                    fa_model.version
                );
            }
        }

        tracing::info!("FluidAudio transcription service initialised (Neural Engine)");
        Ok(())
    }

    #[cfg(not(all(target_os = "macos", feature = "fluidaudio")))]
    Err("FluidAudio backend not available in this build"
        .to_string()
        .into())
}

/// Initialise the transcription service (auto-detect best backend)
///
/// Tries whisper first, falls back to parakeet if whisper model not found.
#[tauri::command]
pub fn init_transcription(model_path: String) -> Result<(), Error> {
    let path = PathBuf::from(&model_path);

    // If it's a direct .bin file path, use whisper
    if path.extension().map(|e| e == "bin").unwrap_or(false) {
        return init_whisper_transcription(model_path);
    }

    // If it's a directory, check what's inside
    if path.is_dir() {
        // First, check for whisper .bin files (priority for Metal GPU)
        if let Ok(entries) = std::fs::read_dir(&path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let entry_path = entry.path();
                if entry_path
                    .extension()
                    .map(|ext| ext == "bin")
                    .unwrap_or(false)
                {
                    tracing::info!("Found whisper model in directory, using Metal GPU backend");
                    return init_whisper_transcription(entry_path.to_string_lossy().to_string());
                }
            }
        }

        // No whisper model found, check for ONNX files (parakeet)
        #[cfg(feature = "parakeet")]
        {
            let encoder = path.join("encoder.int8.onnx");
            if encoder.exists() {
                tracing::info!("Found ONNX model in directory, using Parakeet backend");
                return init_parakeet_transcription(model_path);
            }
        }
        #[cfg(not(feature = "parakeet"))]
        {
            let encoder = path.join("encoder.int8.onnx");
            if encoder.exists() {
                tracing::warn!(
                    "ONNX models found but Parakeet backend not available in this build"
                );
            }
        }

        return Err(format!(
            "No valid transcription model found in directory: {}",
            path.display()
        )
        .into());
    }

    Err(format!(
        "Model path does not exist or is not valid: {}",
        path.display()
    )
    .into())
}

/// Minimum RMS level to consider audio as containing speech.
/// Audio below this threshold is considered silence and won't be transcribed.
/// This prevents Whisper hallucinations on silent recordings.
/// -54 dB ≈ 0.002 linear amplitude. Low enough for quiet/low-gain mics
/// while still filtering out true digital silence.
const MIN_SPEECH_RMS: f32 = 0.002;

/// Transcribe audio from a file path
///
/// Accepts WAV, MP3, M4A, OGG (Vorbis), and FLAC. Non-WAV files and WAV files
/// that are not already 16 kHz mono i16 are transcoded to a temporary WAV before
/// being passed to the ASR backend. The temp file is deleted when this function
/// returns, whether successfully or not.
///
/// Returns empty string if the audio is essentially silent (no speech detected),
/// which prevents Whisper from hallucinating phrases like "Thank you" on silent input.
#[tauri::command]
pub fn transcribe_file(audio_path: String) -> Result<String, Error> {
    let input = PathBuf::from(&audio_path);

    // Decide whether a transcode is needed.
    // is_target_format_wav returns true only for 16kHz mono i16 WAV files.
    // For any other input we run decode_audio_to_wav first.
    let needs_transcode = !crate::audio::decode::is_target_format_wav(&input);

    let (wav_path, _temp): (PathBuf, Option<tempfile::NamedTempFile>) = if needs_transcode {
        let temp = tempfile::Builder::new()
            .prefix("thoth_transcode_")
            .suffix(".wav")
            .tempfile()
            .map_err(|e| format!("Failed to create temporary file: {}", e))?;
        let temp_path = temp.path().to_path_buf();
        let cancel = std::sync::atomic::AtomicBool::new(false);
        crate::audio::decode::decode_audio_to_wav(&input, &temp_path, &cancel)?;
        tracing::info!("Transcoded {} to temporary 16kHz mono WAV", input.display());
        (temp_path, Some(temp))
    } else {
        (input.clone(), None)
    };

    // Check if audio contains speech before transcribing
    if !audio_has_speech(&wav_path)? {
        tracing::info!(
            "Audio file appears to be silent, skipping transcription: {}",
            audio_path
        );
        return Ok(String::new());
    }

    let mut guard = get_service().lock();
    let service = guard
        .as_mut()
        .ok_or_else(|| "Transcription service not initialised".to_string())?;

    service
        .transcribe(&wav_path)
        .map_err(|e| e.to_string())
        .map_err(Into::into)
    // _temp drops here, deleting the temp file (if any) on both Ok and Err paths.
}

/// Check if a WAV file contains speech (has sufficient audio energy)
///
/// Reads the audio samples and calculates RMS. If the RMS is below
/// the silence threshold, returns false (no speech detected).
fn audio_has_speech(path: &std::path::Path) -> Result<bool, String> {
    use std::io::Read;

    let file =
        std::fs::File::open(path).map_err(|e| format!("Failed to open audio file: {}", e))?;
    let mut reader = std::io::BufReader::new(file);

    // Read WAV header (44 bytes minimum for standard WAV)
    let mut header = [0u8; 44];
    reader
        .read_exact(&mut header)
        .map_err(|e| format!("Failed to read WAV header: {}", e))?;

    // Verify RIFF/WAVE header
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return Err("Not a valid WAV file".to_string());
    }

    // Get format info
    let channels = u16::from_le_bytes([header[22], header[23]]) as usize;
    let bits_per_sample = u16::from_le_bytes([header[34], header[35]]);

    if bits_per_sample != 16 {
        // For non-16-bit audio, assume it has speech (can't easily check)
        tracing::debug!(
            "Non-16-bit audio ({}), assuming speech present",
            bits_per_sample
        );
        return Ok(true);
    }

    // Read audio data and calculate RMS
    let mut audio_data = Vec::new();
    reader
        .read_to_end(&mut audio_data)
        .map_err(|e| format!("Failed to read audio data: {}", e))?;

    // Convert i16 samples to f32
    let samples: Vec<f32> = audio_data
        .chunks_exact(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            sample as f32 / 32768.0
        })
        .collect();

    // If stereo, average to mono for RMS calculation
    let mono_samples: Vec<f32> = if channels > 1 {
        samples
            .chunks(channels)
            .map(|frame| frame.iter().sum::<f32>() / channels as f32)
            .collect()
    } else {
        samples
    };

    // Check for speech using windowed RMS rather than overall RMS.
    // Short recordings often contain startup silence from the audio stream
    // initialising, which dilutes the overall RMS below the threshold even
    // when speech is clearly present in part of the recording.
    let overall_rms = crate::audio::metering::calculate_rms(&mono_samples);

    // Also find the peak RMS in 500ms windows
    let window_size = 8000; // 500 ms at 16 kHz
    let peak_window_rms = mono_samples
        .chunks(window_size)
        .map(crate::audio::metering::calculate_rms)
        .fold(0.0f32, f32::max);

    tracing::debug!(
        "Audio RMS: overall={:.6}, peak_window={:.6} (threshold: {}), samples: {}",
        overall_rms,
        peak_window_rms,
        MIN_SPEECH_RMS,
        mono_samples.len()
    );

    Ok(peak_window_rms >= MIN_SPEECH_RMS)
}

/// Eagerly initialise the transcription model in the background.
/// Triggers Metal shader compilation so the first recording is instant.
///
/// Records the outcome in [`WARMUP_FAILED`] so the record guard and pipeline can
/// react immediately when no usable model could be loaded.
pub fn warmup_transcription() {
    // Clear the flag so an in-progress retry is treated optimistically; if this
    // attempt also fails to produce a service, it is set again below.
    WARMUP_FAILED.store(false, Ordering::SeqCst);
    warmup_transcription_inner();
    let loaded = is_transcription_ready();
    WARMUP_FAILED.store(!loaded, Ordering::SeqCst);
    if !loaded {
        tracing::warn!("Warmup finished without loading a usable transcription model");
    }
}

fn warmup_transcription_inner() {
    let selected_id = crate::config::get_config()
        .ok()
        .and_then(|c| c.transcription.model_id.clone());

    let manifest = manifest::get_fallback_manifest();

    // Resolve model type for the selected model
    let selected_model_type = selected_id.as_ref().and_then(|id| {
        manifest
            .models
            .iter()
            .find(|m| m.id == *id)
            .map(|m| m.model_type.as_str())
    });

    // ── FluidAudio path ────────────────────────────────────────────────
    // Try FluidAudio when explicitly selected OR when nothing is selected
    // (it's the recommended default on Apple Silicon).
    let should_try_fluidaudio =
        selected_model_type == Some("fluidaudio_coreml") || selected_id.is_none();

    if should_try_fluidaudio && try_warmup_fluidaudio() {
        return;
    }
    // FluidAudio unavailable/not cached — fall through to Whisper

    // ── Whisper/Parakeet path ──────────────────────────────────────────
    if selected_id.is_some() && selected_model_type != Some("fluidaudio_coreml") {
        // A specific non-FluidAudio model is selected — try to init it
        let model_dir = get_model_directory();
        if !download::check_model_downloaded(None) {
            tracing::info!("Selected model not downloaded yet, skipping warmup");
            return;
        }
        match init_transcription(model_dir) {
            Ok(()) => {
                tracing::info!("Transcription model warmed up");
                return;
            }
            Err(e) => {
                tracing::warn!("Transcription warmup failed: {}", e);
                // Backend might be unavailable, fall through to Whisper fallback
            }
        }
    }

    // ── Whisper fallback ───────────────────────────────────────────────
    warmup_whisper_fallback(&manifest);
}

/// Attempt to warm up FluidAudio. Returns `true` if successful.
fn try_warmup_fluidaudio() -> bool {
    #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
    {
        if fluidaudio::is_cached() {
            match init_fluidaudio_transcription() {
                Ok(()) => {
                    tracing::info!("FluidAudio transcription model warmed up (Neural Engine)");
                    return true;
                }
                Err(e) => {
                    tracing::warn!("FluidAudio warmup failed: {}, falling back", e);
                }
            }
        } else {
            tracing::info!("FluidAudio models not yet cached, falling back to Whisper");
        }
    }

    #[cfg(not(all(target_os = "macos", feature = "fluidaudio")))]
    {
        tracing::debug!("FluidAudio backend not available in this build");
    }

    false
}

/// Fall back to the best available downloaded Whisper model during warmup.
fn warmup_whisper_fallback(manifest: &manifest::ModelManifest) {
    // Try the largest/best downloaded Whisper model (manifest order = quality order)
    if let Some(whisper_model) = manifest
        .models
        .iter()
        .find(|m| m.model_type == "whisper_ggml" && manifest::is_model_downloaded(m))
    {
        let whisper_dir = manifest::get_model_directory(&whisper_model.id);
        match init_transcription(whisper_dir.to_string_lossy().to_string()) {
            Ok(()) => {
                tracing::info!("Fell back to Whisper model '{}'", whisper_model.id);
            }
            Err(e) => {
                tracing::warn!("Whisper fallback also failed: {}", e);
            }
        }
    } else {
        tracing::info!("No downloaded Whisper model available for fallback");
    }
}

/// Check if transcription service is ready
#[tauri::command]
pub fn is_transcription_ready() -> bool {
    get_service().lock().is_some()
}

/// Get the current transcription backend
#[tauri::command]
pub fn get_transcription_backend() -> Option<String> {
    get_service().lock().as_ref().map(|s| match s.backend() {
        TranscriptionBackend::Whisper => "whisper".to_string(),
        TranscriptionBackend::Parakeet => "parakeet".to_string(),
        TranscriptionBackend::FluidAudio => "fluidaudio".to_string(),
    })
}

/// Get the default model directory path for the currently selected/recommended model
#[tauri::command]
pub fn get_model_directory() -> String {
    // Check if a model is selected in config
    let config_model_id = crate::config::get_config()
        .ok()
        .and_then(|c| c.transcription.model_id.clone());

    // Use config model if set, otherwise get recommended from manifest
    let model_id = config_model_id.unwrap_or_else(|| {
        let fallback = manifest::get_fallback_manifest();
        fallback
            .models
            .iter()
            .find(|m| m.recommended)
            .or_else(|| fallback.models.first())
            .map(|m| m.id.clone())
            .unwrap_or_else(|| "ggml-large-v3-turbo".to_string())
    });

    manifest::get_model_directory(&model_id)
        .to_string_lossy()
        .to_string()
}

/// Get the whisper model directory path
#[tauri::command]
pub fn get_whisper_model_directory() -> String {
    whisper::get_whisper_model_directory()
        .to_string_lossy()
        .to_string()
}

/// Check if a whisper model is downloaded
#[tauri::command]
pub fn is_whisper_model_downloaded(model_id: String) -> bool {
    whisper::is_whisper_model_downloaded(&model_id)
}

/// Filter transcription text to clean up filler words and formatting
#[tauri::command]
pub fn filter_transcription(text: String, options: Option<FilterOptions>) -> String {
    let filter_options = options.unwrap_or_default();
    let output_filter = OutputFilter::new(filter_options);
    output_filter.filter(&text)
}

/// Get the currently selected model ID from config
#[tauri::command]
pub fn get_selected_model_id() -> Option<String> {
    crate::config::get_config()
        .ok()
        .and_then(|c| c.transcription.model_id.clone())
}

/// Set the selected model ID in config
#[tauri::command]
pub fn set_selected_model_id(model_id: Option<String>) -> Result<(), Error> {
    let mut config = crate::config::get_config().map_err(|e| e.to_string())?;
    config.transcription.model_id = model_id.clone();
    crate::config::set_config(config).map_err(|e| e.to_string())?;

    tracing::info!("Selected model ID set to: {:?}", model_id);
    Ok(())
}

/// Check if the Parakeet (Sherpa-ONNX) backend is available in this build
#[tauri::command]
pub fn is_parakeet_available() -> bool {
    cfg!(feature = "parakeet")
}

/// Check if the FluidAudio (Apple Neural Engine) backend is available in this build
#[tauri::command]
pub fn is_fluidaudio_available() -> bool {
    cfg!(all(target_os = "macos", feature = "fluidaudio"))
}

/// Check if FluidAudio models are cached (fast init possible)
#[tauri::command]
pub fn is_fluidaudio_cached() -> bool {
    #[cfg(all(target_os = "macos", feature = "fluidaudio"))]
    {
        fluidaudio::is_cached()
    }
    #[cfg(not(all(target_os = "macos", feature = "fluidaudio")))]
    false
}
