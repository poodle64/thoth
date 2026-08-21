//! Smoothing over a raw per-frame speech/noise decision.
//!
//! A neural VAD gives a probability per frame. Acting on that raw signal
//! directly clips speech at both ends:
//!
//! - **At onset**, the first frames of a word are quiet (a soft consonant, an
//!   indrawn breath before the vowel), so the detector fires a frame or two
//!   late and the start of the word is already gone.
//! - **At offset**, a natural pause mid-sentence reads as silence, so a
//!   detector that cuts on the first non-speech frame truncates the utterance.
//!
//! This module wraps the raw decision with the three mitigations Handy's
//! `SmoothedVad` uses, and is the piece #88 (hands-free auto-stop) needs to be
//! correct, since that feature's stated main risk is clipping the end of
//! speech:
//!
//! 1. **Onset debounce** — require N consecutive speech frames before
//!    declaring speech, so a single noisy frame (a keystroke, a chair creak)
//!    does not open a segment.
//! 2. **Pre-roll** — keep a ring buffer of the frames immediately before the
//!    onset and retroactively include them, which recovers the quiet frames
//!    the detector missed *and* the frames the debounce deliberately withheld.
//! 3. **Hangover** — keep the segment open across short silences, so a pause
//!    for breath does not end the utterance.
//!
//! The module is deliberately free of audio and of any VAD engine: it consumes
//! `bool` per frame and returns a decision. That makes the clipping behaviour,
//! which is the part that is easy to get subtly wrong, testable on its own.

/// How long to hold a segment open across silence before declaring it over.
///
/// The two policies exist because the right answer depends on what consumes
/// the segment, not on the audio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HangoverPolicy {
    /// Batch/offline use: the whole recording is transcribed at the end, so a
    /// shorter tail is enough to avoid truncating a sentence.
    Offline,
    /// Streaming/interactive use: a longer tail, because ending a segment
    /// early is far more costly when the result is acted on immediately (it
    /// cuts the user off mid-thought).
    Streaming,
}

/// Consecutive speech frames required before a segment opens.
const ONSET_FRAMES: usize = 2;
/// Silence tail for [`HangoverPolicy::Offline`].
const OFFLINE_HANGOVER_MS: usize = 450;
/// Silence tail for [`HangoverPolicy::Streaming`].
const STREAMING_HANGOVER_MS: usize = 1650;
/// Audio retained before an onset.
const PREROLL_MS: usize = 450;

/// Frames needed to cover `ms`, rounded up so a tail is never short.
const fn frames_for_ms(ms: usize, frame_ms: usize) -> usize {
    // frame_ms is engine-supplied and never zero, but guard rather than divide
    // by zero if a future engine reports something odd.
    if frame_ms == 0 {
        return 0;
    }
    ms.div_ceil(frame_ms)
}

impl HangoverPolicy {
    /// The tail expressed in milliseconds. This is the real constant; frame
    /// counts are derived from it, because the frame size belongs to whichever
    /// VAD engine is running (Silero v5 wants 32 ms, WebRTC wants 10/20/30 ms)
    /// and hardcoding one here would silently mistune the other.
    pub const fn hangover_ms(self) -> usize {
        match self {
            HangoverPolicy::Offline => OFFLINE_HANGOVER_MS,
            HangoverPolicy::Streaming => STREAMING_HANGOVER_MS,
        }
    }

    /// Frames of silence tolerated before the segment is declared over, for an
    /// engine whose frames are `frame_ms` long.
    pub const fn hangover_frames(self, frame_ms: usize) -> usize {
        frames_for_ms(self.hangover_ms(), frame_ms)
    }
}

/// Tuning for [`SmoothedVad`].
#[derive(Debug, Clone, Copy)]
pub struct SmoothingConfig {
    /// Consecutive speech frames required to open a segment.
    pub onset_frames: usize,
    /// Frames of silence tolerated before closing a segment.
    pub hangover_frames: usize,
    /// Maximum frames retroactively included ahead of an onset.
    pub preroll_frames: usize,
}

impl SmoothingConfig {
    /// Build a config from a hangover policy for an engine whose frames are
    /// `frame_ms` long.
    pub const fn for_policy(policy: HangoverPolicy, frame_ms: usize) -> Self {
        Self {
            onset_frames: ONSET_FRAMES,
            hangover_frames: policy.hangover_frames(frame_ms),
            preroll_frames: frames_for_ms(PREROLL_MS, frame_ms),
        }
    }
}

/// A segment boundary crossed on a given frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// A speech segment opened on this frame.
    SpeechStart,
    /// The segment closed on this frame; this frame is not part of it.
    SpeechEnd,
}

/// What the caller should do with one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameDecision {
    /// Whether this frame belongs to a speech segment. Frames inside the
    /// hangover tail are active: that is the point of the tail.
    pub active: bool,
    /// On a [`Transition::SpeechStart`] frame, how many immediately preceding
    /// frames the caller should retroactively include. Zero on every other
    /// frame.
    ///
    /// This is what stops the debounce from eating the start of a word: the
    /// frames withheld while waiting for confirmation are handed back here.
    pub preroll_frames: usize,
    /// The boundary crossed on this frame, if any.
    pub transition: Option<Transition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// No segment open.
    Silence,
    /// Segment open, last frame was speech.
    Speech,
    /// Segment open but running on the hangover tail; `silence_run` frames of
    /// silence seen so far.
    Hangover,
}

/// Smooths a raw per-frame speech decision into segment boundaries.
///
/// See the module docs for what each mitigation is for.
pub struct SmoothedVad {
    config: SmoothingConfig,
    state: State,
    /// Consecutive speech frames seen while waiting to confirm an onset.
    speech_run: usize,
    /// Consecutive silence frames seen while running on the hangover tail.
    silence_run: usize,
    /// Non-active frames seen since the last segment, capped at
    /// `preroll_frames`. This is a count rather than buffered audio: the
    /// caller owns the audio and only needs to know how far back to reach.
    preroll_available: usize,
}

impl SmoothedVad {
    /// Create a smoother with the given tuning.
    pub fn new(config: SmoothingConfig) -> Self {
        Self {
            config,
            state: State::Silence,
            speech_run: 0,
            silence_run: 0,
            preroll_available: 0,
        }
    }

    /// Create a smoother using the defaults for a hangover policy, tuned for
    /// an engine whose frames are `frame_ms` long.
    pub fn with_policy(policy: HangoverPolicy, frame_ms: usize) -> Self {
        Self::new(SmoothingConfig::for_policy(policy, frame_ms))
    }

    /// Whether a segment is currently open.
    pub fn is_active(&self) -> bool {
        matches!(self.state, State::Speech | State::Hangover)
    }

    /// Feed one frame's raw speech decision and get back what to do with it.
    pub fn push(&mut self, is_speech: bool) -> FrameDecision {
        match self.state {
            State::Silence => self.push_while_silent(is_speech),
            State::Speech | State::Hangover => self.push_while_active(is_speech),
        }
    }

    fn push_while_silent(&mut self, is_speech: bool) -> FrameDecision {
        if !is_speech {
            self.speech_run = 0;
            // Grow the pre-roll window, capped: frames older than the window
            // are of no use to a future onset.
            self.preroll_available = (self.preroll_available + 1).min(self.config.preroll_frames);
            return FrameDecision {
                active: false,
                preroll_frames: 0,
                transition: None,
            };
        }

        self.speech_run += 1;
        if self.speech_run < self.config.onset_frames {
            // Not yet confirmed. Withhold the frame, but count it as pre-roll
            // so the onset hands it back rather than losing it.
            self.preroll_available = (self.preroll_available + 1).min(self.config.preroll_frames);
            return FrameDecision {
                active: false,
                preroll_frames: 0,
                transition: None,
            };
        }

        // Onset confirmed. The pre-roll covers the frames withheld during the
        // debounce plus the quiet frames the detector missed before them.
        let preroll = self.preroll_available;
        self.state = State::Speech;
        self.speech_run = 0;
        self.silence_run = 0;
        self.preroll_available = 0;

        FrameDecision {
            active: true,
            preroll_frames: preroll,
            transition: Some(Transition::SpeechStart),
        }
    }

    fn push_while_active(&mut self, is_speech: bool) -> FrameDecision {
        if is_speech {
            self.state = State::Speech;
            self.silence_run = 0;
            return FrameDecision {
                active: true,
                preroll_frames: 0,
                transition: None,
            };
        }

        self.state = State::Hangover;
        self.silence_run += 1;

        if self.silence_run <= self.config.hangover_frames {
            // Still inside the tail: keep the segment open and keep the frame.
            // A pause for breath must not end the utterance.
            return FrameDecision {
                active: true,
                preroll_frames: 0,
                transition: None,
            };
        }

        // Tail exhausted: the segment is over. This frame is silence and is
        // not part of it, so it seeds the next pre-roll window.
        self.state = State::Silence;
        self.speech_run = 0;
        self.silence_run = 0;
        self.preroll_available = 1;

        FrameDecision {
            active: false,
            preroll_frames: 0,
            transition: Some(Transition::SpeechEnd),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Silero v5's frame size; the engine this smoothing actually runs on.
    const FRAME_MS: usize = 32;

    /// Push a sequence of raw decisions, returning one result per frame.
    fn run(vad: &mut SmoothedVad, frames: &[bool]) -> Vec<FrameDecision> {
        frames.iter().map(|&f| vad.push(f)).collect()
    }

    #[test]
    fn hangover_policies_match_their_documented_durations() {
        assert_eq!(HangoverPolicy::Offline.hangover_ms(), 450);
        assert_eq!(HangoverPolicy::Streaming.hangover_ms(), 1650);
        assert!(
            HangoverPolicy::Streaming.hangover_frames(FRAME_MS)
                > HangoverPolicy::Offline.hangover_frames(FRAME_MS),
            "streaming must tolerate longer pauses than offline"
        );
    }

    #[test]
    fn a_single_noisy_frame_does_not_open_a_segment() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);

        // One isolated speech frame among silence: a keystroke, not a word.
        let out = run(&mut vad, &[false, false, true, false, false]);

        assert!(
            out.iter().all(|d| !d.active),
            "onset debounce must reject a single-frame transient"
        );
        assert!(out.iter().all(|d| d.transition.is_none()));
    }

    #[test]
    fn segment_opens_only_after_the_debounce_is_satisfied() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);

        let out = run(&mut vad, &[false, true, true]);

        assert!(!out[1].active, "first speech frame is still unconfirmed");
        assert!(out[2].active, "second consecutive speech frame confirms onset");
        assert_eq!(out[2].transition, Some(Transition::SpeechStart));
    }

    #[test]
    fn onset_hands_back_the_frames_the_debounce_withheld() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);

        // 3 silent frames, then speech. The onset frame must reach back over
        // both the silence and the withheld first speech frame.
        let out = run(&mut vad, &[false, false, false, true, true]);
        let onset = out[4];

        assert_eq!(onset.transition, Some(Transition::SpeechStart));
        assert_eq!(
            onset.preroll_frames, 4,
            "pre-roll must cover 3 silent frames plus the withheld speech frame, \
             otherwise the start of the word is lost"
        );
    }

    #[test]
    fn preroll_is_capped_at_the_configured_window() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        let config = SmoothingConfig::for_policy(HangoverPolicy::Offline, FRAME_MS);

        // Far more silence than the window, then an onset.
        let mut frames = vec![false; 200];
        frames.extend([true, true]);
        let out = run(&mut vad, &frames);

        let onset = *out.last().unwrap();
        assert_eq!(
            onset.preroll_frames, config.preroll_frames,
            "pre-roll must saturate at the window rather than growing unbounded"
        );
    }

    #[test]
    fn preroll_cannot_exceed_the_frames_actually_seen() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);

        // Speech from the very first frame: there is almost nothing behind it.
        let out = run(&mut vad, &[true, true]);
        let onset = out[1];

        assert_eq!(
            onset.preroll_frames, 1,
            "only the withheld frame exists to reach back to"
        );
    }

    #[test]
    fn a_short_pause_does_not_end_the_segment() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        let hangover = HangoverPolicy::Offline.hangover_frames(FRAME_MS);

        run(&mut vad, &[true, true]); // open a segment
        let pause = run(&mut vad, &vec![false; hangover]);

        assert!(
            pause.iter().all(|d| d.active),
            "a pause for breath within the hangover must keep the segment open"
        );
        assert!(
            pause.iter().all(|d| d.transition.is_none()),
            "no boundary may be reported inside the hangover"
        );
        assert!(vad.is_active());
    }

    #[test]
    fn segment_ends_one_frame_after_the_hangover_is_exhausted() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        let hangover = HangoverPolicy::Offline.hangover_frames(FRAME_MS);

        run(&mut vad, &[true, true]);
        run(&mut vad, &vec![false; hangover]);
        let closing = vad.push(false);

        assert_eq!(closing.transition, Some(Transition::SpeechEnd));
        assert!(
            !closing.active,
            "the frame that closes the segment is silence and is not part of it"
        );
        assert!(!vad.is_active());
    }

    #[test]
    fn speech_resuming_inside_the_hangover_keeps_one_segment() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        let hangover = HangoverPolicy::Offline.hangover_frames(FRAME_MS);

        run(&mut vad, &[true, true]);
        run(&mut vad, &vec![false; hangover - 1]);
        let resumed = run(&mut vad, &[true, true, true]);

        assert!(resumed.iter().all(|d| d.active));
        assert!(
            resumed.iter().all(|d| d.transition.is_none()),
            "resuming inside the tail must not split one utterance into two"
        );

        // The tail must have been reset, not merely paused: a fresh full-length
        // pause is tolerated again.
        let pause = run(&mut vad, &vec![false; hangover]);
        assert!(
            pause.iter().all(|d| d.active),
            "hangover must reset on resumed speech"
        );
    }

    #[test]
    fn streaming_policy_tolerates_a_pause_that_offline_would_cut() {
        let offline_tail = HangoverPolicy::Offline.hangover_frames(FRAME_MS);
        let pause = vec![false; offline_tail + 5];

        let mut offline = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        run(&mut offline, &[true, true]);
        let offline_out = run(&mut offline, &pause);

        let mut streaming = SmoothedVad::with_policy(HangoverPolicy::Streaming, FRAME_MS);
        run(&mut streaming, &[true, true]);
        let streaming_out = run(&mut streaming, &pause);

        assert!(
            offline_out.iter().any(|d| d.transition == Some(Transition::SpeechEnd)),
            "offline policy should close on a pause this long"
        );
        assert!(
            streaming_out.iter().all(|d| d.transition.is_none()),
            "streaming policy must hold the same pause open"
        );
    }

    #[test]
    fn a_second_utterance_gets_its_own_preroll() {
        let mut vad = SmoothedVad::with_policy(HangoverPolicy::Offline, FRAME_MS);
        let hangover = HangoverPolicy::Offline.hangover_frames(FRAME_MS);

        // First utterance, then a gap long enough to close it.
        run(&mut vad, &[true, true]);
        run(&mut vad, &vec![false; hangover + 1]);
        assert!(!vad.is_active());

        // Silence, then a second utterance.
        run(&mut vad, &vec![false; 5]);
        let out = run(&mut vad, &[true, true]);
        let onset = out[1];

        assert_eq!(onset.transition, Some(Transition::SpeechStart));
        assert!(
            onset.preroll_frames > 0,
            "the second utterance must get its own pre-roll, not start clipped"
        );
    }
}
