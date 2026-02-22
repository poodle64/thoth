//! FluidAudio transcription using Apple Neural Engine via CoreML
//!
//! Runs Parakeet TDT models on the Apple Neural Engine (ANE) for
//! dramatically faster transcription (~210x real-time factor).
//!
//! Requires the `fluidaudio` Cargo feature and macOS with Apple Silicon.

#![cfg(all(target_os = "macos", feature = "fluidaudio"))]

use anyhow::{anyhow, Result};
use fluidaudio_rs::FluidAudio;
use std::path::{Path, PathBuf};

/// Transcription service using FluidAudio (Apple Neural Engine)
pub struct TranscriptionService {
    audio: FluidAudio,
}

// FluidAudio bridge is Send+Sync internally
unsafe impl Send for TranscriptionService {}

impl TranscriptionService {
    /// Create and initialise the FluidAudio transcription service
    ///
    /// This calls `init_asr()` which downloads and compiles CoreML models
    /// on first run (~500 MB download + 20-30s ANE compilation).
    /// Subsequent calls load from cache in ~1s.
    pub fn new() -> Result<Self> {
        let audio = FluidAudio::new()
            .map_err(|e| anyhow!("Failed to create FluidAudio instance: {}", e))?;

        // Check Apple Silicon before attempting ANE init
        if !audio.is_apple_silicon() {
            return Err(anyhow!(
                "FluidAudio requires Apple Silicon (M1/M2/M3/M4). \
                 Intel Macs are not supported."
            ));
        }

        tracing::info!("Initialising FluidAudio ASR (Neural Engine)...");
        let start = std::time::Instant::now();

        audio
            .init_asr()
            .map_err(|e| anyhow!("Failed to initialise FluidAudio ASR: {}", e))?;

        tracing::info!(
            "FluidAudio ASR initialised in {:.1}s",
            start.elapsed().as_secs_f32()
        );

        Ok(Self { audio })
    }

    /// Transcribe audio from a WAV file
    ///
    /// Pads with 500 ms leading silence (so the model can initialise) and
    /// 1 s trailing silence (so it can finalise the last word), then passes
    /// the padded WAV to FluidAudio for transcription.
    pub fn transcribe(&self, audio_path: &Path) -> Result<String> {
        let padded_path = pad_with_silence(audio_path)?;
        let transcribe_path = padded_path.as_deref().unwrap_or(audio_path);

        let start = std::time::Instant::now();

        let result = self
            .audio
            .transcribe_file(transcribe_path)
            .map_err(|e| anyhow!("FluidAudio transcription failed: {}", e))?;

        let duration = start.elapsed();

        // Clean up temp file
        if let Some(ref tmp) = padded_path {
            let _ = std::fs::remove_file(tmp);
        }

        tracing::info!(
            "FluidAudio transcribed {:.2}s audio in {:.3}s (RTFx: {:.0}, confidence: {:.1}%)",
            result.duration,
            duration.as_secs_f32(),
            result.rtfx,
            result.confidence * 100.0
        );

        Ok(result.text)
    }
}

/// Pad a WAV file with leading and trailing silence, writing a temporary copy.
///
/// Prepends 500 ms of silence so the model can initialise before speech starts,
/// and appends 1 s of silence so it can finalise the last word.
///
/// Returns `Ok(Some(path))` with the padded temp file, or `Ok(None)` if the
/// original file couldn't be read (in which case FluidAudio gets the original).
fn pad_with_silence(audio_path: &Path) -> Result<Option<PathBuf>> {
    let mut reader = match hound::WavReader::open(audio_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Could not read WAV for padding: {}, using original", e);
            return Ok(None);
        }
    };

    let spec = reader.spec();
    let sample_rate = spec.sample_rate;
    let leading_samples = sample_rate as usize / 2; // 500 ms
    let trailing_samples = sample_rate as usize * 3 / 2; // 1.5 seconds

    // Build temp path next to the original
    let tmp_path = audio_path.with_extension("padded.wav");

    let mut writer = hound::WavWriter::create(&tmp_path, spec)?;

    match spec.sample_format {
        hound::SampleFormat::Int => {
            // Leading silence (per channel)
            for _ in 0..leading_samples * spec.channels as usize {
                writer.write_sample(0i16)?;
            }
            // Copy all existing samples
            for sample in reader.samples::<i16>() {
                writer.write_sample(sample?)?;
            }
            // Trailing silence (per channel)
            for _ in 0..trailing_samples * spec.channels as usize {
                writer.write_sample(0i16)?;
            }
        }
        hound::SampleFormat::Float => {
            for _ in 0..leading_samples * spec.channels as usize {
                writer.write_sample(0.0f32)?;
            }
            for sample in reader.samples::<f32>() {
                writer.write_sample(sample?)?;
            }
            for _ in 0..trailing_samples * spec.channels as usize {
                writer.write_sample(0.0f32)?;
            }
        }
    }

    writer.finalize()?;

    tracing::info!(
        "Padded WAV with {:.1}s leading + {:.1}s trailing silence: {}",
        leading_samples as f32 / sample_rate as f32,
        trailing_samples as f32 / sample_rate as f32,
        tmp_path.display()
    );

    Ok(Some(tmp_path))
}

/// Check if FluidAudio model cache has content (models already compiled)
///
/// When cached, `init_asr()` takes ~1s instead of 20-30s.
pub fn is_cached() -> bool {
    let cache_dir = model_cache_directory();
    if !cache_dir.exists() {
        return false;
    }

    // Check if the directory has any files (FluidAudio populates it after first init)
    std::fs::read_dir(&cache_dir)
        .map(|entries| entries.count() > 0)
        .unwrap_or(false)
}

/// Get FluidAudio's model cache directory
///
/// FluidAudio stores compiled CoreML models in:
/// `~/Library/Application Support/FluidAudio/Models/`
pub fn model_cache_directory() -> PathBuf {
    dirs::home_dir()
        .expect("Could not find home directory")
        .join("Library")
        .join("Application Support")
        .join("FluidAudio")
        .join("Models")
}

/// Write a sentinel marker file to the Thoth model directory
///
/// This integrates with Thoth's `check_model_downloaded()` infrastructure.
/// The marker file `.fluidaudio_ready` signals that FluidAudio has been
/// successfully initialised and models are cached.
pub fn write_ready_marker() -> Result<()> {
    let marker_dir = super::manifest::get_model_directory("fluidaudio-parakeet-tdt-coreml");
    std::fs::create_dir_all(&marker_dir)?;

    let marker_path = marker_dir.join(".fluidaudio_ready");
    std::fs::write(
        &marker_path,
        "FluidAudio models cached and ready.\n\
         CoreML cache: ~/Library/Application Support/FluidAudio/Models/\n",
    )?;

    tracing::info!("Wrote FluidAudio ready marker: {}", marker_path.display());
    Ok(())
}

/// Remove the sentinel marker file
///
/// Called when the user "deletes" the FluidAudio model from Model Manager.
pub fn remove_ready_marker() -> Result<()> {
    let marker_dir = super::manifest::get_model_directory("fluidaudio-parakeet-tdt-coreml");
    let marker_path = marker_dir.join(".fluidaudio_ready");

    if marker_path.exists() {
        std::fs::remove_file(&marker_path)?;
        tracing::info!("Removed FluidAudio ready marker: {}", marker_path.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_cache_directory() {
        let dir = model_cache_directory();
        assert!(dir.to_string_lossy().contains("FluidAudio"));
        assert!(dir.to_string_lossy().contains("Models"));
    }

    #[test]
    fn test_is_cached() {
        // May or may not be cached depending on environment
        let _result = is_cached();
    }

    #[test]
    #[ignore] // Run with: cargo test --features fluidaudio -- --ignored
    fn test_service_creation() {
        let result = TranscriptionService::new();
        match result {
            Ok(_service) => println!("FluidAudio service created successfully"),
            Err(e) => println!("FluidAudio service creation failed: {}", e),
        }
    }
}
