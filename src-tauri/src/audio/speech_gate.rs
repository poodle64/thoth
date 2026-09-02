//! Live, frame-accurate speech gating.
//!
//! [`vad::trim_silence`](super::vad) answers one question once, after the
//! recording is over: where did the speech start? This module answers a
//! different one continuously, while the user is still talking: *is there
//! speech right now?* That is what hands-free auto-stop (#88) needs, and what
//! any future streaming or partial-transcript work will need.
//!
//! A raw per-frame decision is too twitchy to act on: one dropped frame
//! mid-word would read as the end of the utterance. [`SpeechGate`] smooths it
//! the way Handy's `SmoothedVad` does — speech must persist for
//! [`GateConfig::onset_frames`] before the gate opens, and silence for
//! [`GateConfig::hangover_frames`] before it closes — and the two are
//! deliberately asymmetric, because opening late costs nothing (the audio is
//! kept in full either way) while closing early clips a word.
//!
//! The per-frame engine sits behind [`SpeechDetector`] so it can be swapped
//! without touching the smoothing, the wiring, or the auto-stop policy. The one
//! that ships is WebRTC's, whose bias is the safe direction here: it
//! over-reports speech (a held tone or a fan reads as voiced), so its failure
//! mode is a gate that stays open — auto-stop that does not fire, never
//! auto-stop that cuts someone off mid-word. #103 tracks the neural engine and
//! why the pure-Rust Silero crate measured as unusable.

use super::vad::VadFrameDuration;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use webrtc_vad::{SampleRate, Vad, VadMode};

/// Sample rate the gate runs at. The capture writer resamples to this before
/// the WAV is written, so the gate sees the same audio the transcriber will.
pub const GATE_SAMPLE_RATE: u32 = 16_000;

/// One frame's speech decision, as a probability in `[0, 1]`.
///
/// A probability rather than a bool so a neural engine, which produces one
/// natively, drops in without changing the smoothing or the threshold.
pub trait SpeechDetector {
    /// Samples per frame at [`GATE_SAMPLE_RATE`].
    fn frame_samples(&self) -> usize;
    /// Classify exactly `frame_samples()` samples in `[-1.0, 1.0]`.
    fn probability(&mut self, frame: &[f32]) -> f32;
    /// Forget any per-stream state before a new recording.
    fn reset(&mut self);
}

/// WebRTC's voice-activity detector.
///
/// `webrtc_vad::Vad` wraps a C handle and is not [`Send`], so a gate built on
/// it stays on the thread that made it. The capture writer thread — the only
/// place with the resampled audio — is that thread, and it publishes what
/// other threads need through [`SpeechActivity`].
pub struct WebRtcDetector {
    vad: Vad,
    frame: VadFrameDuration,
}

/// `Aggressive` matches [`trim_silence`](super::vad), so the live gate and the
/// post-hoc trim agree about what counts as speech.
const DETECTOR_MODE: VadMode = VadMode::Aggressive;

impl WebRtcDetector {
    /// Build a detector over frames of the given duration.
    pub fn new(frame: VadFrameDuration) -> Self {
        Self {
            vad: Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, DETECTOR_MODE),
            frame,
        }
    }
}

impl SpeechDetector for WebRtcDetector {
    fn frame_samples(&self) -> usize {
        self.frame.samples_at_16khz()
    }

    fn probability(&mut self, frame: &[f32]) -> f32 {
        let pcm: Vec<i16> = frame
            .iter()
            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();
        match self.vad.is_voice_segment(&pcm) {
            Ok(true) => 1.0,
            // A rejected frame is a caller bug (wrong length), not silence —
            // but the gate must not open on one either.
            Ok(false) | Err(_) => 0.0,
        }
    }

    fn reset(&mut self) {
        // The detector carries adaptive noise estimates across frames and the
        // wrapper exposes no reset, so a new stream gets a new detector.
        self.vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, DETECTOR_MODE);
    }
}

/// How the raw per-frame decision is smoothed into a speech/silence state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GateConfig {
    /// Probability at or above which a frame counts as speech.
    pub threshold: f32,
    /// Consecutive speech frames needed to open the gate. Debounces a single
    /// noisy frame into a false start.
    pub onset_frames: u32,
    /// Consecutive silence frames needed to close it. This is the tail that
    /// stops a pause for breath reading as the end of the utterance.
    pub hangover_frames: u32,
}

impl Default for GateConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            // 2 × 30 ms on, 15 × 30 ms off — Handy's numbers, and the same
            // 450 ms of grace the offline trim already allows for.
            onset_frames: 2,
            hangover_frames: 15,
        }
    }
}

/// A smoothed live speech/silence decision over a stream of 16 kHz mono audio.
///
/// Feed it with [`push`](Self::push) in whatever sized blocks arrive; it
/// buffers the remainder between calls, so callers do not have to align to the
/// detector's frame size.
pub struct SpeechGate {
    detector: Box<dyn SpeechDetector>,
    config: GateConfig,
    frame_samples: usize,
    /// Samples carried over from the previous `push` that did not fill a frame.
    pending: Vec<f32>,
    /// Total samples fed in, including the ones still in `pending`.
    samples_seen: u64,
    consecutive_speech: u32,
    consecutive_silence: u32,
    speaking: bool,
    /// Sample offset where the first accepted speech run began.
    first_speech_sample: Option<u64>,
    /// Sample offset just past the last frame classified as speech.
    last_speech_end: Option<u64>,
}

impl SpeechGate {
    /// A gate over the shipped WebRTC detector at 30 ms frames.
    pub fn new(config: GateConfig) -> Self {
        Self::with_detector(
            Box::new(WebRtcDetector::new(VadFrameDuration::Ms30)),
            config,
        )
    }

    /// A gate over any detector.
    pub fn with_detector(detector: Box<dyn SpeechDetector>, config: GateConfig) -> Self {
        let frame_samples = detector.frame_samples();
        Self {
            detector,
            config,
            frame_samples,
            pending: Vec::with_capacity(frame_samples),
            samples_seen: 0,
            consecutive_speech: 0,
            consecutive_silence: 0,
            speaking: false,
            first_speech_sample: None,
            last_speech_end: None,
        }
    }

    /// Feed 16 kHz mono samples in `[-1.0, 1.0]`.
    ///
    /// Returns `true` if the smoothed state changed during this call.
    pub fn push(&mut self, samples: &[f32]) -> bool {
        let was_speaking = self.speaking;
        self.samples_seen += samples.len() as u64;

        let mut rest = samples;
        while !rest.is_empty() {
            let want = self.frame_samples - self.pending.len();
            let take = want.min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];

            if self.pending.len() == self.frame_samples {
                // The frame just completed ends this many samples into the
                // stream — everything fed so far, less what is still unread.
                let frame_end = self.samples_seen - rest.len() as u64;
                let probability = self.detector.probability(&self.pending);
                self.pending.clear();
                self.classify(probability, frame_end);
            }
        }

        self.speaking != was_speaking
    }

    /// Apply one frame's probability. `frame_end` is the sample offset just
    /// past that frame.
    fn classify(&mut self, probability: f32, frame_end: u64) {
        if probability >= self.config.threshold {
            self.consecutive_speech += 1;
            self.consecutive_silence = 0;
            self.last_speech_end = Some(frame_end);
            if !self.speaking && self.consecutive_speech >= self.config.onset_frames {
                self.speaking = true;
                if self.first_speech_sample.is_none() {
                    // Back-date to the start of the run that opened the gate,
                    // so a caller trimming to this offset keeps the whole
                    // onset rather than only the part after the debounce.
                    let run = self.consecutive_speech as u64 * self.frame_samples as u64;
                    self.first_speech_sample = Some(frame_end.saturating_sub(run));
                }
            }
        } else {
            self.consecutive_silence += 1;
            self.consecutive_speech = 0;
            if self.speaking && self.consecutive_silence >= self.config.hangover_frames {
                self.speaking = false;
            }
        }
    }

    /// Whether the gate is currently open (smoothed, not the raw frame).
    pub fn is_speaking(&self) -> bool {
        self.speaking
    }

    /// Whether any speech has been accepted since the last [`reset`](Self::reset).
    pub fn has_heard_speech(&self) -> bool {
        self.first_speech_sample.is_some()
    }

    /// Sample offset where the first accepted speech began, if any.
    pub fn first_speech_sample(&self) -> Option<u64> {
        self.first_speech_sample
    }

    /// Milliseconds of audio since the last frame classified as speech.
    ///
    /// `None` until speech has been heard — silence before anyone has spoken
    /// is a user who has not started yet, not one who has finished, and
    /// auto-stop must never confuse the two.
    pub fn silence_ms_after_speech(&self) -> Option<u64> {
        let last = self.last_speech_end?;
        let elapsed = self.samples_seen.saturating_sub(last);
        Some(elapsed * 1000 / GATE_SAMPLE_RATE as u64)
    }

    /// Clear all state for a new recording.
    pub fn reset(&mut self) {
        self.detector.reset();
        self.pending.clear();
        self.samples_seen = 0;
        self.consecutive_speech = 0;
        self.consecutive_silence = 0;
        self.speaking = false;
        self.first_speech_sample = None;
        self.last_speech_end = None;
    }
}

/// The live gate's state, readable from any thread.
///
/// The gate itself lives on the capture writer thread, which owns the only
/// copy of the resampled audio. Auto-stop (#88) and anything else that wants
/// to know whether the user is talking reads this snapshot rather than
/// reaching for the gate.
#[derive(Debug, Default)]
pub struct SpeechActivity {
    /// Whether a gate is running at all. False between recordings, and during
    /// a recording whose gate failed to start.
    live: AtomicBool,
    speaking: AtomicBool,
    heard_speech: AtomicBool,
    /// Milliseconds of trailing silence, meaningful only once `heard_speech`.
    silence_ms: AtomicU64,
}

impl SpeechActivity {
    /// Whether a live gate is publishing to this snapshot.
    pub fn is_live(&self) -> bool {
        self.live.load(Ordering::Relaxed)
    }

    /// Whether the user is speaking right now (smoothed).
    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::Relaxed)
    }

    /// Whether any speech has been heard in this recording.
    pub fn has_heard_speech(&self) -> bool {
        self.heard_speech.load(Ordering::Relaxed)
    }

    /// Trailing silence in milliseconds, or `None` before any speech.
    pub fn silence_ms_after_speech(&self) -> Option<u64> {
        if !self.has_heard_speech() {
            return None;
        }
        Some(self.silence_ms.load(Ordering::Relaxed))
    }

    /// Mark a new recording's gate as running and clear the previous one's
    /// state. A stale `heard_speech` would let auto-stop fire before the user
    /// had said anything.
    pub fn start(&self) {
        self.speaking.store(false, Ordering::Relaxed);
        self.heard_speech.store(false, Ordering::Relaxed);
        self.silence_ms.store(0, Ordering::Relaxed);
        self.live.store(true, Ordering::Relaxed);
    }

    /// Mark the gate as no longer running.
    pub fn stop(&self) {
        self.live.store(false, Ordering::Relaxed);
        self.speaking.store(false, Ordering::Relaxed);
    }

    /// Publish one gate's current state.
    pub fn publish(&self, gate: &SpeechGate) {
        self.speaking.store(gate.is_speaking(), Ordering::Relaxed);
        if let Some(ms) = gate.silence_ms_after_speech() {
            self.silence_ms.store(ms, Ordering::Relaxed);
        }
        // Stored last: a reader that sees `heard_speech` must also see a
        // `silence_ms` belonging to this recording.
        self.heard_speech
            .store(gate.has_heard_speech(), Ordering::Relaxed);
    }
}

static SPEECH_ACTIVITY: LazyLock<SpeechActivity> = LazyLock::new(SpeechActivity::default);

/// The process-wide live speech state.
pub fn speech_activity() -> &'static SpeechActivity {
    &SPEECH_ACTIVITY
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 16 kHz mono silence.
    fn silence(secs: f32) -> Vec<f32> {
        vec![0.0; (secs * GATE_SAMPLE_RATE as f32) as usize]
    }

    /// The fixtures are 16 kHz mono i16 WAVs of recorded speech.
    fn fixture(name: &str) -> Vec<f32> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let mut reader = hound::WavReader::open(&path)
            .unwrap_or_else(|e| panic!("open {}: {e}", path.display()));
        assert_eq!(reader.spec().sample_rate, GATE_SAMPLE_RATE);
        assert_eq!(reader.spec().channels, 1);
        reader
            .samples::<i16>()
            .map(|s| s.unwrap() as f32 / 32768.0)
            .collect()
    }

    /// A detector driven by a fixed script, so the smoothing is testable
    /// without depending on any engine's judgement.
    struct Scripted {
        frames: std::vec::IntoIter<f32>,
    }

    impl Scripted {
        fn new(frames: Vec<f32>) -> Box<Self> {
            Box::new(Self {
                frames: frames.into_iter(),
            })
        }
    }

    impl SpeechDetector for Scripted {
        fn frame_samples(&self) -> usize {
            480
        }
        fn probability(&mut self, _frame: &[f32]) -> f32 {
            self.frames.next().unwrap_or(0.0)
        }
        fn reset(&mut self) {}
    }

    /// `n` frames' worth of samples. Content is irrelevant to `Scripted`.
    fn frames(n: usize) -> Vec<f32> {
        vec![0.0; n * 480]
    }

    #[test]
    fn silence_is_never_speech() {
        let mut gate = SpeechGate::new(GateConfig::default());
        gate.push(&silence(3.0));
        assert!(!gate.is_speaking());
        assert!(!gate.has_heard_speech());
        assert_eq!(
            gate.silence_ms_after_speech(),
            None,
            "silence before anyone has spoken must not read as trailing silence"
        );
    }

    /// Real recorded speech must open the gate. Without this the whole feature
    /// is a no-op that quietly never fires.
    #[test]
    fn recorded_speech_opens_the_gate() {
        let mut gate = SpeechGate::new(GateConfig::default());
        gate.push(&fixture("speech_no_custom_terms.wav"));
        assert!(gate.has_heard_speech(), "a 4 s sentence must register");
        assert!(
            gate.first_speech_sample().unwrap() < GATE_SAMPLE_RATE as u64,
            "speech starts in the first second of the fixture"
        );
    }

    /// Trailing silence is measured from the end of speech, and it is what
    /// auto-stop (#88) triggers on.
    #[test]
    fn trailing_silence_is_measured_from_the_end_of_speech() {
        let mut gate = SpeechGate::new(GateConfig::default());
        gate.push(&fixture("speech_no_custom_terms.wav"));
        assert!(gate.has_heard_speech());

        gate.push(&silence(2.0));
        let after = gate.silence_ms_after_speech().unwrap();

        assert!(
            (1900..=2100).contains(&after),
            "2 s of silence must read as ~2000 ms of trailing silence, got {after}"
        );
        assert!(
            !gate.is_speaking(),
            "the gate must close over 2 s of silence"
        );
    }

    /// The onset debounce is the difference between "someone spoke" and "a
    /// door slammed". One frame must not open the gate.
    #[test]
    fn one_speech_frame_does_not_open_the_gate() {
        let config = GateConfig {
            onset_frames: 3,
            ..GateConfig::default()
        };
        let mut gate = SpeechGate::with_detector(Scripted::new(vec![1.0, 0.0, 1.0, 1.0]), config);
        gate.push(&frames(4));
        assert!(
            !gate.is_speaking(),
            "runs of 1 and 2 frames must not satisfy an onset of 3"
        );
        assert!(!gate.has_heard_speech());
    }

    /// The hangover is the difference between a pause for breath and the end
    /// of the utterance. A short gap must not close the gate.
    #[test]
    fn a_short_gap_does_not_close_the_gate() {
        let config = GateConfig {
            onset_frames: 2,
            hangover_frames: 5,
            ..GateConfig::default()
        };
        // Speech, a 4-frame gap (one short of the hangover), then speech again.
        let script = vec![1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0];
        let mut gate = SpeechGate::with_detector(Scripted::new(script), config);
        gate.push(&frames(8));
        assert!(
            gate.is_speaking(),
            "a gap shorter than the hangover must not end the utterance"
        );
    }

    /// ...and a gap at the hangover must.
    #[test]
    fn a_long_gap_closes_the_gate() {
        let config = GateConfig {
            onset_frames: 2,
            hangover_frames: 5,
            ..GateConfig::default()
        };
        let script = vec![1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut gate = SpeechGate::with_detector(Scripted::new(script), config);
        gate.push(&frames(7));
        assert!(!gate.is_speaking());
        assert!(
            gate.has_heard_speech(),
            "the gate closing does not unsay what was said"
        );
    }

    /// The onset offset is back-dated to the start of the run, not the frame
    /// that tipped the debounce, so a caller trimming there keeps the onset.
    #[test]
    fn first_speech_is_back_dated_to_the_start_of_the_run() {
        let config = GateConfig {
            onset_frames: 3,
            ..GateConfig::default()
        };
        // Two silent frames, then three of speech: the run starts at frame 2.
        let mut gate =
            SpeechGate::with_detector(Scripted::new(vec![0.0, 0.0, 1.0, 1.0, 1.0]), config);
        gate.push(&frames(5));
        assert_eq!(gate.first_speech_sample(), Some(2 * 480));
    }

    /// Block boundaries must not change the answer: the gate buffers a partial
    /// frame between calls rather than dropping it.
    #[test]
    fn framing_is_independent_of_block_size() {
        let speech = fixture("speech_no_custom_terms.wav");

        let mut whole = SpeechGate::new(GateConfig::default());
        whole.push(&speech);

        let mut dribbled = SpeechGate::new(GateConfig::default());
        // 100 is deliberately coprime with the 480-sample frame.
        for block in speech.chunks(100) {
            dribbled.push(block);
        }

        assert_eq!(whole.is_speaking(), dribbled.is_speaking());
        assert_eq!(whole.first_speech_sample(), dribbled.first_speech_sample());
        assert_eq!(
            whole.silence_ms_after_speech(),
            dribbled.silence_ms_after_speech()
        );
    }

    /// The detector carries adaptive state, so a reused gate must forget the
    /// last recording.
    #[test]
    fn reset_clears_the_previous_recording() {
        let mut gate = SpeechGate::new(GateConfig::default());
        gate.push(&fixture("speech_no_custom_terms.wav"));
        assert!(gate.has_heard_speech());

        gate.reset();
        assert!(!gate.has_heard_speech());
        assert!(!gate.is_speaking());
        assert_eq!(gate.silence_ms_after_speech(), None);

        gate.push(&silence(1.0));
        assert!(
            !gate.has_heard_speech(),
            "silence after a reset stays silent"
        );
    }

    /// The snapshot must not leak one recording's speech into the next.
    #[test]
    fn activity_start_clears_the_previous_recording() {
        let activity = SpeechActivity::default();
        let mut gate = SpeechGate::new(GateConfig::default());
        gate.push(&fixture("speech_no_custom_terms.wav"));

        activity.start();
        activity.publish(&gate);
        assert!(activity.has_heard_speech());

        activity.start();
        assert!(!activity.has_heard_speech());
        assert_eq!(activity.silence_ms_after_speech(), None);
        assert!(activity.is_live());

        activity.stop();
        assert!(!activity.is_live());
    }
}
