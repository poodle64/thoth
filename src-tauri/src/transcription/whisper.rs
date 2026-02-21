//! Whisper transcription using whisper.cpp with GPU acceleration
//!
//! This is the primary transcription backend for maximum performance.
//! Uses Metal GPU on macOS and Vulkan on Linux for "snappy as fuck" transcription.

use anyhow::{anyhow, Result};
use std::path::{Path, PathBuf};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

/// Transcription service using Whisper with GPU acceleration
pub struct WhisperTranscriptionService {
    ctx: WhisperContext,
}

impl WhisperTranscriptionService {
    /// Create a new whisper transcription service
    ///
    /// Expects a path to a ggml whisper model file (e.g., ggml-large-v3-turbo.bin)
    ///
    /// GPU acceleration is enabled by default:
    /// - macOS: Uses Metal GPU (always available)
    /// - Linux: Uses CUDA, HIP/ROCm, or Vulkan if compiled with the respective feature
    /// - If GPU initialization fails on Linux, falls back to CPU automatically
    pub fn new(model_path: &Path) -> Result<Self> {
        if !model_path.exists() {
            return Err(anyhow!("Whisper model not found: {}", model_path.display()));
        }

        tracing::info!("Loading Whisper model from {}", model_path.display());

        let model_str = model_path.to_str().ok_or_else(|| {
            anyhow!(
                "Model path contains invalid UTF-8: {}",
                model_path.display()
            )
        })?;

        // Try GPU first, fall back to CPU if it fails
        let ctx = Self::try_load_with_gpu(model_str).or_else(|e| {
            tracing::warn!("GPU initialization failed: {:?}, trying CPU fallback", e);
            Self::load_with_cpu(model_str)
        })?;

        Ok(Self { ctx })
    }

    /// Try to load the model with GPU acceleration
    #[cfg(target_os = "macos")]
    fn try_load_with_gpu(model_str: &str) -> Result<WhisperContext> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);

        let ctx = WhisperContext::new_with_params(model_str, params)
            .map_err(|e| anyhow!("Failed to load Whisper model with Metal: {:?}", e))?;

        tracing::info!("Whisper model loaded with Metal GPU acceleration");
        Ok(ctx)
    }

    /// Try to load the model with GPU acceleration (Linux)
    #[cfg(target_os = "linux")]
    fn try_load_with_gpu(model_str: &str) -> Result<WhisperContext> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);

        let ctx = WhisperContext::new_with_params(model_str, params)
            .map_err(|e| anyhow!("Failed to load Whisper model with GPU: {:?}", e))?;

        // Log which GPU backend was actually used based on compile features
        #[cfg(feature = "cuda")]
        tracing::info!("Whisper model loaded with CUDA GPU acceleration");
        #[cfg(all(not(feature = "cuda"), feature = "hipblas"))]
        tracing::info!("Whisper model loaded with HIP/ROCm GPU acceleration");
        #[cfg(all(not(any(feature = "cuda", feature = "hipblas")), feature = "vulkan"))]
        tracing::info!("Whisper model loaded with Vulkan GPU acceleration");
        #[cfg(not(any(feature = "cuda", feature = "hipblas", feature = "vulkan")))]
        tracing::info!("Whisper model loaded with CPU backend (no GPU feature enabled)");

        Ok(ctx)
    }

    /// Try to load the model with GPU acceleration (Windows)
    #[cfg(target_os = "windows")]
    fn try_load_with_gpu(model_str: &str) -> Result<WhisperContext> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(true);

        let ctx = WhisperContext::new_with_params(model_str, params)
            .map_err(|e| anyhow!("Failed to load Whisper model with GPU: {:?}", e))?;

        #[cfg(feature = "cuda")]
        tracing::info!("Whisper model loaded with CUDA GPU acceleration");
        #[cfg(not(feature = "cuda"))]
        tracing::info!("Whisper model loaded (GPU acceleration requires --features cuda)");

        Ok(ctx)
    }

    /// Load the model with CPU only (fallback)
    fn load_with_cpu(model_str: &str) -> Result<WhisperContext> {
        let mut params = WhisperContextParameters::default();
        params.use_gpu(false);

        let ctx = WhisperContext::new_with_params(model_str, params)
            .map_err(|e| anyhow!("Failed to load Whisper model with CPU: {:?}", e))?;

        tracing::info!("Whisper model loaded with CPU backend (GPU fallback)");
        Ok(ctx)
    }

    /// Transcribe audio from a WAV file
    ///
    /// The file should be 16kHz mono WAV. Returns the transcribed text.
    pub fn transcribe(&self, audio_path: &Path) -> Result<String> {
        let (samples, sample_rate) = load_wav_samples(audio_path)?;

        tracing::info!(
            "Loaded audio: {} samples at {}Hz ({:.2}s)",
            samples.len(),
            sample_rate,
            samples.len() as f32 / sample_rate as f32
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

        // Resample if needed
        let samples = if sample_rate != 16000 {
            tracing::info!("Resampling from {}Hz to 16000Hz", sample_rate);
            resample_audio(&samples, sample_rate, 16000)
        } else {
            samples
        };

        // Append 1 second of silence so the model can finalise punctuation
        // at the end of the utterance. Only pad if the result stays under the
        // single-chunk limit (15 s / 240 000 samples).
        let samples = {
            const TRAILING_SILENCE: usize = 16_000; // 1 s at 16 kHz
            const MAX_SINGLE_CHUNK: usize = 240_000; // 15 s at 16 kHz
            if samples.len() + TRAILING_SILENCE <= MAX_SINGLE_CHUNK {
                let mut padded = samples;
                padded.extend(std::iter::repeat(0.0f32).take(TRAILING_SILENCE));
                padded
            } else {
                samples
            }
        };

        self.transcribe_samples(&samples)
    }

    /// Transcribe audio samples directly
    ///
    /// Samples should be 16kHz f32 mono audio.
    pub fn transcribe_samples(&self, samples: &[f32]) -> Result<String> {
        let start = std::time::Instant::now();

        // Create a state for this transcription
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| anyhow!("Failed to create whisper state: {:?}", e))?;

        // Configure transcription parameters
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // English language for speed (no language detection)
        params.set_language(Some("en"));

        // Disable translation, we want transcription
        params.set_translate(false);

        // No timestamps needed for basic transcription
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Single segment mode for faster processing
        params.set_single_segment(false);

        // Run transcription
        state
            .full(params, samples)
            .map_err(|e| anyhow!("Transcription failed: {:?}", e))?;

        // Collect results using the iterator API
        let mut text = String::new();
        for segment in state.as_iter() {
            if let Ok(segment_text) = segment.to_str() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(segment_text);
            }
        }

        let duration = start.elapsed();
        let audio_duration = samples.len() as f32 / 16000.0;
        let rtf = duration.as_secs_f32() / audio_duration;

        tracing::info!(
            "Transcribed {:.2}s audio in {:.2}s (RTF: {:.3})",
            audio_duration,
            duration.as_secs_f32(),
            rtf
        );

        tracing::info!(
            "Transcription result: '{}' ({} chars)",
            text.trim(),
            text.len()
        );

        Ok(text.trim().to_string())
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

/// Simple linear resampling (for basic sample rate conversion)
fn resample_audio(samples: &[f32], from_rate: u32, to_rate: u32) -> Vec<f32> {
    if from_rate == to_rate {
        return samples.to_vec();
    }

    let ratio = from_rate as f64 / to_rate as f64;
    let new_len = (samples.len() as f64 / ratio) as usize;
    let mut output = Vec::with_capacity(new_len);

    for i in 0..new_len {
        let src_idx = i as f64 * ratio;
        let idx = src_idx as usize;
        let frac = src_idx - idx as f64;

        let sample = if idx + 1 < samples.len() {
            samples[idx] * (1.0 - frac as f32) + samples[idx + 1] * frac as f32
        } else if idx < samples.len() {
            samples[idx]
        } else {
            0.0
        };

        output.push(sample);
    }

    output
}

/// Get the default whisper model directory path
pub fn get_whisper_model_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| {
            tracing::error!("Could not determine home directory, using /tmp");
            PathBuf::from("/tmp")
        })
        .join(".thoth")
        .join("models")
        .join("whisper")
}

/// Get the path to a specific whisper model
pub fn get_whisper_model_path(model_id: &str) -> PathBuf {
    get_whisper_model_directory().join(format!("{}.bin", model_id))
}

/// Check if a whisper model is downloaded
pub fn is_whisper_model_downloaded(model_id: &str) -> bool {
    get_whisper_model_path(model_id).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_whisper_model_directory() {
        let dir = get_whisper_model_directory();
        assert!(dir.ends_with("whisper"));
        assert!(dir.to_string_lossy().contains(".thoth"));
    }

    #[test]
    fn test_get_whisper_model_path() {
        let path = get_whisper_model_path("ggml-large-v3-turbo");
        assert!(path.to_string_lossy().contains("ggml-large-v3-turbo.bin"));
    }

    #[test]
    fn test_resample_same_rate() {
        let samples = vec![1.0, 2.0, 3.0, 4.0];
        let result = resample_audio(&samples, 16000, 16000);
        assert_eq!(result, samples);
    }
}
