//! Regression test for #171: warmup must not rebuild a model that is loaded.
//!
//! `warmup_transcription()` had no readiness guard, and the macOS wake observer
//! subscribes to `NSWorkspaceScreensDidWakeNotification` as well as
//! `NSWorkspaceDidWakeNotification` — so a lid open, a display waking or a
//! monitor hotplug each rebuilt the whole ASR model: 500 MB of ANE-compiled
//! CoreML for FluidAudio, up to 3.1 GB for Whisper large-v3-turbo plus its Metal
//! buffers, and always at 2x peak because the new service is constructed before
//! the old one is dropped. This machine's own logs recorded 6 wake events and 6
//! model loads on 2026-09-01.
//!
//! Time is the observable. Nothing outside the module can see whether a service
//! was rebuilt, but a real load is seconds and a guarded warmup is microseconds,
//! and no honest implementation lands between the two.
//!
//! Ignored by default: it loads a real model from the running user's own
//! `~/.thoth`, which CI has none of.
//!
//! Run with: `cargo test --test warmup_no_reload -- --ignored --nocapture`

use std::time::{Duration, Instant};

use thoth_lib::transcription::{is_transcription_ready, warmup_transcription};

/// A guarded warmup returns without touching the model, so it cannot plausibly
/// take this long. A real load is three orders of magnitude above it.
const NO_OP_CEILING: Duration = Duration::from_millis(50);

#[test]
#[ignore = "loads a real transcription model from the user's own ~/.thoth"]
fn repeat_warmup_does_not_reload_a_loaded_model() {
    let started = Instant::now();
    warmup_transcription();
    let cold = started.elapsed();

    if !is_transcription_ready() {
        // No model on this machine: there is no loaded state to guard, and a
        // pass here would prove nothing. Say so rather than reporting green.
        panic!(
            "no transcription model could be loaded from ~/.thoth, so the \
             already-loaded path was never exercised — this test proves nothing \
             on this machine"
        );
    }
    println!("cold warmup: {cold:?}");

    for attempt in 1..=5 {
        let started = Instant::now();
        warmup_transcription();
        let repeat = started.elapsed();
        println!("repeat warmup {attempt}: {repeat:?}");
        assert!(
            repeat < NO_OP_CEILING,
            "repeat warmup {attempt} took {repeat:?} (cold load was {cold:?}) — \
             the model was rebuilt (#171)"
        );
    }

    assert!(
        is_transcription_ready(),
        "a repeat warmup unloaded the model"
    );
}
