//! Acceptance test for #106: the user's own terms bias Whisper's decoding.
//!
//! Dictionary and canonical correction only ever ran after decoding, so it could
//! repair a word the model got close to and nothing else. This measures the
//! decode-time bias the way the issue asks: the same audio, once without the
//! prompt and once with it, plus a recording that never mentions the terms, to
//! prove the prompt does not pull its own words into unrelated audio — the known
//! whisper.cpp failure mode this feature could otherwise introduce.
//!
//! The fixtures are `say(1)` speech at 16 kHz mono, in the repo rather than on
//! one machine. The terms are invented names the model gets wrong unprompted:
//! `ggml-small.en` hears "Thalvan and Zarnac".
//!
//! Run with:
//! `THOTH_TEST_WHISPER_MODEL=/path/to/ggml-small.en.bin \
//!  cargo test --test vocabulary_bias -- --ignored --nocapture`

use thoth_lib::transcription::whisper::WhisperTranscriptionService;

/// Spoken in `speech_custom_terms.wav`, absent from `speech_no_custom_terms.wav`.
const TERMS: [&str; 2] = ["Thalvyn", "Zarnak"];

fn model() -> String {
    std::env::var("THOTH_TEST_WHISPER_MODEL").unwrap_or_else(|_| {
        panic!(
            "THOTH_TEST_WHISPER_MODEL is not set, so no model was loaded and nothing \
             was measured — this test proves nothing without one"
        )
    })
}

fn samples(fixture: &str) -> Vec<f32> {
    let path = format!("{}/tests/fixtures/{fixture}", env!("CARGO_MANIFEST_DIR"));
    let mut reader = hound::WavReader::open(&path).expect("fixture is readable");
    let spec = reader.spec();
    assert_eq!(spec.channels, 1, "{fixture} is not mono");
    assert_eq!(spec.sample_rate, 16_000, "{fixture} is not 16 kHz");
    reader
        .samples::<i16>()
        .map(|s| s.expect("fixture sample") as f32 / 32768.0)
        .collect()
}

#[test]
#[ignore = "needs THOTH_TEST_WHISPER_MODEL pointing at a ggml Whisper model"]
fn the_users_terms_bias_decoding_without_leaking_into_other_audio() {
    let service =
        WhisperTranscriptionService::new(std::path::Path::new(&model())).expect("model loads");
    let prompt = TERMS.join(", ");

    let spoken = samples("speech_custom_terms.wav");
    let unbiased = service
        .transcribe_samples_biased(&spoken, None)
        .expect("transcribes");
    let biased = service
        .transcribe_samples_biased(&spoken, Some(&prompt))
        .expect("transcribes");
    println!("unbiased: {unbiased:?}");
    println!("  biased: {biased:?}");

    for term in TERMS {
        assert!(
            !unbiased.contains(term),
            "the model already produced {term:?} unprompted ({unbiased:?}), so this \
             fixture measures nothing — the term must be one it gets wrong"
        );
        assert!(
            biased.contains(term),
            "the bias did not produce {term:?}: {biased:?}"
        );
    }

    let unrelated = samples("speech_no_custom_terms.wav");
    let control = service
        .transcribe_samples_biased(&unrelated, Some(&prompt))
        .expect("transcribes");
    println!(" control: {control:?}");
    for term in TERMS {
        assert!(
            !control.contains(term),
            "the prompt leaked {term:?} into audio that never mentioned it: {control:?}"
        );
    }
}
