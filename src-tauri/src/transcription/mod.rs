//! Transcription subsystem with dual backends
//!
//! Primary: whisper.cpp with Metal GPU acceleration (fastest)
//! Fallback: Sherpa-ONNX with Parakeet models (cross-platform)

pub mod download;
pub mod filter;
pub mod manifest;
#[cfg(feature = "parakeet")]
pub mod parakeet;
pub mod whisper;

pub use filter::{FilterOptions, OutputFilter};
pub use manifest::{fetch_model_manifest, get_manifest_update_time, ModelInfo};

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Transcription backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptionBackend {
    /// Whisper with Metal GPU acceleration (primary, fastest)
    Whisper,
    /// Sherpa-ONNX with Parakeet models (fallback)
    Parakeet,
}

impl Default for TranscriptionBackend {
    fn default() -> Self {
        // Whisper with Metal is the primary backend for macOS
        Self::Whisper
    }
}

/// Unified transcription service that can use either backend
pub enum TranscriptionService {
    Whisper(whisper::WhisperTranscriptionService),
    #[cfg(feature = "parakeet")]
    Parakeet(parakeet::TranscriptionService),
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

    /// Transcribe audio from a WAV file
    pub fn transcribe(&mut self, audio_path: &std::path::Path) -> anyhow::Result<String> {
        match self {
            Self::Whisper(service) => service.transcribe(audio_path),
            #[cfg(feature = "parakeet")]
            Self::Parakeet(service) => service.transcribe(audio_path),
        }
    }

    /// Get the backend type
    pub fn backend(&self) -> TranscriptionBackend {
        match self {
            Self::Whisper(_) => TranscriptionBackend::Whisper,
            #[cfg(feature = "parakeet")]
            Self::Parakeet(_) => TranscriptionBackend::Parakeet,
        }
    }
}

/// Global transcription service instance
static TRANSCRIPTION_SERVICE: OnceLock<Mutex<Option<TranscriptionService>>> = OnceLock::new();

fn get_service() -> &'static Mutex<Option<TranscriptionService>> {
    TRANSCRIPTION_SERVICE.get_or_init(|| Mutex::new(None))
}

/// Initialise the transcription service with whisper backend (primary)
#[tauri::command]
pub fn init_whisper_transcription(model_path: String) -> Result<(), String> {
    let service = TranscriptionService::new_whisper(&PathBuf::from(model_path))
        .map_err(|e| e.to_string())?;

    let mut guard = get_service().lock();
    *guard = Some(service);

    tracing::info!("Whisper transcription service initialised with Metal GPU");
    Ok(())
}

/// Initialise the transcription service with parakeet backend (fallback)
#[tauri::command]
pub fn init_parakeet_transcription(_model_dir: String) -> Result<(), String> {
    #[cfg(feature = "parakeet")]
    {
        let service = TranscriptionService::new_parakeet(&PathBuf::from(_model_dir))
            .map_err(|e| e.to_string())?;

        let mut guard = get_service().lock();
        *guard = Some(service);

        tracing::info!("Parakeet transcription service initialised");
        return Ok(());
    }

    #[cfg(not(feature = "parakeet"))]
    Err("Parakeet backend not available in this build".to_string())
}

/// Initialise the transcription service (auto-detect best backend)
///
/// Tries whisper first, falls back to parakeet if whisper model not found.
#[tauri::command]
pub fn init_transcription(model_path: String) -> Result<(), String> {
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
                if entry_path.extension().map(|ext| ext == "bin").unwrap_or(false) {
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
        ));
    }

    Err(format!(
        "Model path does not exist or is not valid: {}",
        path.display()
    ))
}

/// Minimum RMS level to consider audio as containing speech.
/// Audio below this threshold is considered silence and won't be transcribed.
/// This prevents Whisper hallucinations on silent recordings.
/// -54 dB ≈ 0.002 linear amplitude. Low enough for quiet/low-gain mics
/// while still filtering out true digital silence.
const MIN_SPEECH_RMS: f32 = 0.002;

/// Transcribe audio from a file path
///
/// Returns empty string if the audio is essentially silent (no speech detected),
/// which prevents Whisper from hallucinating phrases like "Thank you" on silent input.
#[tauri::command]
pub fn transcribe_file(audio_path: String) -> Result<String, String> {
    let path = PathBuf::from(&audio_path);

    // Check if audio contains speech before transcribing
    if !audio_has_speech(&path)? {
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

    service.transcribe(&path).map_err(|e| e.to_string())
}

/// Check if a WAV file contains speech (has sufficient audio energy)
///
/// Reads the audio samples and calculates RMS. If the RMS is below
/// the silence threshold, returns false (no speech detected).
fn audio_has_speech(path: &std::path::Path) -> Result<bool, String> {
    use std::io::Read;

    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open audio file: {}", e))?;
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
        tracing::debug!("Non-16-bit audio ({}), assuming speech present", bits_per_sample);
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

    // Calculate RMS
    let rms = crate::audio::metering::calculate_rms(&mono_samples);

    tracing::debug!(
        "Audio RMS: {:.6} (threshold: {}), samples: {}",
        rms,
        MIN_SPEECH_RMS,
        mono_samples.len()
    );

    Ok(rms >= MIN_SPEECH_RMS)
}

/// Eagerly initialise the transcription model in the background.
/// Triggers Metal shader compilation so the first recording is instant.
pub fn warmup_transcription() {
    let model_dir = get_model_directory();
    if !download::check_model_downloaded(None) {
        tracing::info!("No model downloaded yet, skipping warmup");
        return;
    }
    match init_transcription(model_dir) {
        Ok(()) => tracing::info!("Transcription model warmed up"),
        Err(e) => tracing::warn!("Transcription warmup failed: {}", e),
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
pub fn set_selected_model_id(model_id: Option<String>) -> Result<(), String> {
    let mut config = crate::config::get_config().map_err(|e| e.to_string())?;
    config.transcription.model_id = model_id.clone();
    crate::config::set_config(config).map_err(|e| e.to_string())?;

    tracing::info!("Selected model ID set to: {:?}", model_id);
    Ok(())
}
