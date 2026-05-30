//! Audio capture using cpal with lock-free ring buffer
//!
//! This module provides real-time safe audio recording. The audio callback
//! uses a lock-free ring buffer to avoid allocations. The recorder keeps the
//! cpal stream open ("warm") between recordings so that pressing record is an
//! instant flag flip rather than a ~150ms device open.

use super::format::AudioConverter;
use super::ring_buffer::AudioRingBuffer;
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Target sample rate for transcription (whisper.cpp / Parakeet expect 16kHz mono).
const TARGET_SAMPLE_RATE: u32 = 16000;

/// Resampler input chunk size in frames. Matches the import path (decode.rs) so
/// live capture and file import resample through the same well-tested path.
const RESAMPLE_CHUNK_SIZE: usize = 1024;

/// Audio recorder using cpal with warm-stream lifecycle.
///
/// Lifecycle:
/// 1. `warm_up` — opens the cpal device and starts the stream. The callback
///    runs continuously but only writes to the recording ring buffer while armed.
/// 2. `arm` — starts the writer thread and flips the armed flag so samples flow
///    into the recording buffer. Does NOT open the device (instant).
/// 3. `disarm` — flips the armed flag and joins the writer thread. Does NOT
///    close the device. Returns the path to the finished WAV.
/// 4. `cool_down` — closes the device. Called after an idle timeout or when
///    the device changes.
pub struct AudioRecorder {
    /// The warm cpal stream (open from `warm_up` to `cool_down`).
    stream: Option<cpal::Stream>,
    /// Writer thread that drains the ring buffer to a WAV file.
    writer_handle: Option<std::thread::JoinHandle<Result<()>>>,
    /// Signals the writer thread to stop draining.
    stop_signal: Arc<AtomicBool>,
    /// Path where the current/last recording is being written.
    output_path: Option<PathBuf>,
    /// Primary recording ring buffer (stable for the warm stream's lifetime).
    ring_buffer: Arc<AudioRingBuffer>,
    /// Optional secondary ring buffer for VAD or other consumers.
    secondary_buffer: Option<Arc<AudioRingBuffer>>,
    /// Optional ring buffer for real-time metering (recording indicator waveform).
    metering_buffer: Option<Arc<AudioRingBuffer>>,
    /// Source sample rate captured at warm_up time.
    source_rate: Option<u32>,
    /// Source channel count captured at warm_up time.
    source_channels: Option<usize>,
    /// Whether the callback should write samples to the recording ring buffer.
    armed: Arc<AtomicBool>,
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRecorder {
    /// Create a new audio recorder.
    pub fn new() -> Self {
        Self {
            stream: None,
            writer_handle: None,
            stop_signal: Arc::new(AtomicBool::new(false)),
            output_path: None,
            ring_buffer: Arc::new(AudioRingBuffer::new()),
            secondary_buffer: None,
            metering_buffer: None,
            source_rate: None,
            source_channels: None,
            armed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set a secondary ring buffer that will receive audio data when armed.
    ///
    /// Must be called before `warm_up` — the callback captures buffer clones at warm-up time.
    pub fn set_secondary_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.secondary_buffer = Some(buffer);
    }

    /// Clear the secondary buffer.
    pub fn clear_secondary_buffer(&mut self) {
        self.secondary_buffer = None;
    }

    /// Set a dedicated metering ring buffer.
    ///
    /// The metering buffer receives samples continuously while warm (even before
    /// arming), so the recording indicator can show levels without delay. Must be
    /// called before `warm_up` — the callback captures the clone at warm-up time.
    pub fn set_metering_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.metering_buffer = Some(buffer);
    }

    /// Clear the metering buffer.
    pub fn clear_metering_buffer(&mut self) {
        self.metering_buffer = None;
    }

    /// Whether the recorder is currently armed (actively capturing to a WAV file).
    pub fn is_recording(&self) -> bool {
        self.armed.load(Ordering::Relaxed)
    }

    /// Whether the cpal stream is open (warm).
    pub fn is_warm(&self) -> bool {
        self.stream.is_some()
    }

    /// Open the cpal input stream and start it playing ("warm").
    ///
    /// If a warm stream is already open, this is a no-op (the same device
    /// identity is assumed; callers must cool_down then re-warm on device change).
    /// Buffers set via `set_metering_buffer` / `set_secondary_buffer` must be
    /// in place before calling this.
    #[allow(deprecated)] // cpal 0.17 deprecates name() but description() is not yet stable
    pub fn warm_up(&mut self, device: &cpal::Device) -> Result<()> {
        if self.stream.is_some() {
            tracing::debug!("AudioRecorder::warm_up: stream already open, no-op");
            return Ok(());
        }

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!("AudioRecorder::warm_up: opening device '{}'", device_name);

        let supported_config = device.default_input_config()?;
        let source_rate = supported_config.sample_rate();
        let source_channels = supported_config.channels() as usize;

        tracing::info!(
            "Warm stream: device='{}', {}Hz, {} channels",
            device_name,
            source_rate,
            source_channels,
        );

        self.source_rate = Some(source_rate);
        self.source_channels = Some(source_channels);

        // Clone all buffers — the callback holds these for the stream's lifetime.
        let callback_ring = self.ring_buffer.clone();
        let callback_secondary = self.secondary_buffer.clone();
        let callback_metering = self.metering_buffer.clone();
        let callback_armed = self.armed.clone();

        let stream = device.build_input_stream(
            &supported_config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // Metering always runs while warm (regardless of armed state) so
                // the recording indicator can show levels before the user hits record.
                if let Some(ref m) = callback_metering {
                    m.write(data);
                }

                // Recording buffers only receive data while armed.
                // LOCK-FREE: armed check + ring buffer writes never allocate.
                if callback_armed.load(Ordering::Relaxed) {
                    let written = callback_ring.write(data);
                    if written < data.len() {
                        tracing::warn!(
                            "Audio buffer overflow: dropped {} samples",
                            data.len() - written
                        );
                    }

                    if let Some(ref secondary) = callback_secondary {
                        secondary.write(data);
                    }
                }
            },
            |err| {
                tracing::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);
        tracing::info!("AudioRecorder::warm_up: stream open and playing");
        Ok(())
    }

    /// Convenience wrapper: open the default input device and warm up.
    pub fn warm_up_default(&mut self) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device available"))?;
        self.warm_up(&device)
    }

    /// Arm the recorder: prepare the output file and start the writer thread.
    ///
    /// Requires the stream to already be warm. This is intentionally instant —
    /// it sets the armed flag only AFTER the writer thread is spawned and ready.
    pub fn arm(&mut self, output_path: &Path) -> Result<()> {
        if self.stream.is_none() {
            return Err(anyhow!(
                "Cannot arm: stream is not warm. Call warm_up() first."
            ));
        }
        if self.armed.load(Ordering::Relaxed) {
            return Err(anyhow!("Already armed / recording in progress"));
        }

        let source_rate = self
            .source_rate
            .ok_or_else(|| anyhow!("source_rate not set — warm_up() must have failed"))?;
        let source_channels = self
            .source_channels
            .ok_or_else(|| anyhow!("source_channels not set — warm_up() must have failed"))?;

        tracing::info!("AudioRecorder::arm: output={}", output_path.display());

        // Discard any samples that arrived before arming.
        self.ring_buffer.clear();

        self.stop_signal.store(false, Ordering::SeqCst);
        self.output_path = Some(output_path.to_path_buf());

        // Spawn the writer thread BEFORE setting armed=true so it is reading
        // before any samples can flow.
        let ring_buffer = self.ring_buffer.clone();
        let stop_signal = self.stop_signal.clone();
        let writer_path = output_path.to_path_buf();

        self.writer_handle = Some(std::thread::spawn(move || {
            write_audio_to_file(
                ring_buffer,
                &writer_path,
                source_rate,
                source_channels,
                stop_signal,
            )
        }));

        // Armed flag is set LAST so the callback doesn't write until the writer
        // thread is running.
        self.armed.store(true, Ordering::SeqCst);
        tracing::info!("AudioRecorder::arm: armed, recording started");
        Ok(())
    }

    /// Disarm the recorder: stop sampling into the recording buffer and finalise the WAV.
    ///
    /// The cpal stream stays open (warm). Returns the path to the finished WAV file.
    pub fn disarm(&mut self) -> Result<PathBuf> {
        if !self.armed.load(Ordering::Relaxed) {
            return Err(anyhow!("Not armed / no recording in progress"));
        }

        // Stop new samples entering the recording buffer first.
        self.armed.store(false, Ordering::SeqCst);

        // Signal the writer thread to drain remaining samples and finalise.
        self.stop_signal.store(true, Ordering::SeqCst);

        if let Some(handle) = self.writer_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow!("Writer thread panicked"))??;
        }

        let path = self
            .output_path
            .take()
            .ok_or_else(|| anyhow!("output_path missing after disarm"))?;

        tracing::info!(
            "AudioRecorder::disarm: recording saved to {}",
            path.display()
        );
        Ok(path)
    }

    /// Close the cpal stream ("cool down").
    ///
    /// Called on idle timeout, device change, or sleep/wake. Refuses to act
    /// while a recording is armed — tearing the stream down mid-capture would
    /// silently lose the recording. Callers must stop the recording first (via
    /// the normal stop-and-process path) if they need to cool down during one.
    pub fn cool_down(&mut self) {
        if self.armed.load(Ordering::Relaxed) {
            tracing::warn!("AudioRecorder::cool_down skipped — a recording is in progress");
            return;
        }

        if let Some(stream) = self.stream.take() {
            drop(stream);
            tracing::info!("AudioRecorder::cool_down: stream closed");
        }

        self.source_rate = None;
        self.source_channels = None;
    }

    // -------------------------------------------------------------------------
    // Legacy single-call API (used by tests and VAD recorder path)
    // These wrap warm_up + arm or disarm + cool_down for callers that don't
    // need the split lifecycle.
    // -------------------------------------------------------------------------

    /// Start recording from the default input device (legacy single-call API).
    pub fn start_default(&mut self, output_path: &Path) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device available"))?;
        self.start(&device, output_path)
    }

    /// Start recording from a specific device (legacy single-call API).
    #[allow(deprecated)]
    pub fn start(&mut self, device: &cpal::Device, output_path: &Path) -> Result<()> {
        if self.stream.is_some() {
            return Err(anyhow!("Recording already in progress"));
        }
        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!("AudioRecorder::start (legacy): device='{}'", device_name);

        self.warm_up(device)?;
        self.arm(output_path)?;
        Ok(())
    }

    /// Stop recording and return the path to the recorded file (legacy single-call API).
    pub fn stop(&mut self) -> Result<PathBuf> {
        let path = self.disarm()?;
        self.cool_down();
        Ok(path)
    }

    /// Get the source sample rate (only valid while warm).
    pub fn source_rate(&self) -> Option<u32> {
        self.source_rate
    }

    /// Get the source channel count (only valid while warm).
    pub fn source_channels(&self) -> Option<usize> {
        self.source_channels
    }
}

/// Write audio from ring buffer to WAV file.
///
/// Resamples the device-native interleaved f32 stream to 16kHz mono i16 using the
/// same anti-aliased rubato resampler the file-import path uses (`AudioConverter`),
/// rather than naive decimation. Decimation without a low-pass filter aliases
/// content above 8kHz back into the speech band and corrupts transcription input;
/// it also mislabelled any non-48kHz device (e.g. a 44.1kHz mic would have been
/// written too fast). The WAV header is stamped at the true 16kHz output rate.
fn write_audio_to_file(
    ring_buffer: Arc<AudioRingBuffer>,
    path: &Path,
    source_rate: u32,
    source_channels: usize,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    tracing::info!(
        "Writer thread starting: source_rate={}, channels={}, target={}, output={}",
        source_rate,
        source_channels,
        TARGET_SAMPLE_RATE,
        path.display()
    );

    let mut converter = AudioConverter::new(
        source_rate,
        TARGET_SAMPLE_RATE,
        source_channels,
        RESAMPLE_CHUNK_SIZE,
    )
    .map_err(|e| anyhow!("Failed to create resampler: {}", e))?;

    // The resampler consumes fixed-size chunks of interleaved frames. Accumulate
    // the variable-sized ring-buffer reads and drain in exact chunks.
    let frames_per_chunk = RESAMPLE_CHUNK_SIZE * source_channels;
    let mut accumulator: Vec<f32> = Vec::with_capacity(frames_per_chunk * 2);

    let mut writer = hound::WavWriter::create(path, spec)?;
    let mut read_buffer = vec![0.0f32; 4096];
    let mut total_samples = 0usize;

    let drain_full_chunks = |accumulator: &mut Vec<f32>,
                             converter: &mut AudioConverter,
                             writer: &mut hound::WavWriter<std::io::BufWriter<std::fs::File>>,
                             total: &mut usize|
     -> Result<()> {
        while accumulator.len() >= frames_per_chunk {
            let chunk: Vec<f32> = accumulator.drain(..frames_per_chunk).collect();
            let resampled = converter
                .process_to_i16(&chunk)
                .map_err(|e| anyhow!("Resampling error: {}", e))?;
            for sample in &resampled {
                writer.write_sample(*sample)?;
            }
            *total += resampled.len();
        }
        Ok(())
    };

    while !stop_signal.load(Ordering::SeqCst) {
        let read = ring_buffer.read(&mut read_buffer);
        if read > 0 {
            accumulator.extend_from_slice(&read_buffer[..read]);
            drain_full_chunks(
                &mut accumulator,
                &mut converter,
                &mut writer,
                &mut total_samples,
            )?;
        } else {
            // No data available, sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // Drain whatever the recorder produced after the stop signal.
    loop {
        let read = ring_buffer.read(&mut read_buffer);
        if read == 0 {
            break;
        }
        accumulator.extend_from_slice(&read_buffer[..read]);
        drain_full_chunks(
            &mut accumulator,
            &mut converter,
            &mut writer,
            &mut total_samples,
        )?;
    }

    // Flush the final partial chunk by zero-padding it to a full chunk (matches
    // the import path); the trailing silence is negligible at 16kHz.
    if !accumulator.is_empty() {
        accumulator.resize(frames_per_chunk, 0.0);
        let resampled = converter
            .process_to_i16(&accumulator)
            .map_err(|e| anyhow!("Resampling error during flush: {}", e))?;
        for sample in &resampled {
            writer.write_sample(*sample)?;
        }
        total_samples += resampled.len();
    }

    writer.finalize()?;
    tracing::debug!("Audio writer finished: {} samples written", total_samples);
    Ok(())
}

/// Decimate and mix to 16kHz mono f32 for VAD processing.
///
/// Naive integer decimation (no anti-alias filter); adequate for the coarse
/// energy/VAD use it serves. Public for use by the vad_recorder module.
pub fn downsample_to_mono_f32(samples: &[f32], source_rate: u32, channels: usize) -> Vec<f32> {
    let ratio = (source_rate as usize) / 16000;

    samples
        .chunks(channels)
        .step_by(ratio.max(1))
        .map(|frame| {
            // Average all channels for mono mix
            frame.iter().sum::<f32>() / frame.len() as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_recorder_new() {
        let recorder = AudioRecorder::new();
        assert!(!recorder.is_recording());
        assert!(!recorder.is_warm());
    }

    #[test]
    fn test_writer_produces_correct_rate_wav() {
        // A recorded WAV must be stamped at 16kHz and contain roughly the
        // expected number of 16kHz samples for the input duration — proving
        // the writer genuinely resamples rather than relabelling decimated data.
        // Feed 1 second of 48kHz stereo silence-ish signal through the writer.
        let dir = tempdir().unwrap();
        let path = dir.path().join("rate_test.wav");

        let ring = Arc::new(AudioRingBuffer::new());
        let stop = Arc::new(AtomicBool::new(false));

        // Spawn the writer, then feed it ~0.25s of 48kHz stereo audio in chunks
        // small enough to fit the 65536-sample ring buffer, then stop.
        let writer_ring = ring.clone();
        let writer_stop = stop.clone();
        let writer_path = path.clone();
        let handle = std::thread::spawn(move || {
            write_audio_to_file(writer_ring, &writer_path, 48000, 2, writer_stop)
        });

        // 0.25s at 48kHz stereo = 12000 frames = 24000 interleaved samples.
        let frame = [0.2f32, -0.2f32];
        let mut fed = 0;
        while fed < 12000 {
            let mut written = 0;
            while written < frame.len() {
                written += ring.write(&frame[written..]);
                if written < frame.len() {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
            fed += 1;
        }
        // Give the writer a moment to drain, then stop.
        std::thread::sleep(std::time::Duration::from_millis(50));
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap().unwrap();

        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000, "header must be stamped at 16kHz");
        assert_eq!(spec.channels, 1, "output must be mono");

        // 0.25s of input should yield ~4000 samples at 16kHz; allow generous
        // tolerance for resampler latency and flush padding.
        let n = reader.into_samples::<i16>().count();
        assert!(
            n > 3000 && n < 5000,
            "expected ~4000 samples for 0.25s at 16kHz, got {}",
            n
        );

        fs::remove_file(path).ok();
    }

    #[test]
    fn test_record_and_stop() {
        // Skip if no audio device available (CI environment)
        let host = cpal::default_host();
        if host.default_input_device().is_none() {
            println!("No audio device available, skipping test");
            return;
        }

        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test_recording.wav");

        let mut recorder = AudioRecorder::new();

        // Test both the split lifecycle and the warm/is_warm checks.
        assert!(!recorder.is_warm());
        assert!(recorder.start_default(&output_path).is_ok());
        assert!(recorder.is_recording());

        // Record for 500ms
        std::thread::sleep(std::time::Duration::from_millis(500));

        let result_path = recorder.stop().unwrap();
        assert!(!recorder.is_recording());
        assert!(result_path.exists());

        // Verify it's a valid WAV file
        let reader = hound::WavReader::open(&result_path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, 16000);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);

        // Clean up
        fs::remove_file(result_path).ok();
    }

    #[test]
    fn test_arm_requires_warm() {
        let dir = tempdir().unwrap();
        let output_path = dir.path().join("test.wav");
        let mut recorder = AudioRecorder::new();
        // Calling arm without warming up must return an error.
        assert!(recorder.arm(&output_path).is_err());
    }

    #[test]
    fn test_disarm_without_arm_returns_error() {
        let mut recorder = AudioRecorder::new();
        assert!(recorder.disarm().is_err());
    }
}
