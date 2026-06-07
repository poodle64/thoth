//! Headless Parakeet smoke test.
//!
//! Exercises the real `transcription::parakeet` path (Sherpa-ONNX, CPU provider
//! on Linux) end to end — model load, WAV decode, transcript — without the GUI
//! or a microphone. Used to runtime-verify the Parakeet backend on Linux, where
//! the dev box is macOS and the build box is headless (see #53/#81).
//!
//! Usage:
//!   cargo run --example parakeet_smoke --no-default-features \
//!       --features vulkan,parakeet -- <model_dir> <audio.wav>
//!
//! `model_dir` must contain encoder.int8.onnx, decoder.int8.onnx,
//! joiner.int8.onnx and tokens.txt; `audio.wav` should be 16 kHz mono.

#[cfg(feature = "parakeet")]
fn main() -> anyhow::Result<()> {
    use std::path::Path;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let model_dir = args
        .next()
        .expect("usage: parakeet_smoke <model_dir> <audio.wav>");
    let wav = args
        .next()
        .expect("usage: parakeet_smoke <model_dir> <audio.wav>");

    let start = std::time::Instant::now();
    let mut service =
        thoth_lib::transcription::parakeet::TranscriptionService::new(Path::new(&model_dir))?;
    eprintln!("Model loaded in {:.1}s", start.elapsed().as_secs_f32());

    let text = service.transcribe(Path::new(&wav))?;
    println!("TRANSCRIPT: {text}");
    Ok(())
}

#[cfg(not(feature = "parakeet"))]
fn main() {
    eprintln!("Build with --features parakeet to run this example.");
    std::process::exit(2);
}
