//! Regression test for the FluidAudio "downloaded" signal.
//!
//! FluidAudio writes a `.fluidaudio_ready` sentinel under `~/.thoth/models/`
//! once, on first successful init, but the models it actually loads are the
//! compiled CoreML bundles under `~/Library/Application Support/FluidAudio/
//! Models/`. Clearing that cache leaves the sentinel behind, so every layer
//! that trusted the sentinel — tray status, the record guard, the Settings
//! "Active" badge — reported a usable model that could not transcribe, and
//! recording then wedged for 60 s on a model that never arrived.
//!
//! Readiness is therefore the cache, not the sentinel. This drives both states
//! against the real functions, with `HOME` pointed at a throwaway directory so
//! nothing touches the developer's own models:
//!
//!   sentinel present, cache absent  -> NOT downloaded  (the regression)
//!   sentinel present, cache present ->     downloaded  (proves the check can
//!                                                       still pass, so the
//!                                                       false above is a real
//!                                                       decision and not a
//!                                                       structurally dead path)
//!
//! `HOME` is process-global and both paths derive from it, so this lives in its
//! own integration binary with a single test.
#![cfg(all(target_os = "macos", feature = "fluidaudio"))]

use tempfile::TempDir;
use thoth_lib::transcription::{download, fluidaudio, manifest};

#[test]
fn fluidaudio_readiness_follows_the_coreml_cache_not_the_sentinel() {
    let home = TempDir::new().expect("temp home");
    unsafe { std::env::set_var("HOME", home.path()) };

    let fallback = manifest::get_fallback_manifest();
    let model = fallback
        .models
        .iter()
        .find(|m| m.model_type == "fluidaudio_coreml")
        .expect("the fallback manifest ships a FluidAudio model");

    // The stale sentinel the old check trusted: present, non-empty, exactly
    // where `required_files` says to look.
    let model_dir = manifest::get_model_directory(&model.id);
    std::fs::create_dir_all(&model_dir).expect("model dir");
    std::fs::write(model_dir.join(".fluidaudio_ready"), b"ready").expect("sentinel");

    // No CoreML cache yet — the state a user reaches by clearing it.
    assert!(
        !fluidaudio::is_cached(),
        "the throwaway HOME must start with no CoreML cache"
    );
    assert!(
        !manifest::is_model_downloaded(model),
        "a lingering .fluidaudio_ready sentinel must not report the model downloaded"
    );
    assert!(
        !download::check_model_downloaded(Some(model.id.clone())),
        "check_model_downloaded must agree with the cache, not the sentinel"
    );

    // Populate the cache: the same inputs must now report ready.
    let cache_dir = fluidaudio::model_cache_directory();
    std::fs::create_dir_all(&cache_dir).expect("cache dir");
    std::fs::write(cache_dir.join("parakeet-tdt-0.6b-v3-coreml"), b"model").expect("cache entry");

    assert!(fluidaudio::is_cached(), "a populated cache reads as cached");
    assert!(
        manifest::is_model_downloaded(model),
        "with the CoreML cache present the model is downloaded"
    );
    assert!(
        download::check_model_downloaded(Some(model.id.clone())),
        "check_model_downloaded agrees once the cache is present"
    );
}
