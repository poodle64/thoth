//! FluidAudio transcription using Apple Neural Engine via CoreML
//!
//! Runs Parakeet TDT models on the Apple Neural Engine (ANE) for
//! dramatically faster transcription (~210x real-time factor).
//!
//! Requires the `fluidaudio` Cargo feature and macOS with Apple Silicon.
//!
//! # NOTE — empirically load-bearing constants and invariants
//!
//! The segmentation constants (`SINGLE_SHOT_MAX_SECS`, `SEGMENT_TARGET_SECS`,
//! pad lengths) and the no-normalisation invariant in `write_padded_wav` are
//! empirically tuned to work around a non-monotonic failure mode in FluidAudio
//! 0.15's chunked decoder: depending on recording length the decoder either
//! drops the tail or appends a hallucinated filler token. Padding-only tuning
//! merely moves which lengths fail. Any change to this decode path MUST be
//! verified via a LIVE APP recording at 19 s, 52 s, 144 s, and ~5 min.
//! Unit tests and the fork's standalone CLI example do NOT exercise the full
//! decode path (they skip the app's pad + WAV-write step).

#![cfg(all(target_os = "macos", feature = "fluidaudio"))]

use anyhow::{Result, anyhow};
use fluidaudio_rs::FluidAudio;
use std::path::{Path, PathBuf};

/// Maximum audio length (seconds, before padding) handed to FluidAudio as a
/// single unit.
///
/// FluidAudio 0.15 decodes audio up to its single-shot limit (15.0 s /
/// 240 000 samples at 16 kHz) in one reliable pass. Longer audio takes its
/// sliding-window chunk processor, whose *final* chunk decodes unreliably:
/// depending on where the last boundary lands relative to the audio length it
/// either drops the tail (a long recording loses its closing words) or appends
/// a hallucinated filler token ("Okay", "Thank you") on the trailing low-energy
/// frames. Tuning the padding only moves which lengths land badly; the only
/// robust fix is to keep every unit at or below the single-shot limit. With the
/// padding below a unit of this length stays under 240 000 samples.
const SINGLE_SHOT_MAX_SECS: f32 = 14.4;

/// Target maximum length (seconds, before padding) of each segment when a
/// recording must be split. Comfortably under [`SINGLE_SHOT_MAX_SECS`] so a
/// padded segment still fits the single-shot decoder.
const SEGMENT_TARGET_SECS: f32 = 13.0;

/// Minimum segment length (seconds) when splitting; avoids tiny fragments and
/// keeps the cut search from landing immediately after the previous boundary.
const SEGMENT_MIN_SECS: f32 = 4.0;

/// Leading silence padding (seconds) wrapped around every unit so the TDT
/// decoder has run-in room before the first word.
const LEAD_PAD_SECS: f32 = 0.25;

/// Trailing silence padding (seconds) so the decoder has run-out room to
/// finalise the last word.
const TRAIL_PAD_SECS: f32 = 0.25;

/// Analysis frame length (seconds) for silence-gap detection.
const GAP_FRAME_SECS: f32 = 0.02;

/// Minimum pause length (seconds) that qualifies as a segment cut point.
const GAP_MIN_SECS: f32 = 0.30;

/// A frame counts as silent below this fraction of the recording's speech
/// level. Relative so the detector adapts to quiet lapel-mic recordings.
const GAP_REL_THRESHOLD: f32 = 0.10;

/// Absolute RMS floor below which a frame is silent regardless of the relative
/// threshold; guards against a near-silent recording producing a speech level
/// of ~0 and flagging everything as silence.
const GAP_ABS_FLOOR: f32 = 0.004;

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

    /// Transcribe audio from a WAV file.
    ///
    /// Keeps every unit handed to FluidAudio at or below the single-shot decode
    /// limit (see [`SINGLE_SHOT_MAX_SECS`]). Short recordings transcribe in one
    /// pass; longer ones are split at natural pauses into single-shot segments
    /// whose transcripts are joined with a space. This avoids FluidAudio 0.15's
    /// unreliable final-chunk decode, which otherwise drops the tail of long
    /// recordings or appends a hallucinated trailing filler token.
    pub fn transcribe(&self, audio_path: &Path) -> Result<String> {
        let (samples, spec) = match read_wav_mono(audio_path) {
            Ok(loaded) => loaded,
            Err(e) => {
                // We couldn't parse the WAV ourselves; hand the original file to
                // FluidAudio rather than failing the transcription outright.
                tracing::warn!(
                    "Could not read WAV for segmentation ({e}); sending original to FluidAudio"
                );
                return self.transcribe_one(audio_path);
            }
        };

        let segments = plan_segments(&samples, spec.sample_rate);
        let total_secs = samples.len() as f32 / spec.sample_rate as f32;
        tracing::info!(
            "FluidAudio: {:.1}s recording → {} segment(s) (single-shot limit {:.1}s)",
            total_secs,
            segments.len(),
            SINGLE_SHOT_MAX_SECS,
        );

        let start = std::time::Instant::now();
        let mut parts: Vec<String> = Vec::with_capacity(segments.len());
        for (i, &(begin, end)) in segments.iter().enumerate() {
            let tmp = audio_path.with_extension(format!("seg{i}.wav"));
            write_padded_wav(&samples[begin..end], &spec, &tmp)?;
            let text = self.transcribe_one(&tmp);
            let _ = std::fs::remove_file(&tmp);

            let text = text?;
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }

        let joined = parts.join(" ");
        tracing::info!(
            "FluidAudio transcript: {} chars, {} words from {} segment(s) in {:.3}s",
            joined.len(),
            joined.split_whitespace().count(),
            segments.len(),
            start.elapsed().as_secs_f32(),
        );
        Ok(joined)
    }

    /// Transcribe a single WAV file (one unit, no further segmentation).
    fn transcribe_one(&self, path: &Path) -> Result<String> {
        let result = self
            .audio
            .transcribe_file(path)
            .map_err(|e| anyhow!("FluidAudio transcription failed: {}", e))?;
        Ok(result.text)
    }
}

/// Read a WAV file as mono `f32` samples in `[-1.0, 1.0]`.
///
/// If the file has more than one channel, only the first is kept (recordings
/// are mono, but this stays correct if that ever changes). Returns the samples
/// alongside the file's [`hound::WavSpec`] so segments can be written back in
/// the same format.
fn read_wav_mono(path: &Path) -> Result<(Vec<f32>, hound::WavSpec)> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = (spec.channels as usize).max(1);

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = (1i64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .step_by(channels)
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<std::result::Result<_, _>>()?
        }
        hound::SampleFormat::Float => reader
            .samples::<f32>()
            .step_by(channels)
            .collect::<std::result::Result<_, _>>()?,
    };

    Ok((samples, spec))
}

/// Plan the segments to transcribe for a recording.
///
/// Recordings within the single-shot limit are returned as a single
/// `(0, len)` range. Longer recordings are split greedily at silence: each
/// segment runs to the last detected pause within
/// `[pos + SEGMENT_MIN, pos + SEGMENT_TARGET]`, falling back to a hard cut at
/// `SEGMENT_TARGET` if that window holds no pause. Cutting at pauses keeps
/// segment boundaries off mid-word positions, so joining the transcripts loses
/// no audio.
fn plan_segments(samples: &[f32], rate: u32) -> Vec<(usize, usize)> {
    let total = samples.len();
    if total == 0 {
        return vec![(0, 0)];
    }
    if total as f32 / rate as f32 <= SINGLE_SHOT_MAX_SECS {
        return vec![(0, total)];
    }

    let cuts = find_silence_cuts(samples, rate);
    let target = (SEGMENT_TARGET_SECS * rate as f32) as usize;
    let min_len = (SEGMENT_MIN_SECS * rate as f32) as usize;

    let mut segments = Vec::new();
    let mut pos = 0usize;
    while total - pos > target {
        let window_lo = pos + min_len;
        let window_hi = pos + target;
        let cut = cuts
            .iter()
            .copied()
            .rfind(|&c| c > window_lo && c <= window_hi)
            .unwrap_or(window_hi);
        segments.push((pos, cut));
        pos = cut;
    }
    segments.push((pos, total));
    segments
}

/// Find candidate cut points (sample offsets) at silent gaps in the audio.
///
/// Computes per-frame RMS, derives an adaptive silence threshold from the
/// recording's speech level (so it works on quiet lapel-mic audio), and returns
/// the midpoint of every silent run lasting at least [`GAP_MIN_SECS`].
fn find_silence_cuts(samples: &[f32], rate: u32) -> Vec<usize> {
    let frame = ((GAP_FRAME_SECS * rate as f32) as usize).max(1);

    let mut rms: Vec<f32> = Vec::with_capacity(samples.len() / frame + 1);
    let mut i = 0;
    while i + frame <= samples.len() {
        let energy: f32 = samples[i..i + frame].iter().map(|s| s * s).sum();
        rms.push((energy / frame as f32).sqrt());
        i += frame;
    }
    if rms.is_empty() {
        return Vec::new();
    }

    let mut sorted = rms.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let speech_level = sorted[((sorted.len() as f32 * 0.90) as usize).min(sorted.len() - 1)];
    let threshold = (speech_level * GAP_REL_THRESHOLD).max(GAP_ABS_FLOOR);
    let min_gap_frames = ((GAP_MIN_SECS / GAP_FRAME_SECS) as usize).max(1);

    let mut cuts = Vec::new();
    let mut run_start: Option<usize> = None;
    for (fi, &v) in rms.iter().enumerate() {
        if v < threshold {
            run_start.get_or_insert(fi);
        } else if let Some(s) = run_start.take() {
            if fi - s >= min_gap_frames {
                cuts.push((s + fi) / 2 * frame);
            }
        }
    }
    if let Some(s) = run_start {
        if rms.len() - s >= min_gap_frames {
            cuts.push((s + rms.len()) / 2 * frame);
        }
    }
    cuts
}

/// Write `samples` to `path`, wrapped in leading and trailing silence.
///
/// The padding gives the TDT decoder run-in and run-out room. The audio is
/// copied through unchanged: an earlier version RMS-normalised it (to lift quiet
/// lapel-mic recordings), but with FluidAudio 0.15 that gain shifted the
/// silence-aligned chunk boundaries and dropped the tail of long recordings;
/// the level is left alone.
fn write_padded_wav(samples: &[f32], spec: &hound::WavSpec, path: &Path) -> Result<()> {
    let rate = spec.sample_rate;
    let lead = (LEAD_PAD_SECS * rate as f32) as usize;
    let trail = (TRAIL_PAD_SECS * rate as f32) as usize;

    let mut writer = hound::WavWriter::create(path, *spec)?;
    match spec.sample_format {
        hound::SampleFormat::Int => {
            let scale = ((1i64 << (spec.bits_per_sample - 1)) - 1) as f32;
            for _ in 0..lead {
                writer.write_sample(0i32)?;
            }
            for &s in samples {
                writer.write_sample((s * scale).round().clamp(-scale, scale) as i32)?;
            }
            for _ in 0..trail {
                writer.write_sample(0i32)?;
            }
        }
        hound::SampleFormat::Float => {
            for _ in 0..lead {
                writer.write_sample(0.0f32)?;
            }
            for &s in samples {
                writer.write_sample(s)?;
            }
            for _ in 0..trail {
                writer.write_sample(0.0f32)?;
            }
        }
    }
    writer.finalize()?;
    Ok(())
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

    /// A recording within the single-shot limit is transcribed as one unit.
    #[test]
    fn test_plan_segments_short_is_single() {
        let rate = 16_000u32;
        let samples = vec![0.1f32; (10.0 * rate as f32) as usize]; // 10 s
        assert_eq!(plan_segments(&samples, rate), vec![(0, samples.len())]);
    }

    /// A long recording is split into several units, each within the target
    /// length, and the segments tile the whole recording with no gaps.
    #[test]
    fn test_plan_segments_long_splits_and_covers() {
        let rate = 16_000u32;
        let frame = (GAP_FRAME_SECS * rate as f32) as usize;
        // 60 s alternating 2 s speech / 0.5 s silence so there are real pauses.
        let mut samples = Vec::new();
        while samples.len() < (60.0 * rate as f32) as usize {
            let speech_frames = (2.0 * rate as f32) as usize;
            let silence_frames = (0.5 * rate as f32) as usize;
            samples.extend(std::iter::repeat_n(0.3f32, speech_frames));
            samples.extend(std::iter::repeat_n(0.0f32, silence_frames));
        }
        let segs = plan_segments(&samples, rate);
        assert!(segs.len() > 1, "long recording should split");
        let target = (SEGMENT_TARGET_SECS * rate as f32) as usize;
        assert_eq!(segs.first().unwrap().0, 0);
        assert_eq!(segs.last().unwrap().1, samples.len());
        for (i, &(a, b)) in segs.iter().enumerate() {
            assert!(b > a, "segment must be non-empty");
            if i + 1 < segs.len() {
                assert_eq!(b, segs[i + 1].0, "segments must be contiguous");
                assert!(b - a <= target + frame, "non-final segment within target");
            }
        }
    }

    /// Silence-gap detection finds the pause between two speech bursts.
    #[test]
    fn test_find_silence_cuts_detects_pause() {
        let rate = 16_000u32;
        let mut samples = vec![0.3f32; (2.0 * rate as f32) as usize]; // 2 s speech
        samples.extend(vec![0.0f32; (0.5 * rate as f32) as usize]); // 0.5 s silence
        samples.extend(vec![0.3f32; (2.0 * rate as f32) as usize]); // 2 s speech
        let cuts = find_silence_cuts(&samples, rate);
        assert_eq!(cuts.len(), 1, "exactly one gap");
        let cut_secs = cuts[0] as f32 / rate as f32;
        assert!(
            (2.0..2.5).contains(&cut_secs),
            "cut should fall inside the pause, got {cut_secs}s"
        );
    }

    /// Padding writes a readable WAV whose length is the input plus the pads.
    #[test]
    fn test_write_padded_wav_roundtrip() {
        let rate = 16_000u32;
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let samples = vec![0.2f32; rate as usize]; // 1 s
        let tmp = std::env::temp_dir().join("thoth_pad_test.wav");
        write_padded_wav(&samples, &spec, &tmp).unwrap();

        let (read_back, _) = read_wav_mono(&tmp).unwrap();
        let expected = samples.len()
            + (LEAD_PAD_SECS * rate as f32) as usize
            + (TRAIL_PAD_SECS * rate as f32) as usize;
        assert_eq!(read_back.len(), expected);
        let _ = std::fs::remove_file(&tmp);
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
