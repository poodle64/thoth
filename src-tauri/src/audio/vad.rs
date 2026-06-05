//! Voice Activity Detection (VAD) for speech boundary detection
//!
//! This module provides VAD functionality using webrtc-vad to detect speech
//! boundaries. The primary export is `trim_silence`, used by the transcription
//! pipeline to strip *leading* silence (dead air before the user starts
//! talking) from long recordings. The trailing edge is never trimmed — see
//! `trim_silence` for why.

use serde::{Deserialize, Serialize};
use webrtc_vad::{SampleRate, Vad, VadMode};

/// Frame duration for VAD processing
///
/// WebRTC VAD supports 10ms, 20ms, or 30ms frames.
/// At 16kHz sample rate:
/// - 10ms = 160 samples
/// - 20ms = 320 samples
/// - 30ms = 480 samples
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadFrameDuration {
    /// 10ms frame (160 samples at 16kHz)
    Ms10 = 10,
    /// 20ms frame (320 samples at 16kHz)
    Ms20 = 20,
    /// 30ms frame (480 samples at 16kHz)
    #[default]
    Ms30 = 30,
}

impl VadFrameDuration {
    /// Returns the number of samples for this frame duration at 16kHz
    pub const fn samples_at_16khz(&self) -> usize {
        match self {
            VadFrameDuration::Ms10 => 160,
            VadFrameDuration::Ms20 => 320,
            VadFrameDuration::Ms30 => 480,
        }
    }
}

/// Minimum recording duration (in samples) before silence trimming is applied.
/// For shorter recordings the VAD overhead isn't worth it.
/// 20 seconds × 16 000 Hz = 320 000 samples.
const TRIM_MIN_SAMPLES_16KHZ: usize = 320_000;

/// Safety margin (in seconds) kept before the first detected speech frame so we
/// don't clip into the start of an utterance. 500 ms gives enough room for a
/// soft onset.
const TRIM_MARGIN_SECS: f32 = 0.5;

/// Trim leading silence from audio samples using VAD.
///
/// For recordings shorter than ~20 seconds (at the given sample rate) the
/// original slice is returned unchanged because the VAD overhead is not worth
/// the saving.
///
/// The function runs a single-pass VAD scan over the audio, locates the first
/// speech frame, and returns a sub-slice that starts a safety margin (500 ms)
/// before it. **The trailing edge is never trimmed** — `end` is always
/// `samples.len()`.
///
/// The trailing edge is never trimmed: WebRTC VAD in aggressive mode tags the
/// quiet tail of a sentence (especially on a lapel mic) as non-speech, so
/// trimming there risks slicing off the final words. Leading silence is where
/// the latency saving lives; keeping the whole tail costs only a little decode
/// time and never loses a word.
///
/// # Arguments
/// * `samples` — mono f32 audio samples normalised to [-1.0, 1.0]
/// * `sample_rate` — sample rate in Hz (typically 16 000)
///
/// # Returns
/// A `(start, end)` range into `samples`. Callers should use `samples[start..end]`.
/// `end` is always `samples.len()`. If no speech is detected the full range
/// `(0, samples.len())` is returned so the downstream silence check can decide
/// what to do.
pub fn trim_silence(samples: &[f32], sample_rate: u32) -> (usize, usize) {
    let total = samples.len();

    // Scale threshold proportionally for non-16 kHz audio
    let threshold = (TRIM_MIN_SAMPLES_16KHZ as f64 * sample_rate as f64 / 16_000.0) as usize;
    if total < threshold {
        return (0, total);
    }

    // Use a lightweight VAD config (30 ms frames, aggressive mode, minimal
    // start/end thresholds so we detect the *outermost* speech edges).
    let frame_dur = VadFrameDuration::Ms30;
    let frame_size = frame_dur.samples_at_16khz();

    // If the audio isn't 16 kHz the frame size needs scaling.
    let actual_frame_size = (frame_size as u64 * sample_rate as u64 / 16_000) as usize;
    if actual_frame_size == 0 {
        return (0, total);
    }

    // webrtc-vad only accepts 16 kHz sample rate, so we need to work with
    // 16 kHz-sized frames.  When the input is already 16 kHz we can use the
    // frames directly; otherwise we do a cheap decimation/interpolation per
    // frame.  For the purpose of *detecting* speech boundaries a rough
    // resample is perfectly adequate.
    let need_resample = sample_rate != 16_000;

    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::Aggressive);

    let mut first_speech_frame: Option<usize> = None;

    let mut frame_idx: usize = 0;
    let mut pos: usize = 0;

    // We only need the first speech frame, so stop scanning once we've found it.
    while pos + actual_frame_size <= total && first_speech_frame.is_none() {
        let frame_i16: Vec<i16> = if need_resample {
            // Cheap linear resample of this frame to 16 kHz
            (0..frame_size)
                .map(|i| {
                    let src = pos as f64 + i as f64 * actual_frame_size as f64 / frame_size as f64;
                    let idx = src as usize;
                    let s = if idx < total { samples[idx] } else { 0.0 };
                    (s * 32767.0).clamp(-32768.0, 32767.0) as i16
                })
                .collect()
        } else {
            samples[pos..pos + frame_size]
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect()
        };

        if let Ok(true) = vad.is_voice_segment(&frame_i16) {
            first_speech_frame = Some(frame_idx);
        }

        frame_idx += 1;
        pos += actual_frame_size;
    }

    // No speech found — return the full range so downstream silence detection
    // can handle it (returning an empty range would silently discard audio).
    let Some(first) = first_speech_frame else {
        return (0, total);
    };

    let margin_samples = (TRIM_MARGIN_SECS * sample_rate as f32) as usize;

    let start = (first * actual_frame_size).saturating_sub(margin_samples);
    // The trailing edge is never trimmed — keep every sample through to the end.
    let end = total;

    let original_duration = total as f32 / sample_rate as f32;
    tracing::info!(
        "VAD leading-silence trim: {:.1}s → {:.1}s (removed {:.1}s leading; tail kept in full)",
        original_duration,
        (end - start) as f32 / sample_rate as f32,
        start as f32 / sample_rate as f32,
    );

    (start, end)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frame_duration_samples() {
        assert_eq!(VadFrameDuration::Ms10.samples_at_16khz(), 160);
        assert_eq!(VadFrameDuration::Ms20.samples_at_16khz(), 320);
        assert_eq!(VadFrameDuration::Ms30.samples_at_16khz(), 480);
    }

    #[test]
    fn test_trim_silence_short_recording_unchanged() {
        // 10 seconds at 16 kHz — below the 20-second threshold
        let samples = vec![0.0f32; 160_000];
        let (start, end) = trim_silence(&samples, 16_000);
        assert_eq!(start, 0);
        assert_eq!(end, samples.len());
    }

    #[test]
    fn test_trim_silence_all_silence_returns_full_range() {
        // 30 seconds of silence at 16 kHz — above threshold but no speech
        let samples = vec![0.0f32; 480_000];
        let (start, end) = trim_silence(&samples, 16_000);
        // No speech detected → returns full range
        assert_eq!(start, 0);
        assert_eq!(end, samples.len());
    }

    #[test]
    fn test_trim_silence_trims_leading_only_and_keeps_tail() {
        // 30 seconds at 16 kHz: silence, then speech-like noise, then silence
        let sample_rate = 16_000u32;
        let total_samples = 30 * sample_rate as usize; // 480_000
        let mut samples = vec![0.0f32; total_samples];

        // Put loud noise (simulating speech) from 10s to 20s
        let speech_start = 10 * sample_rate as usize; // 160_000
        let speech_end = 20 * sample_rate as usize; // 320_000
        for (i, sample) in samples
            .iter_mut()
            .enumerate()
            .take(speech_end)
            .skip(speech_start)
        {
            // Generate a tone that VAD will detect as speech
            let t = i as f32 / sample_rate as f32;
            *sample = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
        }

        let (start, end) = trim_silence(&samples, sample_rate);

        // Leading silence is trimmed...
        assert!(start > 0, "Should trim leading silence");
        // ...but the trailing edge is always kept in full, so no real trailing
        // word can ever be sliced away (#46).
        assert_eq!(end, total_samples, "Trailing edge must never be trimmed");

        // The trim must start at or before the speech onset (allowing the margin).
        let margin = (TRIM_MARGIN_SECS * sample_rate as f32) as usize;
        assert!(
            start <= speech_start + margin,
            "Trim start ({start}) should be at or before speech start + margin ({})",
            speech_start + margin
        );
    }

    #[test]
    fn test_trim_silence_non_16khz() {
        // 30 seconds at 48 kHz — tests the proportional threshold scaling
        let sample_rate = 48_000u32;
        let total_samples = 30 * sample_rate as usize;
        let samples = vec![0.0f32; total_samples];

        // Should not panic and should return full range (all silence)
        let (start, end) = trim_silence(&samples, sample_rate);
        assert_eq!(start, 0);
        assert_eq!(end, samples.len());
    }
}
