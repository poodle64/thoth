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

        let text = result.text.clone();
        let word_count = text.split_whitespace().count();
        tracing::info!(
            "FluidAudio transcript: {} chars, {} words (decoded {:.2}s audio in {:.3}s, RTFx: {:.0}, confidence: {:.1}%)",
            text.len(),
            word_count,
            result.duration,
            duration.as_secs_f32(),
            result.rtfx,
            result.confidence * 100.0
        );
        Ok(text)
    }
}

/// Compute the normalisation gain for a clip given its RMS and peak amplitude.
///
/// The goal is to lift quiet recordings (e.g. from a lapel mic with overall RMS
/// ~0.01) so the soft trailing words clear the TDT decoder's blank threshold,
/// without clipping loud recordings or amplifying pure silence.
///
/// Rules:
/// - Target RMS is 0.05; gain = target / rms (or 1.0 when rms is below noise floor).
/// - Cap gain so peak * gain ≤ 0.95 (never clip).
/// - Never attenuate: loud recordings get gain = 1.0 (only boost, never reduce).
pub(crate) fn normalisation_gain(rms: f32, peak: f32) -> f32 {
    const TARGET_RMS: f32 = 0.05;
    const NOISE_FLOOR: f32 = 1e-6;
    const MAX_PEAK: f32 = 0.95;

    let mut gain = if rms > NOISE_FLOOR {
        TARGET_RMS / rms
    } else {
        // Pure silence — don't amplify noise
        1.0
    };

    // Cap so we don't clip
    if peak > NOISE_FLOOR {
        gain = gain.min(MAX_PEAK / peak);
    }

    // Only boost; never attenuate a recording that is already loud enough
    gain.max(1.0)
}

/// Pad a WAV file with leading silence and low-level trailing dither, writing a
/// temporary copy.  The original audio is RMS-normalised before writing so that
/// soft trailing words (common on lapel mics) clear the TDT decoder's blank
/// threshold instead of triggering its early-exit.
///
/// Padding:
/// - 500 ms of hard-zero leading silence (gives the model time to initialise).
/// - 0.75 s of low-level dither trailing (avoids a run of consecutive blanks that
///   fires the decoder's early-exit before it emits the final words).
///
/// Returns `Ok(Some(path))` with the padded temp file, or `Ok(None)` if the
/// original file couldn't be read (in which case FluidAudio gets the original).
fn pad_with_silence(audio_path: &Path) -> Result<Option<PathBuf>> {
    let reader = match hound::WavReader::open(audio_path) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Could not read WAV for padding: {}, using original", e);
            return Ok(None);
        }
    };

    let spec = reader.spec();
    tracing::info!(
        "FluidAudio pad_with_silence: loaded {:.2}s ({} frames) at {}Hz from {}",
        reader.duration() as f32 / spec.sample_rate as f32,
        reader.duration(),
        spec.sample_rate,
        audio_path.display()
    );
    let sample_rate = spec.sample_rate;
    let leading_samples = sample_rate as usize / 2; // 500 ms
    let trailing_samples = sample_rate as usize * 3 / 4; // 0.75 s

    // Collect all original samples as f32 in [-1.0, 1.0] so we can compute
    // statistics for normalisation before writing.  We must collect first
    // because `into_samples()` consumes the reader.
    let samples_f32: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => reader
            .into_samples::<i16>()
            .filter_map(|s| s.ok())
            .map(|s| s as f32 / 32768.0)
            .collect(),
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(|s| s.ok())
            .collect(),
    };

    // RMS and peak over all collected samples
    let rms = if samples_f32.is_empty() {
        0.0
    } else {
        let mean_sq = samples_f32.iter().map(|&s| s * s).sum::<f32>() / samples_f32.len() as f32;
        mean_sq.sqrt()
    };
    let peak = samples_f32.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

    let gain = normalisation_gain(rms, peak);
    tracing::info!(
        "FluidAudio normalise: rms={:.4} peak={:.4} gain={:.2}x",
        rms,
        peak,
        gain
    );

    // Build temp path next to the original
    let tmp_path = audio_path.with_extension("padded.wav");

    let mut writer = hound::WavWriter::create(&tmp_path, spec)?;

    // Cheap deterministic dither: sine-derived so it's non-repeating over short
    // windows but totally reproducible.  Amplitude 0.001 is far below the speech
    // floor yet enough to keep the decoder out of its consecutive-blank path.
    let dither_f32 = |i: usize| -> f32 { (i as f32 * 0.000_123_f32).sin() * 0.001 };

    let channels = spec.channels as usize;

    match spec.sample_format {
        hound::SampleFormat::Int => {
            // Leading hard-zero silence
            for _ in 0..leading_samples * channels {
                writer.write_sample(0i16)?;
            }
            // Normalised original audio
            for &s in &samples_f32 {
                let out = (s * gain).clamp(-1.0, 1.0);
                writer.write_sample((out * 32767.0).round() as i16)?;
            }
            // Trailing low-level dither
            for i in 0..trailing_samples * channels {
                let out = dither_f32(i);
                writer.write_sample((out * 32767.0).round() as i16)?;
            }
        }
        hound::SampleFormat::Float => {
            // Leading hard-zero silence
            for _ in 0..leading_samples * channels {
                writer.write_sample(0.0f32)?;
            }
            // Normalised original audio
            for &s in &samples_f32 {
                let out = (s * gain).clamp(-1.0, 1.0);
                writer.write_sample(out)?;
            }
            // Trailing low-level dither
            for i in 0..trailing_samples * channels {
                writer.write_sample(dither_f32(i))?;
            }
        }
    }

    writer.finalize()?;

    tracing::info!(
        "Padded WAV with {:.1}s leading silence + {:.1}s trailing dither: {}",
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

    // --- normalisation_gain unit tests ---

    /// DJI MIC MINI real-world fixture: rms=0.011, peak=0.082.
    /// target_rms / rms = 0.05 / 0.011 ≈ 4.545; peak cap = 0.95 / 0.082 ≈ 11.59.
    /// Peak cap does not bind; max(1.0) does not bind.  Expected gain ≈ 4.545.
    #[test]
    fn test_gain_typical_quiet_clip() {
        let gain = normalisation_gain(0.011, 0.082);
        assert!(
            (gain - 4.545).abs() < 0.01,
            "expected ~4.545, got {gain:.4}"
        );
    }

    /// Pure silence: rms below noise floor → gain = 1.0 (no amplification).
    #[test]
    fn test_gain_pure_silence() {
        let gain = normalisation_gain(0.0, 0.0);
        assert_eq!(gain, 1.0, "silence should return gain 1.0");
    }

    /// Loud clip: rms=0.1, peak=0.9.  target/rms = 0.5, which is below 1.0,
    /// so max(1.0) applies → gain = 1.0 (never attenuate).
    #[test]
    fn test_gain_loud_clip_not_attenuated() {
        let gain = normalisation_gain(0.1, 0.9);
        assert_eq!(gain, 1.0, "loud clip should not be attenuated");
    }

    /// Peak-cap case: rms=0.001, peak=0.5.
    /// target/rms = 0.05 / 0.001 = 50; peak cap = 0.95 / 0.5 = 1.9.
    /// Peak cap binds → gain = 1.9; max(1.0) does not further reduce it.
    #[test]
    fn test_gain_peak_cap_binds() {
        let gain = normalisation_gain(0.001, 0.5);
        assert!(
            (gain - 1.9).abs() < 0.001,
            "expected peak-capped gain ~1.9, got {gain:.4}"
        );
    }
}
