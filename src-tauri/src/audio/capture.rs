//! Audio capture using cpal with lock-free ring buffer
//!
//! This module provides real-time safe audio recording. The audio callback
//! uses a lock-free ring buffer to avoid allocations.

use super::ring_buffer::AudioRingBuffer;
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Audio recorder using cpal
pub struct AudioRecorder {
    stream: Option<cpal::Stream>,
    writer_handle: Option<std::thread::JoinHandle<Result<()>>>,
    stop_signal: Arc<AtomicBool>,
    output_path: Option<PathBuf>,
    ring_buffer: Arc<AudioRingBuffer>,
    /// Optional secondary ring buffer for VAD or other consumers
    secondary_buffer: Option<Arc<AudioRingBuffer>>,
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

        // Build input stream
        let stream = device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                // LOCK-FREE: Ring buffer write does not allocate
                let written = callback_buffer.write(data);
                if written < data.len() {
                    // Buffer overflow - samples dropped (this is worth logging)
                    tracing::warn!(
                        "Audio buffer overflow: dropped {} samples",
                        data.len() - written
                    );
                }

                // Write to secondary buffer if present (for VAD processing)
                if let Some(ref secondary) = secondary_callback_buffer {
                    secondary.write(data);
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

/// Write audio from ring buffer to WAV file
fn write_audio_to_file(
    ring_buffer: Arc<AudioRingBuffer>,
    path: &Path,
    source_rate: u32,
    source_channels: usize,
    stop_signal: Arc<AtomicBool>,
) -> Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    tracing::info!(
        "Writer thread starting: source_rate={}, channels={}, output={}",
        source_rate,
        source_channels,
        path.display()
    );

    let mut writer = hound::WavWriter::create(path, spec)?;
    let mut read_buffer = vec![0.0f32; 4096];
    let mut total_samples = 0usize;

    while !stop_signal.load(Ordering::SeqCst) {
        let read = ring_buffer.read(&mut read_buffer);
        if read > 0 {
            let processed =
                downsample_and_convert(&read_buffer[..read], source_rate, source_channels);
            for sample in &processed {
                writer.write_sample(*sample)?;
            }
            total_samples += processed.len();
        } else {
            // No data available, sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // Drain remaining samples from buffer
    loop {
        let read = ring_buffer.read(&mut read_buffer);
        if read == 0 {
            break;
        }
        let processed = downsample_and_convert(&read_buffer[..read], source_rate, source_channels);
        for sample in &processed {
            writer.write_sample(*sample)?;
        }
        total_samples += processed.len();
    }

    writer.finalize()?;
    tracing::debug!("Audio writer finished: {} samples written", total_samples);
    Ok(())
}

/// Downsample and convert audio to 16kHz mono i16
///
/// This is a simple decimation - proper resampling will be added in WP-02.3
fn downsample_and_convert(samples: &[f32], source_rate: u32, channels: usize) -> Vec<i16> {
    let ratio = (source_rate as usize) / 16000;

    // Mix channels to mono and decimate
    samples
        .chunks(channels)
        .step_by(ratio.max(1))
        .map(|frame| {
            // Average all channels for mono mix
            let mono: f32 = frame.iter().sum::<f32>() / frame.len() as f32;
            // Convert to i16
            (mono * 32767.0).clamp(-32768.0, 32767.0) as i16
        })
        .collect()
}

/// Downsample and convert audio to 16kHz mono f32
///
/// Similar to `downsample_and_convert` but outputs f32 for VAD processing.
/// Public for use by vad_recorder module.
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
    fn test_downsample_stereo_to_mono() {
        // Stereo 48kHz -> mono 16kHz (ratio 3)
        let stereo: Vec<f32> = vec![0.5, -0.5, 0.3, -0.3, 0.1, -0.1]; // 3 stereo frames
        let result = downsample_and_convert(&stereo, 48000, 2);
        // Should have 1 sample (every 3rd frame)
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_downsample_preserves_values() {
        // Mono at 16kHz (no downsampling needed)
        let mono = vec![0.5f32, 0.25, 0.0, -0.25, -0.5];
        let result = downsample_and_convert(&mono, 16000, 1);
        assert_eq!(result.len(), 5);
        assert_eq!(result[0], (0.5 * 32767.0) as i16);
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
