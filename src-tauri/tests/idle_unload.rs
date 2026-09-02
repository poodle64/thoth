//! Acceptance test for #105: an idle transcription model is unloaded, its
//! memory comes back, and the next dictation reloads it without erroring.
//!
//! Once loaded, the model used to stay resident for the whole process lifetime —
//! up to 3.1 GB for Whisper large-v3-turbo plus its Metal buffers, ~500 MB of
//! ANE-compiled CoreML for FluidAudio — for a tray app that sits idle between
//! dictations. The decision logic (`should_unload`) and the two refusal paths
//! (`Busy` while a warmup or a transcription holds the model) are unit-tested in
//! `src/transcription/mod.rs`, deterministically and without a model. What can
//! only be proved against a real model is here: that the memory is genuinely
//! released and that a transcription after an unload still succeeds.
//!
//! Ignored by default: it loads a real model from the running user's own
//! `~/.thoth`, which CI has none of.
//!
//! Run with: `cargo test --test idle_unload -- --ignored --nocapture`

use std::time::{Duration, Instant};

use thoth_lib::transcription::{
    UnloadOutcome, get_transcription_backend, init_whisper_transcription, is_transcription_ready,
    maybe_unload_idle_model, transcribe_file, warmup_transcription,
};

/// Any real ASR model is hundreds of megabytes. Ten is a floor low enough that
/// allocator retention cannot fail the test, and high enough that nothing but a
/// genuine release can pass it.
const MIN_RECLAIM_KB: u64 = 10 * 1024;

/// A reload that returns faster than this did not rebuild anything — the same
/// ceiling `warmup_no_reload.rs` uses to tell a no-op from a real load.
const NO_OP_CEILING: Duration = Duration::from_millis(50);

/// This process's memory footprint in KB.
///
/// `ps -o rss=` is not an option: macOS 26 refuses the `rss` keyword without an
/// entitlement. `vmmap --summary` needs none for one's own process and reports
/// the physical footprint, which is the number Activity Monitor shows.
#[cfg(target_os = "macos")]
fn footprint_kb() -> u64 {
    let pid = std::process::id().to_string();
    let out = std::process::Command::new("/usr/bin/vmmap")
        .args(["--summary", &pid])
        .output()
        .expect("vmmap failed");
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text
        .lines()
        .find(|l| l.starts_with("Physical footprint:"))
        .unwrap_or_else(|| panic!("vmmap printed no physical footprint:\n{text}"));
    parse_footprint(line.split_once(':').expect("checked above").1.trim())
}

/// Turn vmmap's "1584K" / "2.3G" into KB.
#[cfg(target_os = "macos")]
fn parse_footprint(value: &str) -> u64 {
    let (number, unit) = value.split_at(value.len() - 1);
    let scale = match unit {
        "K" => 1.0,
        "M" => 1024.0,
        "G" => 1024.0 * 1024.0,
        other => panic!("unknown vmmap unit {other:?} in {value:?}"),
    };
    (number.parse::<f64>().expect("vmmap printed no number") * scale) as u64
}

/// Linux: field 2 of `/proc/self/statm` is the resident set in pages.
#[cfg(not(target_os = "macos"))]
fn footprint_kb() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").expect("no /proc/self/statm");
    let pages: u64 = statm
        .split_whitespace()
        .nth(1)
        .expect("/proc/self/statm has no resident field")
        .parse()
        .expect("resident field is not a number");
    pages * 4 // 4 KB pages
}

#[test]
#[ignore = "loads a real transcription model from the user's own ~/.thoth"]
fn an_idle_model_is_unloaded_and_reloads_on_the_next_transcription() {
    let baseline = footprint_kb();
    println!("footprint before loading anything: {baseline} KB");

    let started = Instant::now();
    warmup_transcription();
    let cold_load = started.elapsed();
    if !is_transcription_ready() {
        // No model on this machine: nothing to unload, and a pass here would
        // prove nothing. Say so rather than reporting green.
        panic!(
            "no transcription model could be loaded from ~/.thoth, so neither the \
             unload nor the reload was exercised — this test proves nothing on \
             this machine"
        );
    }

    let backend = get_transcription_backend().expect("ready, so a backend is loaded");
    let loaded_rss = footprint_kb();
    println!(
        "loaded the {backend} backend in {cold_load:?}; footprint now {loaded_rss} KB \
         (+{} KB)",
        loaded_rss.saturating_sub(baseline)
    );

    // A zero timeout means "idle since the last use", which is now.
    assert_eq!(
        maybe_unload_idle_model(Duration::ZERO),
        UnloadOutcome::Unloaded,
        "an idle model past its timeout was not unloaded"
    );
    assert!(
        !is_transcription_ready(),
        "the unload reported success but a model is still loaded"
    );

    let unloaded_rss = footprint_kb();
    let reclaimed = loaded_rss.saturating_sub(unloaded_rss);
    println!("footprint after the unload: {unloaded_rss} KB (reclaimed {reclaimed} KB)");

    // Whose memory came back depends on where the weights live. Whisper and
    // Parakeet allocate in this process (plus Metal buffers), so the footprint
    // is the observable. FluidAudio hands the CoreML model to the Apple Neural
    // Engine, whose weights are held by a system daemon and never appear in
    // this process's footprint — measured here at ~38 MB loaded, unchanged
    // across the unload. Asserting a reclaim floor on that backend would be
    // asserting something the machine cannot show.
    if backend == "fluidaudio" {
        println!(
            "the {backend} backend keeps its weights on the ANE, outside this process — \
             in-process footprint is not the observable for it"
        );
    } else {
        assert!(
            reclaimed >= MIN_RECLAIM_KB,
            "unloading the {backend} backend released only {reclaimed} KB (from \
             {loaded_rss} to {unloaded_rss}) — the model was dropped from the map \
             without its memory coming back"
        );
    }

    // Nothing is loaded, so a second attempt has nothing to do — and says so
    // distinctly from having refused.
    assert_eq!(
        maybe_unload_idle_model(Duration::ZERO),
        UnloadOutcome::NotLoaded,
        "unloading twice reported something other than an empty service"
    );

    // The user's next dictation: `transcribe_file` is the interactive entry
    // point, and it must reload rather than fail with "not initialised".
    let started = Instant::now();
    let result = transcribe_file(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tone_440hz.wav").to_string(),
    );
    let reload = started.elapsed();
    assert!(
        result.is_ok(),
        "a transcription after an idle unload failed: {:?}",
        result.err()
    );
    assert!(
        is_transcription_ready(),
        "the transcription returned without loading a model — either the reload \
         did not happen, or the fixture was treated as silent and never reached it"
    );
    assert!(
        reload > NO_OP_CEILING,
        "the reload took {reload:?}, which is too fast to have rebuilt anything"
    );
    println!("transcribe after unload (includes the reload): {reload:?}");
}

/// The headline case: Whisper keeps its weights in this process, so unloading
/// one is the memory the issue was raised about (up to 3.1 GB for
/// large-v3-turbo plus its Metal buffers). The machine this was developed on
/// runs the ANE backend and has no Whisper model, so the model comes from an
/// explicit path rather than `~/.thoth`.
///
/// Run with:
/// `THOTH_TEST_WHISPER_MODEL=/path/to/ggml-small.en.bin \
///  cargo test --test idle_unload -- --ignored --nocapture`
#[test]
#[ignore = "needs THOTH_TEST_WHISPER_MODEL pointing at a ggml Whisper model"]
fn unloading_a_whisper_model_returns_its_memory() {
    let Ok(model) = std::env::var("THOTH_TEST_WHISPER_MODEL") else {
        panic!(
            "THOTH_TEST_WHISPER_MODEL is not set, so no Whisper model was loaded and \
             nothing was measured — this test proves nothing without one"
        );
    };

    let baseline = footprint_kb();
    init_whisper_transcription(model.clone()).expect("could not load the Whisper model");
    assert!(
        is_transcription_ready(),
        "init_whisper_transcription returned Ok without loading a model"
    );
    let loaded = footprint_kb();
    println!(
        "loaded {model}: footprint {baseline} -> {loaded} KB (+{} KB)",
        loaded.saturating_sub(baseline)
    );

    assert_eq!(
        maybe_unload_idle_model(Duration::ZERO),
        UnloadOutcome::Unloaded
    );
    let unloaded = footprint_kb();
    let reclaimed = loaded.saturating_sub(unloaded);
    println!("after the unload: {unloaded} KB (reclaimed {reclaimed} KB)");

    assert!(
        reclaimed >= MIN_RECLAIM_KB,
        "unloading released only {reclaimed} KB (from {loaded} to {unloaded}) — the \
         model was dropped from the map without its memory coming back"
    );
}
