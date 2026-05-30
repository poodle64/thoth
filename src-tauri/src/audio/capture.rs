//! Audio capture using cpal with lock-free ring buffer
//!
//! This module provides real-time safe audio recording. The audio callback
//! uses a lock-free ring buffer to avoid allocations.

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

/// Audio recorder using cpal
pub struct AudioRecorder {
    stream: Option<cpal::Stream>,
    writer_handle: Option<std::thread::JoinHandle<Result<()>>>,
    stop_signal: Arc<AtomicBool>,
    output_path: Option<PathBuf>,
    ring_buffer: Arc<AudioRingBuffer>,
    /// Optional secondary ring buffer for VAD or other consumers
    secondary_buffer: Option<Arc<AudioRingBuffer>>,
    /// Optional ring buffer for real-time metering (recording indicator waveform)
    metering_buffer: Option<Arc<AudioRingBuffer>>,
    /// Source sample rate (set during recording)
    source_rate: Option<u32>,
    /// Source channel count (set during recording)
    source_channels: Option<usize>,
}

impl Default for AudioRecorder {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioRecorder {
    /// Create a new audio recorder
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
        }
    }

    /// Set a secondary ring buffer that will receive audio data alongside the primary buffer.
    ///
    /// This allows external consumers (e.g., VAD processing) to receive audio data
    /// without creating a separate audio input stream. Must be called before `start()`.
    pub fn set_secondary_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.secondary_buffer = Some(buffer);
    }

    /// Clear the secondary buffer
    pub fn clear_secondary_buffer(&mut self) {
        self.secondary_buffer = None;
    }

    /// Set a dedicated metering ring buffer that receives audio data alongside the primary buffer.
    ///
    /// Routes raw device-native samples to the metering consumer without opening a second
    /// device stream. Must be called before `start()`.
    pub fn set_metering_buffer(&mut self, buffer: Arc<AudioRingBuffer>) {
        self.metering_buffer = Some(buffer);
    }

    /// Clear the metering buffer
    pub fn clear_metering_buffer(&mut self) {
        self.metering_buffer = None;
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// Start recording from the default input device
    #[allow(deprecated)] // cpal 0.17 deprecates name() but description() is not yet stable
    pub fn start_default(&mut self, output_path: &Path) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device available"))?;

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!("Using default input device: {}", device_name);

        self.start(&device, output_path)
    }

    /// Start recording from a specific device
    #[allow(deprecated)]
    pub fn start(&mut self, device: &cpal::Device, output_path: &Path) -> Result<()> {
        if self.stream.is_some() {
            return Err(anyhow!("Recording already in progress"));
        }

        let device_name = device.name().unwrap_or_else(|_| "Unknown".to_string());
        tracing::info!("AudioRecorder::start called for device: {}", device_name);

        let supported_config = device.default_input_config()?;
        // cpal 0.17 returns u32 directly, not a tuple
        let source_rate = supported_config.sample_rate();
        let source_channels = supported_config.channels() as usize;
        let sample_format = supported_config.sample_format();

        tracing::info!(
            "Starting recording: device='{}', {}Hz, {} channels, format={:?}, output={}",
            device_name,
            source_rate,
            source_channels,
            sample_format,
            output_path.display()
        );

        // Convert to StreamConfig for building the stream
        let config = supported_config;

        // Reset state
        self.stop_signal.store(false, Ordering::SeqCst);
        self.output_path = Some(output_path.to_path_buf());
        self.ring_buffer = Arc::new(AudioRingBuffer::new());
        self.source_rate = Some(source_rate);
        self.source_channels = Some(source_channels);

        // Clone for the writer thread
        let ring_buffer = self.ring_buffer.clone();
        let stop_signal = self.stop_signal.clone();
        let writer_path = output_path.to_path_buf();

        // Writer thread - reads from ring buffer and writes to file
        self.writer_handle = Some(std::thread::spawn(move || {
            write_audio_to_file(
                ring_buffer,
                &writer_path,
                source_rate,
                source_channels,
                stop_signal,
            )
        }));

        // Clone ring buffers for the audio callback
        let callback_buffer = self.ring_buffer.clone();
        let secondary_callback_buffer = self.secondary_buffer.clone();
        let metering_callback_buffer = self.metering_buffer.clone();

        // Build input stream
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // LOCK-FREE: Ring buffer write does not allocate
                let written = callback_buffer.write(data);
                if written < data.len() {
                    tracing::warn!(
                        "Audio buffer overflow: dropped {} samples",
                        data.len() - written
                    );
                }

                // Write to secondary buffer if present (for VAD processing)
                if let Some(ref secondary) = secondary_callback_buffer {
                    secondary.write(data);
                }

                // Write to metering buffer if present (for recording-indicator waveform)
                if let Some(ref m) = metering_callback_buffer {
                    m.write(data);
                }
            },
            |err| {
                tracing::error!("Audio stream error: {}", err);
            },
            None,
        )?;

        stream.play()?;
        self.stream = Some(stream);

        tracing::info!("Recording started");
        Ok(())
    }

    /// Stop recording and return the path to the recorded file
    pub fn stop(&mut self) -> Result<PathBuf> {
        // Signal writer to stop
        self.stop_signal.store(true, Ordering::SeqCst);

        // Stop the audio stream
        if let Some(stream) = self.stream.take() {
            drop(stream);
        }

        // Wait for writer thread to finish
        if let Some(handle) = self.writer_handle.take() {
            handle
                .join()
                .map_err(|_| anyhow!("Writer thread panicked"))??;
        }

        let path = self
            .output_path
            .take()
            .ok_or_else(|| anyhow!("No recording in progress"))?;

        tracing::info!("Recording stopped: {}", path.display());
        Ok(path)
    }

    /// Get the source sample rate (only valid during recording)
    pub fn source_rate(&self) -> Option<u32> {
        self.source_rate
    }

    /// Get the source channel count (only valid during recording)
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
}
