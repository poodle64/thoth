//! Silero v5 neural VAD, run on `tract` (a pure-Rust ONNX engine).
//!
//! # Why tract and not ONNX Runtime
//!
//! Every Silero wrapper on crates.io (`vad-rs`, `silero-vad-rs`,
//! `vad-silero-rs`) depends on `ort`, whose `ort-sys` build script downloads
//! prebuilt ONNX Runtime binaries. Thoth builds in a Nix sandbox with no
//! network, so that cannot work here, and it would also land a second ONNX
//! runtime in Parakeet builds next to the one `sherpa-onnx` already carries.
//! `tract` is pure Rust: nothing to link, nothing to download, and it stays
//! out of the way of the optional `parakeet` feature (#103).
//!
//! # Model contract
//!
//! Silero v5 (`silero_vad.onnx`, ~2.2 MB) takes three inputs and returns two:
//!
//! | Name    | Type | Shape             | Meaning                       |
//! |---------|------|-------------------|-------------------------------|
//! | `input` | f32  | `[1, 512]`        | one frame of mono audio       |
//! | `state` | f32  | `[2, 1, 128]`     | recurrent state               |
//! | `sr`    | i64  | scalar            | sample rate                   |
//! | `output`| f32  | `[1, 1]`          | probability the frame is speech |
//! | `stateN`| f32  | `[2, 1, 128]`     | state to feed the next frame  |
//!
//! The 512-sample window is not a free choice: v5 is trained on it and gives
//! meaningless output at other sizes. At 16 kHz that is 32 ms per frame, which
//! is why the smoothing layer derives its frame counts from a duration rather
//! than assuming the 30 ms WebRTC frame.
//!
//! The model is **stateful across frames**: `stateN` must be fed back in as
//! `state`. Feeding a zeroed state every frame silently degrades accuracy
//! rather than failing, so [`SileroVad::reset`] exists to clear it explicitly
//! between recordings.

use std::path::{Path, PathBuf};

use tract_onnx::prelude::*;

/// Samples per frame at 16 kHz. Fixed by the model; see the module docs.
pub const FRAME_SAMPLES: usize = 512;

/// Sample rate the model is used at.
pub const SAMPLE_RATE: i64 = 16_000;

/// Frame duration in milliseconds, derived from the two constants above so it
/// cannot disagree with them.
pub const FRAME_MS: usize = (FRAME_SAMPLES * 1000) / SAMPLE_RATE as usize;

/// Size of the recurrent state tensor: `[2, 1, 128]`.
const STATE_DIMS: [usize; 3] = [2, 1, 128];

/// Probability above which a frame counts as speech.
///
/// Silero's own examples use 0.5. The smoothing layer is what provides
/// robustness here, so this stays at the model's default rather than being
/// tuned to compensate for missing debounce/hangover.
pub const DEFAULT_SPEECH_THRESHOLD: f32 = 0.5;

/// File name the model is stored under.
pub const MODEL_FILE_NAME: &str = "silero_vad.onnx";

/// Model id used for the on-disk directory, matching the transcription models'
/// layout under `~/.thoth/models/`.
pub const MODEL_ID: &str = "silero-vad-v5";

/// `into_runnable` hands back an `Arc`, so the alias has to carry it.
type Model = std::sync::Arc<TypedRunnableModel>;

/// Where the Silero model is expected on disk.
pub fn model_path() -> PathBuf {
    crate::transcription::manifest::get_model_directory(MODEL_ID).join(MODEL_FILE_NAME)
}

/// Whether the Silero model has been downloaded.
///
/// Callers use this to decide whether to run neural VAD or fall back to the
/// WebRTC path, so a missing model degrades quality rather than blocking
/// recording.
pub fn is_available() -> bool {
    model_path().is_file()
}

/// A loaded Silero VAD.
pub struct SileroVad {
    model: Model,
    /// Recurrent state carried between frames.
    state: Tensor,
    threshold: f32,
    /// Input slots, resolved by name at load time; see [`SileroVad::load_from`].
    input_idx: usize,
    sr_idx: usize,
    state_idx: usize,
}

/// The optimised graph is not `Debug`, so report only what is useful to a
/// reader. This exists so `Result<SileroVad, _>` can be unwrapped in tests.
impl std::fmt::Debug for SileroVad {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SileroVad")
            .field("threshold", &self.threshold)
            .field("frame_samples", &FRAME_SAMPLES)
            .finish_non_exhaustive()
    }
}

impl SileroVad {
    /// Load the model from the default location.
    pub fn load() -> anyhow::Result<Self> {
        Self::load_from(&model_path())
    }

    /// Load the model from an explicit path.
    pub fn load_from(path: &Path) -> anyhow::Result<Self> {
        if !path.is_file() {
            anyhow::bail!(
                "Silero VAD model not found at {}. Download it to enable neural VAD.",
                path.display()
            );
        }

        let mut model = tract_onnx::onnx().model_for_path(path)?;

        // Resolve inputs by name rather than by position. The exports do not
        // agree on order (the combined model is input/state/sr, the ifless one
        // is input/sr/state), and getting it wrong does not fail cleanly: it
        // surfaces as an unrelated shape-inference error deep in the graph.
        let names: Vec<String> = model
            .input_outlets()?
            .iter()
            .map(|o| model.node(o.node).name.clone())
            .collect();
        let index_of = |want: &str| -> anyhow::Result<usize> {
            names.iter().position(|n| n == want).ok_or_else(|| {
                anyhow::anyhow!("Silero model has no `{want}` input; found {names:?}")
            })
        };
        let input_idx = index_of("input")?;
        let sr_idx = index_of("sr")?;
        let state_idx = index_of("state")?;

        // Shapes are pinned rather than left symbolic: tract optimises far
        // better against concrete shapes, and only these are ever used.
        model = model.with_input_fact(input_idx, f32::fact([1, FRAME_SAMPLES]).into())?;
        model = model.with_input_fact(sr_idx, i64::fact::<[usize; 0]>([]).into())?;
        model = model.with_input_fact(state_idx, f32::fact(STATE_DIMS).into())?;

        let model = model.into_optimized()?.into_runnable()?;

        Ok(Self {
            model,
            state: Self::zero_state(),
            threshold: DEFAULT_SPEECH_THRESHOLD,
            input_idx,
            sr_idx,
            state_idx,
        })
    }

    fn zero_state() -> Tensor {
        Tensor::zero::<f32>(&STATE_DIMS).expect("zero tensor of a constant shape cannot fail")
    }

    /// Override the speech probability threshold.
    pub fn with_threshold(mut self, threshold: f32) -> Self {
        self.threshold = threshold;
        self
    }

    /// The threshold currently in use.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// Clear the recurrent state.
    ///
    /// Must be called between recordings: leftover state from a previous
    /// utterance biases the first frames of the next one, and because the
    /// model still returns plausible numbers it fails quietly rather than
    /// loudly.
    pub fn reset(&mut self) {
        self.state = Self::zero_state();
    }

    /// Probability that one frame contains speech.
    ///
    /// `frame` must be exactly [`FRAME_SAMPLES`] mono samples in `[-1.0, 1.0]`
    /// at 16 kHz. Advances the recurrent state.
    pub fn speech_probability(&mut self, frame: &[f32]) -> anyhow::Result<f32> {
        if frame.len() != FRAME_SAMPLES {
            anyhow::bail!(
                "Silero v5 requires exactly {} samples per frame, got {}",
                FRAME_SAMPLES,
                frame.len()
            );
        }

        // Placed by resolved index, not in literal order, for the reason given
        // in `load_from`.
        let mut inputs: TVec<TValue> = tvec!(Tensor::from(0i64).into(); 3);
        inputs[self.input_idx] = Tensor::from_shape(&[1, FRAME_SAMPLES], frame)?.into();
        inputs[self.sr_idx] = Tensor::from(SAMPLE_RATE).into();
        inputs[self.state_idx] =
            std::mem::replace(&mut self.state, Self::zero_state()).into();

        let outputs = self.model.run(inputs)?;

        // Feed the new state forward before reading the probability, so an
        // error reading the output cannot leave a stale state behind.
        self.state = outputs[1].clone().into_tensor();

        let probability = outputs[0].to_plain_array_view::<f32>()?.iter().copied().next();

        probability.ok_or_else(|| anyhow::anyhow!("Silero returned an empty probability tensor"))
    }

    /// Whether one frame contains speech, per the current threshold.
    pub fn is_speech(&mut self, frame: &[f32]) -> anyhow::Result<bool> {
        Ok(self.speech_probability(frame)? >= self.threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frame of silence.
    fn silence() -> Vec<f32> {
        vec![0.0; FRAME_SAMPLES]
    }

    /// Frame of loud broadband noise. Not speech, but energetic, which is
    /// exactly what an energy-threshold VAD mistakes for speech.
    fn noise(seed: u32) -> Vec<f32> {
        let mut x = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
        (0..FRAME_SAMPLES)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                (x as f32 / u32::MAX as f32) * 2.0 - 1.0
            })
            .collect()
    }

    #[test]
    fn frame_duration_is_derived_from_the_model_constants() {
        // 512 samples at 16 kHz is 32 ms. If either constant changes, this is
        // the test that catches the smoothing layer being mistuned.
        assert_eq!(FRAME_MS, 32);
    }

    #[test]
    fn model_path_lives_under_the_shared_model_directory() {
        let path = model_path();
        assert!(path.ends_with(format!("{MODEL_ID}/{MODEL_FILE_NAME}")), "{path:?}");
    }

    #[test]
    fn loading_a_missing_model_names_the_path() {
        let err = SileroVad::load_from(Path::new("/nonexistent/silero_vad.onnx"))
            .expect_err("a missing model must not load");
        let msg = err.to_string();
        assert!(msg.contains("/nonexistent/silero_vad.onnx"), "{msg}");
    }

    // The tests below need the real model. They skip rather than fail when it
    // is absent, so a fresh checkout without the download still runs green;
    // CI with the model present exercises them.
    fn load_or_skip() -> Option<SileroVad> {
        if !is_available() {
            eprintln!(
                "skipping: Silero model not present at {}",
                model_path().display()
            );
            return None;
        }
        Some(SileroVad::load().expect("model present but failed to load"))
    }

    #[test]
    fn rejects_a_frame_of_the_wrong_length() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        let err = vad
            .speech_probability(&vec![0.0; FRAME_SAMPLES - 1])
            .expect_err("a short frame must be rejected, not silently padded");
        assert!(err.to_string().contains("512"), "{err}");
    }

    #[test]
    fn silence_scores_below_the_threshold() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        for _ in 0..10 {
            let p = vad.speech_probability(&silence()).expect("inference failed");
            assert!(
                p < DEFAULT_SPEECH_THRESHOLD,
                "silence scored {p}, at or above the speech threshold"
            );
        }
    }

    #[test]
    fn loud_noise_is_not_mistaken_for_speech() {
        // The whole reason for moving off an energy threshold: loud non-speech
        // must not read as speech.
        let Some(mut vad) = load_or_skip() else {
            return;
        };
        let mut speech_frames = 0;
        for seed in 0..20 {
            if vad.is_speech(&noise(seed)).expect("inference failed") {
                speech_frames += 1;
            }
        }
        assert!(
            speech_frames <= 4,
            "{speech_frames}/20 noise frames read as speech; neural VAD should \
             reject broadband noise an energy threshold would accept"
        );
    }

    #[test]
    fn reset_clears_recurrent_state() {
        let Some(mut vad) = load_or_skip() else {
            return;
        };

        // Drive the state with noise, then reset and confirm silence scores
        // the same as it does on a freshly loaded model.
        let mut fresh = SileroVad::load().expect("model present but failed to load");
        let baseline = fresh.speech_probability(&silence()).expect("inference failed");

        for seed in 0..10 {
            let _ = vad.speech_probability(&noise(seed));
        }
        vad.reset();
        let after_reset = vad.speech_probability(&silence()).expect("inference failed");

        assert!(
            (after_reset - baseline).abs() < 1e-6,
            "reset must restore the initial state: baseline {baseline}, after reset {after_reset}"
        );
    }
}
