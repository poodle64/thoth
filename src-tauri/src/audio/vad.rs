//! Voice Activity Detection (VAD) for speech boundary detection
//!
//! This module provides VAD functionality using webrtc-vad to detect speech
//! start/end boundaries for automatic transcription triggering.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use webrtc_vad::{SampleRate, Vad, VadMode};

/// VAD operating mode determining aggressiveness of speech detection
///
/// Higher modes are more aggressive (stricter about what counts as speech),
/// which reduces false positives but may increase missed detections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadAggressiveness {
    /// Least aggressive; best for clean audio environments
    Quality = 0,
    /// Low bitrate optimised
    LowBitrate = 1,
    /// More aggressive; good for moderate background noise
    #[default]
    Aggressive = 2,
    /// Most aggressive; best for noisy environments
    VeryAggressive = 3,
}

impl From<VadAggressiveness> for VadMode {
    fn from(mode: VadAggressiveness) -> Self {
        match mode {
            VadAggressiveness::Quality => VadMode::Quality,
            VadAggressiveness::LowBitrate => VadMode::LowBitrate,
            VadAggressiveness::Aggressive => VadMode::Aggressive,
            VadAggressiveness::VeryAggressive => VadMode::VeryAggressive,
        }
    }
}

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

/// Configuration for Voice Activity Detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadConfig {
    /// VAD aggressiveness mode
    pub aggressiveness: VadAggressiveness,

    /// Frame duration for VAD processing
    pub frame_duration: VadFrameDuration,

    /// Number of consecutive speech frames required to trigger speech start
    ///
    /// Higher values reduce false positives from transient noises.
    /// Default: 3 (90ms at 30ms frames)
    pub speech_start_frames: u32,

    /// Number of consecutive silence frames required to trigger speech end
    ///
    /// Higher values prevent premature cutoff during natural pauses.
    /// Default: 15 (450ms at 30ms frames)
    pub speech_end_frames: u32,

    /// Padding duration in milliseconds to add before detected speech start
    ///
    /// Captures audio slightly before speech detection triggered.
    /// Default: 300ms
    pub pre_speech_padding_ms: u32,

    /// Padding duration in milliseconds to add after detected speech end
    ///
    /// Captures trailing audio after speech detection ends.
    /// Default: 300ms
    pub post_speech_padding_ms: u32,

    /// Auto-stop recording after this many milliseconds of silence
    ///
    /// When set to Some(ms), recording will automatically stop after
    /// the specified duration of silence following detected speech.
    /// Set to None to disable auto-stop.
    /// Default: Some(2000) (2 seconds)
    pub auto_stop_silence_ms: Option<u32>,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            aggressiveness: VadAggressiveness::default(),
            frame_duration: VadFrameDuration::default(),
            speech_start_frames: 3,
            speech_end_frames: 15,
            pre_speech_padding_ms: 300,
            post_speech_padding_ms: 300,
            auto_stop_silence_ms: Some(2000),
        }
    }
}

/// Current state of the VAD speech detector
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VadSpeechState {
    /// No speech detected; waiting for speech to begin
    #[default]
    Silence,
    /// Potential speech detected; accumulating consecutive speech frames
    PossibleSpeech,
    /// Speech confirmed and ongoing
    Speaking,
    /// Speech may have ended; accumulating consecutive silence frames
    PossibleSilence,
}

/// Event emitted when VAD detects a speech boundary
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum VadEvent {
    /// Speech has started
    SpeechStart {
        /// Timestamp in milliseconds from start of monitoring
        timestamp_ms: u64,
    },
    /// Speech has ended
    SpeechEnd {
        /// Timestamp in milliseconds from start of monitoring
        timestamp_ms: u64,
        /// Duration of the speech segment in milliseconds
        duration_ms: u64,
    },
    /// Auto-stop triggered after sustained silence following speech
    AutoStopTriggered {
        /// Timestamp in milliseconds from start of monitoring
        timestamp_ms: u64,
        /// Duration of silence that triggered the auto-stop
        silence_duration_ms: u64,
    },
}

/// Voice Activity Detector wrapper
///
/// This struct wraps the webrtc-vad library and provides speech boundary
/// detection with configurable start/end thresholds.
///
/// Note: The underlying `Vad` type is `!Send` and `!Sync`, so this wrapper
/// must be used from a single thread (typically a dedicated VAD thread).
pub struct VoiceActivityDetector {
    vad: Vad,
    config: VadConfig,
    state: VadSpeechState,
    consecutive_speech_frames: u32,
    consecutive_silence_frames: u32,
    frame_count: u64,
    speech_start_frame: Option<u64>,
    enabled: Arc<AtomicBool>,
    /// Whether speech has been detected at least once (for auto-stop)
    has_detected_speech: bool,
    /// Frame count when speech ended (for auto-stop timing)
    speech_end_frame: Option<u64>,
}

impl VoiceActivityDetector {
    /// Creates a new Voice Activity Detector with the given configuration
    pub fn new(config: VadConfig) -> Self {
        let vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, config.aggressiveness.into());

        Self {
            vad,
            config,
            state: VadSpeechState::Silence,
            consecutive_speech_frames: 0,
            consecutive_silence_frames: 0,
            frame_count: 0,
            speech_start_frame: None,
            enabled: Arc::new(AtomicBool::new(true)),
            has_detected_speech: false,
            speech_end_frame: None,
        }
    }

    /// Creates a new Voice Activity Detector with default configuration
    pub fn with_defaults() -> Self {
        Self::new(VadConfig::default())
    }

    /// Returns a handle to the enabled flag for cross-thread control
    pub fn enabled_handle(&self) -> Arc<AtomicBool> {
        self.enabled.clone()
    }

    /// Checks if VAD is currently enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enables or disables VAD processing
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Returns the current speech state
    pub fn state(&self) -> VadSpeechState {
        self.state
    }

    /// Returns the current configuration
    pub fn config(&self) -> &VadConfig {
        &self.config
    }

    /// Returns the expected frame size in samples
    pub fn frame_size(&self) -> usize {
        self.config.frame_duration.samples_at_16khz()
    }

    /// Resets the VAD state machine
    pub fn reset(&mut self) {
        self.vad.reset();
        self.state = VadSpeechState::Silence;
        self.consecutive_speech_frames = 0;
        self.consecutive_silence_frames = 0;
        self.frame_count = 0;
        self.speech_start_frame = None;
        self.has_detected_speech = false;
        self.speech_end_frame = None;
    }

    /// Returns whether speech has been detected at least once since last reset
    pub fn has_detected_speech(&self) -> bool {
        self.has_detected_speech
    }

    /// Check if auto-stop should be triggered
    ///
    /// Returns `Some(VadEvent::AutoStopTriggered)` if auto-stop conditions are met:
    /// - Auto-stop is enabled (auto_stop_silence_ms is Some)
    /// - Speech has been detected at least once
    /// - Currently in silence state
    /// - Sufficient silence duration has elapsed since speech ended
    pub fn check_auto_stop(&self) -> Option<VadEvent> {
        let auto_stop_ms = self.config.auto_stop_silence_ms?;

        if !self.has_detected_speech {
            return None;
        }

        if self.state != VadSpeechState::Silence {
            return None;
        }

        let speech_end = self.speech_end_frame?;
        let silence_frames = self.frame_count.saturating_sub(speech_end);
        let silence_ms = silence_frames * self.config.frame_duration as u64;

        if silence_ms >= auto_stop_ms as u64 {
            Some(VadEvent::AutoStopTriggered {
                timestamp_ms: self.frame_to_ms(self.frame_count),
                silence_duration_ms: silence_ms,
            })
        } else {
            None
        }
    }

    /// Processes a frame of 16kHz mono i16 audio samples
    ///
    /// The frame must contain exactly the number of samples specified by
    /// the configured frame duration (160, 320, or 480 samples).
    ///
    /// Returns `Some(VadEvent)` if a speech boundary was detected, or `None`
    /// if no state change occurred.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame has an invalid length.
    pub fn process_frame(&mut self, samples: &[i16]) -> Result<Option<VadEvent>, VadError> {
        let expected_size = self.frame_size();
        if samples.len() != expected_size {
            return Err(VadError::InvalidFrameLength {
                expected: expected_size,
                actual: samples.len(),
            });
        }

        if !self.is_enabled() {
            return Ok(None);
        }

        let is_speech = self
            .vad
            .is_voice_segment(samples)
            .map_err(|()| VadError::ProcessingFailed)?;

        self.frame_count += 1;
        let event = self.update_state_machine(is_speech);

        Ok(event)
    }

    /// Processes a frame of 16kHz mono f32 audio samples
    ///
    /// Converts f32 samples to i16 before processing. The frame must contain
    /// exactly the number of samples specified by the configured frame duration.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame has an invalid length or processing fails.
    pub fn process_frame_f32(&mut self, samples: &[f32]) -> Result<Option<VadEvent>, VadError> {
        let expected_size = self.frame_size();
        if samples.len() != expected_size {
            return Err(VadError::InvalidFrameLength {
                expected: expected_size,
                actual: samples.len(),
            });
        }

        // Convert f32 to i16
        let i16_samples: Vec<i16> = samples
            .iter()
            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();

        self.process_frame(&i16_samples)
    }

    /// Updates the state machine based on whether the current frame is speech
    fn update_state_machine(&mut self, is_speech: bool) -> Option<VadEvent> {
        match self.state {
            VadSpeechState::Silence => {
                if is_speech {
                    self.consecutive_speech_frames = 1;
                    self.state = VadSpeechState::PossibleSpeech;
                }
                None
            }
            VadSpeechState::PossibleSpeech => {
                if is_speech {
                    self.consecutive_speech_frames += 1;
                    if self.consecutive_speech_frames >= self.config.speech_start_frames {
                        self.state = VadSpeechState::Speaking;
                        self.consecutive_silence_frames = 0;
                        self.has_detected_speech = true;
                        self.speech_end_frame = None; // Clear any previous end frame
                                                      // Calculate speech start frame accounting for pre-speech padding
                        let padding_frames =
                            self.config.pre_speech_padding_ms / self.config.frame_duration as u32;
                        self.speech_start_frame = Some(
                            self.frame_count
                                .saturating_sub(self.consecutive_speech_frames as u64)
                                .saturating_sub(padding_frames as u64),
                        );
                        return Some(VadEvent::SpeechStart {
                            timestamp_ms: self
                                .frame_to_ms(self.speech_start_frame.unwrap_or(self.frame_count)),
                        });
                    }
                } else {
                    // Reset on silence
                    self.consecutive_speech_frames = 0;
                    self.state = VadSpeechState::Silence;
                }
                None
            }
            VadSpeechState::Speaking => {
                if is_speech {
                    self.consecutive_silence_frames = 0;
                } else {
                    self.consecutive_silence_frames = 1;
                    self.state = VadSpeechState::PossibleSilence;
                }
                None
            }
            VadSpeechState::PossibleSilence => {
                if is_speech {
                    // Resume speaking
                    self.consecutive_silence_frames = 0;
                    self.state = VadSpeechState::Speaking;
                    None
                } else {
                    self.consecutive_silence_frames += 1;
                    if self.consecutive_silence_frames >= self.config.speech_end_frames {
                        self.state = VadSpeechState::Silence;
                        self.speech_end_frame = Some(self.frame_count); // Track when speech ended
                        let speech_start = self.speech_start_frame.take();
                        // Calculate end frame with post-speech padding
                        let padding_frames =
                            self.config.post_speech_padding_ms / self.config.frame_duration as u32;
                        let end_frame = self.frame_count + padding_frames as u64;

                        if let Some(start_frame) = speech_start {
                            let start_ms = self.frame_to_ms(start_frame);
                            let end_ms = self.frame_to_ms(end_frame);
                            return Some(VadEvent::SpeechEnd {
                                timestamp_ms: end_ms,
                                duration_ms: end_ms.saturating_sub(start_ms),
                            });
                        }

                        self.consecutive_speech_frames = 0;
                    }
                    None
                }
            }
        }
    }

    /// Converts a frame number to milliseconds
    fn frame_to_ms(&self, frame: u64) -> u64 {
        frame * self.config.frame_duration as u64
    }
}

/// Minimum recording duration (in samples) before silence trimming is applied.
/// For shorter recordings the VAD overhead isn't worth it.
/// 20 seconds × 16 000 Hz = 320 000 samples.
const TRIM_MIN_SAMPLES_16KHZ: usize = 320_000;

/// Safety margin (in seconds) kept around detected speech boundaries so we
/// don't clip into the start or end of an utterance.
const TRIM_MARGIN_SECS: f32 = 0.2;

/// Trim leading and trailing silence from audio samples using VAD.
///
/// For recordings shorter than ~20 seconds (at the given sample rate) the
/// original slice is returned unchanged because the VAD overhead is not worth
/// the saving.
///
/// The function runs a single-pass VAD scan over the audio, locates the first
/// and last speech frames, and returns a sub-slice that keeps a small margin
/// (200 ms) around the detected speech boundaries.
///
/// # Arguments
/// * `samples` — mono f32 audio samples normalised to [-1.0, 1.0]
/// * `sample_rate` — sample rate in Hz (typically 16 000)
///
/// # Returns
/// A `(start, end)` range into `samples`. Callers should use `samples[start..end]`.
/// If no speech is detected the full range `(0, samples.len())` is returned so
/// the downstream silence check can decide what to do.
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
    let mut last_speech_frame: Option<usize> = None;

    let mut frame_idx: usize = 0;
    let mut pos: usize = 0;

    while pos + actual_frame_size <= total {
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

        if let Ok(is_speech) = vad.is_voice_segment(&frame_i16) {
            if is_speech {
                if first_speech_frame.is_none() {
                    first_speech_frame = Some(frame_idx);
                }
                last_speech_frame = Some(frame_idx);
            }
        }

        frame_idx += 1;
        pos += actual_frame_size;
    }

    // No speech found — return the full range so downstream silence detection
    // can handle it (returning an empty range would silently discard audio).
    let (first, last) = match (first_speech_frame, last_speech_frame) {
        (Some(f), Some(l)) => (f, l),
        _ => return (0, total),
    };

    let margin_samples = (TRIM_MARGIN_SECS * sample_rate as f32) as usize;

    let start = (first * actual_frame_size).saturating_sub(margin_samples);
    // End is one frame *past* the last speech frame, plus margin
    let end = ((last + 1) * actual_frame_size + margin_samples).min(total);

    let trimmed_duration = (end - start) as f32 / sample_rate as f32;
    let original_duration = total as f32 / sample_rate as f32;
    tracing::info!(
        "VAD silence trim: {:.1}s → {:.1}s (removed {:.1}s leading + {:.1}s trailing)",
        original_duration,
        trimmed_duration,
        start as f32 / sample_rate as f32,
        (total - end) as f32 / sample_rate as f32,
    );

    (start, end)
}

/// Errors that can occur during VAD processing
#[derive(Debug, Clone, thiserror::Error)]
pub enum VadError {
    /// Frame has invalid length
    #[error("Invalid frame length: expected {expected} samples, got {actual}")]
    InvalidFrameLength { expected: usize, actual: usize },

    /// VAD processing failed (internal webrtc-vad error)
    #[error("VAD processing failed")]
    ProcessingFailed,
}

/// Status information for VAD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VadStatus {
    /// Whether VAD is enabled
    pub enabled: bool,
    /// Current speech state
    pub state: VadSpeechState,
    /// Current configuration
    pub config: VadConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vad_config_default() {
        let config = VadConfig::default();
        assert_eq!(config.aggressiveness, VadAggressiveness::Aggressive);
        assert_eq!(config.frame_duration, VadFrameDuration::Ms30);
        assert_eq!(config.speech_start_frames, 3);
        assert_eq!(config.speech_end_frames, 15);
        assert_eq!(config.auto_stop_silence_ms, Some(2000));
    }

    #[test]
    fn test_frame_duration_samples() {
        assert_eq!(VadFrameDuration::Ms10.samples_at_16khz(), 160);
        assert_eq!(VadFrameDuration::Ms20.samples_at_16khz(), 320);
        assert_eq!(VadFrameDuration::Ms30.samples_at_16khz(), 480);
    }

    #[test]
    fn test_vad_new() {
        let vad = VoiceActivityDetector::with_defaults();
        assert_eq!(vad.state(), VadSpeechState::Silence);
        assert_eq!(vad.frame_size(), 480);
        assert!(vad.is_enabled());
    }

    #[test]
    fn test_vad_invalid_frame_length() {
        let mut vad = VoiceActivityDetector::with_defaults();
        let samples = vec![0i16; 100]; // Wrong size
        let result = vad.process_frame(&samples);
        assert!(matches!(result, Err(VadError::InvalidFrameLength { .. })));
    }

    #[test]
    fn test_vad_silence_detection() {
        let mut vad = VoiceActivityDetector::with_defaults();
        let silence = vec![0i16; 480];

        // Processing silence should not trigger any events
        for _ in 0..10 {
            let event = vad.process_frame(&silence).unwrap();
            assert!(event.is_none());
        }
        assert_eq!(vad.state(), VadSpeechState::Silence);
    }

    #[test]
    fn test_vad_enable_disable() {
        let vad = VoiceActivityDetector::with_defaults();
        assert!(vad.is_enabled());

        vad.set_enabled(false);
        assert!(!vad.is_enabled());

        vad.set_enabled(true);
        assert!(vad.is_enabled());
    }

    #[test]
    fn test_vad_reset() {
        let mut vad = VoiceActivityDetector::with_defaults();

        // Simulate some state changes
        let silence = vec![0i16; 480];
        for _ in 0..5 {
            vad.process_frame(&silence).ok();
        }

        vad.reset();
        assert_eq!(vad.state(), VadSpeechState::Silence);
    }

    #[test]
    fn test_vad_aggressiveness_conversion() {
        // Test that conversion works by creating a VAD with each mode
        // webrtc_vad::VadMode doesn't implement PartialEq/Debug, so we verify
        // the conversion works by successfully creating a Vad with each mode
        let mut vad = Vad::new();

        // Each set_mode call verifies the VadMode was correctly converted
        vad.set_mode(VadMode::from(VadAggressiveness::Quality));
        vad.set_mode(VadMode::from(VadAggressiveness::LowBitrate));
        vad.set_mode(VadMode::from(VadAggressiveness::Aggressive));
        vad.set_mode(VadMode::from(VadAggressiveness::VeryAggressive));

        // Also verify the enum values match expected indices
        assert_eq!(VadAggressiveness::Quality as u8, 0);
        assert_eq!(VadAggressiveness::LowBitrate as u8, 1);
        assert_eq!(VadAggressiveness::Aggressive as u8, 2);
        assert_eq!(VadAggressiveness::VeryAggressive as u8, 3);
    }

    #[test]
    fn test_f32_to_i16_conversion() {
        let mut vad = VoiceActivityDetector::with_defaults();
        let silence_f32 = vec![0.0f32; 480];

        let event = vad.process_frame_f32(&silence_f32).unwrap();
        assert!(event.is_none());
    }

    #[test]
    fn test_vad_status() {
        let vad = VoiceActivityDetector::with_defaults();
        let status = VadStatus {
            enabled: vad.is_enabled(),
            state: vad.state(),
            config: vad.config().clone(),
        };

        assert!(status.enabled);
        assert_eq!(status.state, VadSpeechState::Silence);
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
    fn test_trim_silence_trims_leading_and_trailing() {
        // 30 seconds at 16 kHz: silence, then speech-like noise, then silence
        let sample_rate = 16_000u32;
        let total_samples = 30 * sample_rate as usize; // 480_000
        let mut samples = vec![0.0f32; total_samples];

        // Put loud noise (simulating speech) from 10s to 20s
        let speech_start = 10 * sample_rate as usize; // 160_000
        let speech_end = 20 * sample_rate as usize; // 320_000
        for i in speech_start..speech_end {
            // Generate a tone that VAD will detect as speech
            let t = i as f32 / sample_rate as f32;
            samples[i] = (t * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.5;
        }

        let (start, end) = trim_silence(&samples, sample_rate);

        // The trimmed region should be smaller than the original
        assert!(start > 0, "Should trim leading silence");
        assert!(end < total_samples, "Should trim trailing silence");

        // The speech region (10s-20s) should be fully contained
        let margin = (TRIM_MARGIN_SECS * sample_rate as f32) as usize;
        assert!(
            start <= speech_start + margin,
            "Trim start ({start}) should be at or before speech start + margin ({})",
            speech_start + margin
        );
        assert!(
            end >= speech_end.saturating_sub(margin),
            "Trim end ({end}) should be at or after speech end - margin ({})",
            speech_end.saturating_sub(margin)
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
