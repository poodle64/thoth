//! Silero neural VAD, run through the ONNX Runtime `sherpa-onnx` already
//! carries.
//!
//! # Why this engine
//!
//! Silero is an ONNX model and needs an inference engine. Three were weighed
//! (#103):
//!
//! - **`tract`** (pure Rust) was tried first and rejected on evidence: it
//!   cannot load any published Silero export. All three fail on the model's
//!   `If` nodes (`silero_vad.onnx` on a Squeeze axis, `op18_ifless` on a Pad
//!   batch unification, `16k_op15` on mismatched If branch facts).
//! - **`ort`** is what every Silero crate on crates.io uses, but `ort-sys`
//!   downloads prebuilt ONNX Runtime binaries at build time, which the Nix
//!   sandbox cannot do, and it would put a second ONNX runtime next to the one
//!   `sherpa-onnx` already links.
//! - **`sherpa-onnx`**, which ships a complete Silero VAD API and is already a
//!   dependency of the default feature set. No new native dependency.
//!
//! # Build-configuration caveat
//!
//! `sherpa-onnx` is optional and enabled by the `parakeet` feature, which is in
//! `default`. The Linux GPU build uses `--no-default-features --features vulkan`
//! and therefore has no Silero. That is deliberate rather than an oversight:
//! [`is_available`] reports false there and callers fall back to the WebRTC
//! path, which is the graceful degradation #103 asks for. It does mean VAD
//! quality varies by build configuration, so anything user-facing should read
//! [`is_available`] rather than assume neural VAD is present.
//!
//! # Smoothing
//!
//! sherpa's detector does its own onset/hangover smoothing, configured through
//! `min_speech_duration` and `min_silence_duration`. Those are driven from
//! [`HangoverPolicy`] here so the neural path and the WebRTC path share one set
//! of policy constants instead of drifting apart.

use std::path::PathBuf;

use super::smoothed::HangoverPolicy;

/// Samples per Silero window at 16 kHz. Fixed by the model.
pub const WINDOW_SAMPLES: i32 = 512;

/// Sample rate the model is used at.
pub const SAMPLE_RATE: u32 = 16_000;

/// Probability above which a frame counts as speech. Silero's own default.
pub const DEFAULT_SPEECH_THRESHOLD: f32 = 0.5;

/// Shortest run of speech that opens a segment, in seconds. Mirrors the onset
/// debounce in [`super::smoothed`]: long enough to reject a keystroke or a
/// chair creak, short enough not to eat a word.
pub const MIN_SPEECH_SECS: f32 = 0.064;

/// Longest single segment before the detector forces a cut, in seconds.
/// Without a bound, continuous speech never emits a segment.
pub const MAX_SPEECH_SECS: f32 = 30.0;

/// Ring buffer the detector keeps, in seconds. Only meaningful when an engine
/// is linked.
#[cfg(feature = "parakeet")]
const BUFFER_SECS: f32 = 60.0;

/// File name the model is stored under.
pub const MODEL_FILE_NAME: &str = "silero_vad.onnx";

/// Model id, giving the on-disk directory under `~/.thoth/models/`.
pub const MODEL_ID: &str = "silero-vad-v5";

/// Where the Silero model is expected on disk.
pub fn model_path() -> PathBuf {
    crate::transcription::manifest::get_model_directory(MODEL_ID).join(MODEL_FILE_NAME)
}

/// Whether neural VAD can actually run: the model is present **and** this build
/// links an inference engine.
///
/// Callers use this to choose between the neural path and the WebRTC fallback,
/// so a missing model or a `--no-default-features` build degrades quality
/// rather than blocking recording.
pub fn is_available() -> bool {
    cfg!(feature = "parakeet") && model_path().is_file()
}

/// Why neural VAD is unavailable, for logging and for the settings UI.
///
/// Returns `None` when it is available.
pub fn unavailable_reason() -> Option<String> {
    if !cfg!(feature = "parakeet") {
        return Some(
            "this build has no ONNX runtime (built --no-default-features); \
             falling back to WebRTC VAD"
                .to_string(),
        );
    }
    if !model_path().is_file() {
        return Some(format!(
            "Silero model not downloaded to {}; falling back to WebRTC VAD",
            model_path().display()
        ));
    }
    None
}

/// Detector tuning derived from a [`HangoverPolicy`].
///
/// Kept as a plain struct so the mapping from policy to sherpa's knobs is
/// testable without an ONNX runtime or a model, which matters because the
/// `--no-default-features` build has neither.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SileroSettings {
    pub threshold: f32,
    pub min_silence_duration: f32,
    pub min_speech_duration: f32,
    pub max_speech_duration: f32,
    pub window_size: i32,
}

impl SileroSettings {
    /// Build settings for a hangover policy.
    pub fn for_policy(policy: HangoverPolicy) -> Self {
        Self {
            threshold: DEFAULT_SPEECH_THRESHOLD,
            // The hangover is the tail that stops a pause for breath ending an
            // utterance; sherpa calls it min_silence_duration.
            min_silence_duration: policy.hangover_ms() as f32 / 1000.0,
            min_speech_duration: MIN_SPEECH_SECS,
            max_speech_duration: MAX_SPEECH_SECS,
            window_size: WINDOW_SAMPLES,
        }
    }
}

#[cfg(feature = "parakeet")]
pub use engine::SileroVad;

#[cfg(feature = "parakeet")]
mod engine {
    use super::*;
    use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

    /// A speech segment the detector has emitted.
    #[derive(Debug, Clone)]
    pub struct Segment {
        /// Offset of the segment in samples from the start of the stream.
        pub start: usize,
        /// The segment's audio.
        pub samples: Vec<f32>,
    }

    /// Live Silero VAD over a stream of 16 kHz mono samples.
    pub struct SileroVad {
        detector: VoiceActivityDetector,
        settings: SileroSettings,
    }

    impl std::fmt::Debug for SileroVad {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("SileroVad")
                .field("settings", &self.settings)
                .finish_non_exhaustive()
        }
    }

    impl SileroVad {
        /// Load the model from the default location using a hangover policy.
        pub fn new(policy: HangoverPolicy) -> anyhow::Result<Self> {
            Self::with_settings(SileroSettings::for_policy(policy))
        }

        /// Load the model with explicit settings.
        pub fn with_settings(settings: SileroSettings) -> anyhow::Result<Self> {
            let path = model_path();
            if !path.is_file() {
                anyhow::bail!(
                    "Silero VAD model not found at {}. Download it to enable neural VAD.",
                    path.display()
                );
            }

            let config = VadModelConfig {
                silero_vad: SileroVadModelConfig {
                    model: Some(path.to_string_lossy().into_owned()),
                    threshold: settings.threshold,
                    min_silence_duration: settings.min_silence_duration,
                    min_speech_duration: settings.min_speech_duration,
                    window_size: settings.window_size,
                    max_speech_duration: settings.max_speech_duration,
                },
                sample_rate: SAMPLE_RATE as i32,
                // Both of these must be set explicitly. `Default` gives
                // num_threads = 0 and a null provider, which ONNX Runtime does
                // not accept; the failure is a `free(): invalid pointer` abort
                // inside the C library rather than a returned error.
                num_threads: 1,
                provider: Some("cpu".to_string()),
                ..Default::default()
            };

            let detector = VoiceActivityDetector::create(&config, BUFFER_SECS)
                .ok_or_else(|| anyhow::anyhow!("failed to create the Silero detector"))?;

            Ok(Self { detector, settings })
        }

        /// The settings in use.
        pub fn settings(&self) -> SileroSettings {
            self.settings
        }

        /// Feed audio. Samples must be 16 kHz mono in `[-1.0, 1.0]`.
        pub fn accept(&mut self, samples: &[f32]) {
            self.detector.accept_waveform(samples);
        }

        /// Whether speech is being detected right now.
        ///
        /// This is the live signal #88 (hands-free auto-stop) needs: it is
        /// available during capture rather than only after the fact.
        pub fn is_speech_active(&self) -> bool {
            self.detector.detected()
        }

        /// Take the next completed speech segment, if one is ready.
        pub fn next_segment(&mut self) -> Option<Segment> {
            let segment = self.detector.front()?;
            let out = Segment {
                start: segment.start().max(0) as usize,
                samples: segment.samples().to_vec(),
            };
            // `front()` hands back a view onto the queue's own segment, but the
            // wrapper's `SpeechSegment` still runs `SherpaOnnxDestroySpeechSegment`
            // on drop. Letting it drop and then calling `pop()` frees the same
            // pointer twice, which aborts with `free(): invalid pointer`. The
            // data is already copied out above, so suppress the destructor and
            // let `pop()` own the free.
            std::mem::forget(segment);
            self.detector.pop();
            Some(out)
        }

        /// Clear all state between recordings.
        ///
        /// Leftover state biases the first frames of the next utterance, and
        /// because the model still returns plausible numbers it fails quietly.
        pub fn reset(&mut self) {
            self.detector.reset();
            self.detector.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_is_the_size_silero_requires() {
        // Silero v5 is trained on 512-sample windows at 16 kHz and gives
        // meaningless output at other sizes.
        assert_eq!(WINDOW_SAMPLES, 512);
        assert_eq!(SAMPLE_RATE, 16_000);
    }

    #[test]
    fn model_path_lives_under_the_shared_model_directory() {
        let path = model_path();
        assert!(
            path.ends_with(format!("{MODEL_ID}/{MODEL_FILE_NAME}")),
            "{path:?}"
        );
    }

    #[test]
    fn hangover_policy_drives_the_detector_silence_window() {
        let offline = SileroSettings::for_policy(HangoverPolicy::Offline);
        let streaming = SileroSettings::for_policy(HangoverPolicy::Streaming);

        // The policy constants live in one place; this pins that they actually
        // reach the engine rather than the engine carrying its own copy.
        assert!((offline.min_silence_duration - 0.450).abs() < 1e-6);
        assert!((streaming.min_silence_duration - 1.650).abs() < 1e-6);
        assert!(
            streaming.min_silence_duration > offline.min_silence_duration,
            "streaming must tolerate longer pauses than offline"
        );
    }

    #[test]
    fn settings_use_silero_native_window() {
        let s = SileroSettings::for_policy(HangoverPolicy::Offline);
        assert_eq!(s.window_size, WINDOW_SAMPLES);
        assert!(
            s.min_speech_duration > 0.0 && s.min_speech_duration < 0.2,
            "onset debounce should reject transients without eating a word, got {}",
            s.min_speech_duration
        );
    }

    #[test]
    fn availability_requires_both_an_engine_and_a_model() {
        // The two failure modes must be distinguishable, because they have
        // different fixes: download a model, versus rebuild with features.
        let available = is_available();
        let reason = unavailable_reason();
        assert_eq!(
            available,
            reason.is_none(),
            "is_available and unavailable_reason must agree; reason={reason:?}"
        );

        if !cfg!(feature = "parakeet") {
            let reason = reason.expect("a build without the engine must give a reason");
            assert!(reason.contains("no ONNX runtime"), "{reason}");
        }
    }

    #[cfg(feature = "parakeet")]
    mod engine_tests {
        use super::*;

        fn load_or_skip() -> Option<SileroVad> {
            if !is_available() {
                eprintln!("skipping: {:?}", unavailable_reason());
                return None;
            }
            Some(SileroVad::new(HangoverPolicy::Offline).expect("model present but failed to load"))
        }

        /// Loud broadband noise. Not speech, but energetic, which is exactly
        /// what an energy threshold mistakes for speech.
        fn noise(seed: u32, n: usize) -> Vec<f32> {
            let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
            (0..n)
                .map(|_| {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    (x as f32 / u32::MAX as f32) * 2.0 - 1.0
                })
                .collect()
        }

        #[test]
        fn silence_produces_no_speech_and_no_segments() {
            let Some(mut vad) = load_or_skip() else {
                return;
            };
            vad.accept(&vec![0.0; SAMPLE_RATE as usize * 2]);
            assert!(!vad.is_speech_active(), "silence must not read as speech");
            assert!(
                vad.next_segment().is_none(),
                "silence must not emit a speech segment"
            );
        }

        #[test]
        fn loud_noise_is_not_mistaken_for_speech() {
            // The whole reason for moving off an energy threshold.
            let Some(mut vad) = load_or_skip() else {
                return;
            };
            vad.accept(&noise(7, SAMPLE_RATE as usize * 2));
            assert!(
                vad.next_segment().is_none(),
                "broadband noise must not emit a speech segment; an energy \
                 threshold would accept it"
            );
        }

        #[test]
        fn reset_clears_queued_state() {
            let Some(mut vad) = load_or_skip() else {
                return;
            };
            vad.accept(&noise(3, SAMPLE_RATE as usize));
            vad.reset();
            assert!(!vad.is_speech_active());
            assert!(vad.next_segment().is_none(), "reset must drop queued state");
        }
    }
}
