//! Parakeet TDT transcription using Sherpa-ONNX
//!
//! Uses the Parakeet V3 NeMo Transducer model for high-quality
//! offline speech recognition.
//!
//! Requires the `parakeet` Cargo feature to be enabled.

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

        // Trim leading silence for long recordings (the tail is always kept).
        let (trim_start, trim_end) = crate::audio::vad::trim_silence(&samples, sample_rate);
        let samples = samples[trim_start..trim_end].to_vec();

        // The int8 CPU transducer is unreliable on very quiet input — it can
        // return nothing at all. Boost low-level recordings to a consistent peak
        // before transcribing. Linux only; macOS (CoreML) handles quiet audio,
        // so its path is left untouched.
        #[cfg(target_os = "linux")]
        let samples = normalize_peak(samples);

        if sample_rate != 16000 {
            tracing::warn!(
                "Audio sample rate is {}Hz, expected 16000Hz. Results may be affected.",
                sample_rate
            );
        }

        let start = std::time::Instant::now();

        // Linux runs the CPU/int8 transducer, which degrades — and sometimes
        // returns *nothing* — on long single-pass sequences, so it chunks and
        // retries (see `transcribe_linux`). macOS runs Metal and handles the
        // whole utterance in one pass.
        #[cfg(target_os = "linux")]
        let text = self.transcribe_linux(&samples, sample_rate);

        #[cfg(not(target_os = "linux"))]
        let text = self.transcribe_padded(&samples, sample_rate);

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

    /// Pad audio with a little dead air on each side and run one transducer pass.
    ///
    /// A short lead (0.5 s) avoids clipping a soft onset; a longer tail (1.5 s)
    /// keeps the transducer from dropping the final word.
    fn transcribe_padded(&mut self, audio: &[f32], sample_rate: u32) -> String {
        const LEADING_SILENCE: usize = 8_000; // 0.5 s at 16 kHz
        const TRAILING_SILENCE: usize = 24_000; // 1.5 s at 16 kHz
        let mut padded = Vec::with_capacity(LEADING_SILENCE + audio.len() + TRAILING_SILENCE);
        padded.extend(std::iter::repeat_n(0.0f32, LEADING_SILENCE));
        padded.extend_from_slice(audio);
        padded.extend(std::iter::repeat_n(0.0f32, TRAILING_SILENCE));
        self.recognizer.transcribe(sample_rate, &padded)
    }

    /// Linux transcription path: split long audio at silence, retry empties.
    ///
    /// The CPU/int8 transducer is reliable on short, healthy-level clips but can
    /// return garbled or empty text on long ones, so audio longer than ~15 s is
    /// cut into pieces at the quietest point near each boundary — never mid-word
    /// — and transcribed independently. Because the cuts land in a pause there's
    /// no overlap and nothing to de-duplicate; the pieces are just concatenated.
    #[cfg(target_os = "linux")]
    fn transcribe_linux(&mut self, samples: &[f32], sample_rate: u32) -> String {
        const CHUNK_SECS: usize = 15;
        // Roughly the trailing pad `transcribe_padded` adds; keeps the
        // single-pass cutoff a touch above the chunk size.
        const TRAILING_SILENCE: usize = 24_000;

        let chunk_samples = CHUNK_SECS * sample_rate as usize;

        if samples.len() <= chunk_samples + TRAILING_SILENCE {
            return self.transcribe_resilient(samples, sample_rate);
        }

        // Hunt for the cut within ±1.5 s of each target boundary.
        let radius = sample_rate as usize * 3 / 2;

        let mut segments: Vec<String> = Vec::new();
        let mut offset = 0;
        let total = samples.len();

        while offset < total {
            let end = if total - offset <= chunk_samples + TRAILING_SILENCE {
                total
            } else {
                find_quiet_split(samples, offset + chunk_samples, radius, sample_rate)
            };
            let chunk = &samples[offset..end];

            let segment_text = self.transcribe_resilient(chunk, sample_rate);
            tracing::info!(
                "Parakeet chunk {}: {:.1}s–{:.1}s → '{}' ({} chars)",
                segments.len() + 1,
                offset as f32 / sample_rate as f32,
                end as f32 / sample_rate as f32,
                segment_text.chars().take(60).collect::<String>(),
                segment_text.len()
            );
            if !segment_text.trim().is_empty() {
                segments.push(segment_text.trim().to_string());
            }

            offset = end;
        }

        segments.join(" ")
    }

    /// Transcribe one segment, retrying split-at-silence on a spurious empty.
    ///
    /// If the transducer returns nothing for audio that clearly contains signal,
    /// it glitched rather than heard silence — cut the segment at its quietest
    /// interior point (so no word is split) and retry each half. Bounded by a 4 s
    /// floor so genuinely-empty audio doesn't recurse forever.
    #[cfg(target_os = "linux")]
    fn transcribe_resilient(&mut self, audio: &[f32], sample_rate: u32) -> String {
        let text = self.transcribe_padded(audio, sample_rate);
        if !text.trim().is_empty() {
            return text;
        }

        let min_retry_samples = 4 * sample_rate as usize;
        let max_level = audio.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        if max_level < 0.01 || audio.len() <= min_retry_samples {
            return text; // genuinely silent, or too short to bother splitting
        }

        tracing::warn!(
            "Parakeet returned empty for non-silent {:.1}s segment (max {:.3}); retrying split",
            audio.len() as f32 / sample_rate as f32,
            max_level
        );

        // Cut at the quietest point in the middle third so a word isn't sliced.
        let radius = audio.len() / 3;
        let split = find_quiet_split(audio, audio.len() / 2, radius, sample_rate);
        let left = self.transcribe_resilient(&audio[..split], sample_rate);
        let right = self.transcribe_resilient(&audio[split..], sample_rate);
        format!("{} {}", left.trim(), right.trim())
            .trim()
            .to_string()
    }

    /// Transcribe audio samples directly
    ///
    /// Samples should be 16kHz f32 mono audio.
    pub fn transcribe_samples(&mut self, samples: &[f32]) -> String {
        self.recognizer.transcribe(16000, samples)
    }
}

/// Scale very quiet audio up to a consistent peak level.
///
/// The int8 CPU transducer can return nothing for low-amplitude input (a quiet
/// mic or a distant speaker). Boosting the peak to a healthy level before
/// transcription makes it behave regardless of input gain. Audio that's already
/// at a reasonable level — or essentially silent — is left alone so we don't
/// amplify noise into speech.
#[cfg(target_os = "linux")]
fn normalize_peak(mut samples: Vec<f32>) -> Vec<f32> {
    const TARGET_PEAK: f32 = 0.5;
    const MIN_PEAK: f32 = 0.01;
    let peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
    if (MIN_PEAK..TARGET_PEAK).contains(&peak) {
        let gain = TARGET_PEAK / peak;
        for s in &mut samples {
            *s *= gain;
        }
        tracing::info!(
            "Normalised quiet audio: peak {:.3} → {:.2} (gain {:.1}×)",
            peak,
            TARGET_PEAK,
            gain
        );
    }
    samples
}

/// Find a low-energy cut point near `target` so a chunk boundary lands in a
/// pause rather than mid-word.
///
/// Scans 20 ms frames within `radius` samples of `target` and returns the centre
/// of the quietest one. Falls back to `target` (clamped) when the window is too
/// small to scan.
#[cfg(target_os = "linux")]
fn find_quiet_split(samples: &[f32], target: usize, radius: usize, sample_rate: u32) -> usize {
    let frame = (sample_rate as usize / 50).max(1); // 20 ms
    let lo = target.saturating_sub(radius);
    let hi = (target + radius).min(samples.len());
    if hi < lo + frame {
        return target.min(samples.len());
    }

    let mut best = target.min(samples.len());
    let mut best_energy = f32::MAX;
    let mut i = lo;
    while i + frame <= hi {
        let energy: f32 = samples[i..i + frame].iter().map(|s| s * s).sum();
        if energy < best_energy {
            best_energy = energy;
            best = i + frame / 2;
        }
        i += frame;
    }
    best
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

    #[cfg(target_os = "linux")]
    #[test]
    fn test_normalize_peak_boosts_quiet_audio() {
        let samples = vec![0.05f32, -0.05, 0.025, -0.05];
        let out = normalize_peak(samples);
        let peak = out.iter().map(|s| s.abs()).fold(0.0f32, f32::max);
        assert!((peak - 0.5).abs() < 1e-4, "peak should be boosted to 0.5");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_normalize_peak_leaves_healthy_audio() {
        let samples = vec![0.6f32, -0.7, 0.5];
        let out = normalize_peak(samples.clone());
        assert_eq!(out, samples, "already-loud audio is untouched");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_normalize_peak_leaves_near_silence() {
        let samples = vec![0.001f32, -0.002, 0.0005];
        let out = normalize_peak(samples.clone());
        assert_eq!(out, samples, "near-silent audio is not amplified");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn test_find_quiet_split_lands_in_pause() {
        let sr = 16_000u32;
        // 4 s of signal with a 100 ms silent gap centred at 2.0 s.
        let mut samples = vec![0.5f32; sr as usize * 4];
        let gap_start = sr as usize * 2;
        let gap_len = sr as usize / 10;
        for s in &mut samples[gap_start..gap_start + gap_len] {
            *s = 0.0;
        }
        let split = find_quiet_split(&samples, sr as usize * 2, sr as usize, sr);
        assert!(
            split >= gap_start && split < gap_start + gap_len,
            "split {split} should land inside the silent gap [{gap_start}, {})",
            gap_start + gap_len
        );
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
