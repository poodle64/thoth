//! Parakeet TDT transcription using Sherpa-ONNX
//!
//! Uses the Parakeet V3 NeMo Transducer model for high-quality
//! offline speech recognition.
//!
//! Requires the `parakeet` Cargo feature to be enabled.

#![cfg(feature = "parakeet")]

use anyhow::{anyhow, Result};
use sherpa_rs::transducer::{TransducerConfig, TransducerRecognizer};
use std::path::{Path, PathBuf};

/// Transcription service using Parakeet TDT model
pub struct TranscriptionService {
    recognizer: TransducerRecognizer,
}

impl TranscriptionService {
    /// Create a new transcription service with models from the given directory
    ///
    /// Expects the following files in the model directory:
    /// - `encoder.int8.onnx`
    /// - `decoder.int8.onnx`
    /// - `joiner.int8.onnx`
    /// - `tokens.txt`
    pub fn new(model_dir: &Path) -> Result<Self> {
        if !model_dir.exists() {
            return Err(anyhow!(
                "Model directory does not exist: {}",
                model_dir.display()
            ));
        }

        // Check for required files
        let encoder = model_dir.join("encoder.int8.onnx");
        let decoder = model_dir.join("decoder.int8.onnx");
        let joiner = model_dir.join("joiner.int8.onnx");
        let tokens = model_dir.join("tokens.txt");

        for path in [&encoder, &decoder, &joiner, &tokens] {
            if !path.exists() {
                return Err(anyhow!("Required model file not found: {}", path.display()));
            }
        }

        tracing::info!("Loading Parakeet model from {}", model_dir.display());

        let encoder_str = encoder.to_string_lossy().to_string();
        let decoder_str = decoder.to_string_lossy().to_string();
        let joiner_str = joiner.to_string_lossy().to_string();
        let tokens_str = tokens.to_string_lossy().to_string();
        let num_threads = num_cpus::get().min(8) as i32;

        let build_config = |provider: Option<String>| TransducerConfig {
            encoder: encoder_str.clone(),
            decoder: decoder_str.clone(),
            joiner: joiner_str.clone(),
            tokens: tokens_str.clone(),
            num_threads,
            sample_rate: 16000,
            feature_dim: 80,
            model_type: "nemo_transducer".to_string(),
            provider,
            ..Default::default()
        };

        // Try CoreML on macOS for GPU acceleration, fall back to CPU if it fails
        #[cfg(target_os = "macos")]
        let recognizer = {
            tracing::info!("Attempting CoreML provider for GPU acceleration");
            match TransducerRecognizer::new(build_config(Some("coreml".to_string()))) {
                Ok(r) => {
                    tracing::info!("CoreML provider initialised successfully");
                    r
                }
                Err(e) => {
                    tracing::warn!("CoreML provider failed ({}), falling back to CPU", e);
                    TransducerRecognizer::new(build_config(None))
                        .map_err(|e| anyhow!("Failed to create recognizer with CPU: {}", e))?
                }
            }
        };

        #[cfg(not(target_os = "macos"))]
        let recognizer = {
            tracing::info!("Using CPU provider");
            TransducerRecognizer::new(build_config(None))
                .map_err(|e| anyhow!("Failed to create recognizer: {}", e))?
        };

        tracing::info!("Parakeet model loaded successfully");
        Ok(Self { recognizer })
    }

    /// Transcribe audio from a WAV file
    ///
    /// The file should be 16kHz mono WAV. Returns the transcribed text.
    pub fn transcribe(&mut self, audio_path: &Path) -> Result<String> {
        let (samples, sample_rate) = load_wav_samples(audio_path)?;

        tracing::info!(
            "Loaded audio: {} samples at {}Hz ({:.2}s), path: {}",
            samples.len(),
            sample_rate,
            samples.len() as f32 / sample_rate as f32,
            audio_path.display()
        );

        // Check audio levels for debugging
        let max_level = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        let rms = (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt();
        tracing::info!(
            "Audio levels - max: {:.4}, RMS: {:.4} (dB: {:.1})",
            max_level,
            rms,
            20.0 * (rms + 1e-10).log10()
        );

        if max_level < 0.001 {
            tracing::warn!(
                "Audio appears to be silent or extremely quiet (max level: {})",
                max_level
            );
        }

        // Trim leading/trailing silence for long recordings
        let (trim_start, trim_end) = crate::audio::vad::trim_silence(&samples, sample_rate);
        let samples = samples[trim_start..trim_end].to_vec();

        if sample_rate != 16000 {
            tracing::warn!(
                "Audio sample rate is {}Hz, expected 16000Hz. Results may be affected.",
                sample_rate
            );
        }

        // Pad with silence so the model can initialise before the first word
        // and finalise after the last word. Parakeet handles arbitrary lengths,
        // so the extra padding never causes a problem.
        let samples = {
            const LEADING_SILENCE: usize = 8_000; // 500 ms at 16 kHz
            const TRAILING_SILENCE: usize = 24_000; // 1.5 s at 16 kHz
            let mut padded = Vec::with_capacity(LEADING_SILENCE + samples.len() + TRAILING_SILENCE);
            padded.extend(std::iter::repeat(0.0f32).take(LEADING_SILENCE));
            padded.extend(samples);
            padded.extend(std::iter::repeat(0.0f32).take(TRAILING_SILENCE));
            padded
        };

        let start = std::time::Instant::now();
        let text = self.recognizer.transcribe(sample_rate, &samples);
        let duration = start.elapsed();

        let audio_duration = samples.len() as f32 / sample_rate as f32;
        let rtf = duration.as_secs_f32() / audio_duration;

        tracing::info!(
            "Transcribed {:.2}s audio in {:.2}s (RTF: {:.3})",
            audio_duration,
            duration.as_secs_f32(),
            rtf
        );

        tracing::info!("Transcription result: '{}' ({} chars)", text, text.len());

        Ok(text)
    }

    /// Transcribe audio samples directly
    ///
    /// Samples should be 16kHz f32 mono audio.
    pub fn transcribe_samples(&mut self, samples: &[f32]) -> String {
        self.recognizer.transcribe(16000, samples)
    }
}

/// Load samples from a WAV file
fn load_wav_samples(path: &Path) -> Result<(Vec<f32>, u32)> {
    let reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let sample_rate = spec.sample_rate;

    let samples: Vec<f32> = if spec.bits_per_sample == 16 {
        reader
            .into_samples::<i16>()
            .filter_map(|s| s.ok())
            .map(|s| s as f32 / 32768.0)
            .collect()
    } else if spec.bits_per_sample == 32 && spec.sample_format == hound::SampleFormat::Float {
        reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect()
    } else {
        return Err(anyhow!(
            "Unsupported audio format: {} bits, {:?}",
            spec.bits_per_sample,
            spec.sample_format
        ));
    };

    // Mix to mono if stereo
    let mono_samples = if spec.channels == 2 {
        samples
            .chunks(2)
            .map(|c| (c[0] + c.get(1).copied().unwrap_or(0.0)) / 2.0)
            .collect()
    } else {
        samples
    };

    Ok((mono_samples, sample_rate))
}

/// Get the default model directory path
pub fn get_model_directory() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".thoth").join("models").join("parakeet")
}

/// Check if the model is downloaded
pub fn is_model_downloaded() -> bool {
    let model_dir = get_model_directory();
    model_dir.join("encoder.int8.onnx").exists()
        && model_dir.join("decoder.int8.onnx").exists()
        && model_dir.join("joiner.int8.onnx").exists()
        && model_dir.join("tokens.txt").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_model_directory() {
        let dir = get_model_directory();
        assert!(dir.ends_with("parakeet"));
        assert!(dir.to_string_lossy().contains(".thoth"));
    }

    #[test]
    fn test_is_model_downloaded() {
        // May or may not be downloaded depending on environment
        let _result = is_model_downloaded();
    }

    #[test]
    fn test_service_creation_missing_model() {
        let result = TranscriptionService::new(Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    // Integration test - only runs if model is downloaded
    #[test]
    #[ignore] // Run with: cargo test -- --ignored
    fn test_transcription() {
        if !is_model_downloaded() {
            println!("Model not downloaded, skipping test");
            return;
        }

        let model_dir = get_model_directory();
        let _service = TranscriptionService::new(&model_dir).expect("Failed to create service");

        // Would need a test audio file to actually test transcription
        println!("Service created successfully");
    }
}
