//! VAD-enabled audio recording
//!
//! This module provides `VadRecorder`, which wraps `AudioRecorder` to add
//! Voice Activity Detection (VAD) processing during recording. VAD events
//! are sent through a channel for external consumers.
//!
//! Unlike the previous implementation which created a separate audio stream,
//! this version uses a secondary ring buffer that receives audio data from
//! the same audio callback as the primary buffer, eliminating resource duplication.

use super::capture::{downsample_to_mono_f32, AudioRecorder};
use super::ring_buffer::AudioRingBuffer;
use super::vad::{VadConfig, VadEvent, VoiceActivityDetector};
use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait};
use crossbeam_channel::{bounded, Receiver, Sender};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// VAD event receiver for external consumers
pub type VadEventReceiver = Receiver<VadEvent>;

/// Audio recorder with integrated Voice Activity Detection
///
/// Wraps `AudioRecorder` to add VAD processing during recording.
/// VAD events (speech start, speech end, auto-stop) are sent through a channel.
///
/// This implementation uses a secondary ring buffer that receives audio data
/// from the same audio callback, avoiding the need for a separate audio input stream.
pub struct VadRecorder {
    recorder: AudioRecorder,
    vad_config: VadConfig,
    vad_thread: Option<std::thread::JoinHandle<()>>,
    vad_stop_signal: Arc<AtomicBool>,
    event_sender: Sender<VadEvent>,
    event_receiver: Receiver<VadEvent>,
    auto_stop_triggered: Arc<AtomicBool>,
    /// Ring buffer dedicated to VAD processing
    vad_ring_buffer: Arc<AudioRingBuffer>,
}

impl Default for VadRecorder {
    fn default() -> Self {
        Self::new(VadConfig::default())
    }
}

impl VadRecorder {
    /// Create a new VAD-enabled recorder with the given configuration
    pub fn new(vad_config: VadConfig) -> Self {
        let (sender, receiver) = bounded(64);
        Self {
            recorder: AudioRecorder::new(),
            vad_config,
            vad_thread: None,
            vad_stop_signal: Arc::new(AtomicBool::new(false)),
            event_sender: sender,
            event_receiver: receiver,
            auto_stop_triggered: Arc::new(AtomicBool::new(false)),
            vad_ring_buffer: Arc::new(AudioRingBuffer::new()),
        }
    }

    /// Update VAD configuration
    ///
    /// Only takes effect on the next recording; does not affect current recording.
    pub fn set_config(&mut self, config: VadConfig) {
        self.vad_config = config;
    }

    /// Get the current VAD configuration
    pub fn config(&self) -> &VadConfig {
        &self.vad_config
    }

    /// Check if recording is in progress
    pub fn is_recording(&self) -> bool {
        self.recorder.is_recording()
    }

    /// Check if auto-stop was triggered
    pub fn auto_stop_triggered(&self) -> bool {
        self.auto_stop_triggered.load(Ordering::SeqCst)
    }

    /// Get the event receiver for VAD events
    ///
    /// Clone this to receive VAD events in another thread.
    pub fn event_receiver(&self) -> Receiver<VadEvent> {
        self.event_receiver.clone()
    }

    /// Start recording with VAD processing from the default input device
    pub fn start_default(&mut self, output_path: &Path) -> Result<()> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| anyhow!("No default input device available"))?;

        self.start(&device, output_path)
    }

    /// Start recording with VAD processing from a specific device
    pub fn start(&mut self, device: &cpal::Device, output_path: &Path) -> Result<()> {
        if self.recorder.is_recording() {
            return Err(anyhow!("Recording already in progress"));
        }

        // Get device config for sample rate and channels (needed before starting recorder)
        let config = device.default_input_config()?;
        let source_rate = config.sample_rate(); // cpal 0.17 returns u32 directly
        let source_channels = config.channels() as usize;

        // Reset state
        self.vad_stop_signal.store(false, Ordering::SeqCst);
        self.auto_stop_triggered.store(false, Ordering::SeqCst);

        // Create new event channel for this recording session
        let (sender, receiver) = bounded(64);
        self.event_sender = sender;
        self.event_receiver = receiver;

        // Create a fresh VAD ring buffer for this recording session
        self.vad_ring_buffer = Arc::new(AudioRingBuffer::new());

        // Configure the recorder to write to our VAD buffer as a secondary output
        // This way, the audio callback writes to both buffers from a single stream
        self.recorder
            .set_secondary_buffer(self.vad_ring_buffer.clone());

        // Start the underlying recorder
        self.recorder.start(device, output_path)?;

        let ring_buffer = self.vad_ring_buffer.clone();
        let vad_stop = self.vad_stop_signal.clone();
        let vad_config = self.vad_config.clone();
        let event_tx = self.event_sender.clone();
        let auto_stop_flag = self.auto_stop_triggered.clone();

        // Spawn VAD processing thread that reads from the dedicated VAD ring buffer
        let vad_thread = std::thread::spawn(move || {
            process_vad(
                ring_buffer,
                vad_stop,
                vad_config,
                source_rate,
                source_channels,
                event_tx,
                auto_stop_flag,
            );
        });

        self.vad_thread = Some(vad_thread);

        tracing::info!("VAD-enabled recording started");
        Ok(())
    }

    /// Stop recording and return the path to the recorded file
    pub fn stop(&mut self) -> Result<PathBuf> {
        // Signal VAD thread to stop
        self.vad_stop_signal.store(true, Ordering::SeqCst);

        // Wait for VAD thread
        if let Some(handle) = self.vad_thread.take() {
            let _ = handle.join();
        }

        // Clear the secondary buffer from the recorder
        self.recorder.clear_secondary_buffer();

        // Stop the underlying recorder
        let path = self.recorder.stop()?;

        tracing::info!("VAD-enabled recording stopped");
        Ok(path)
    }

    /// Try to receive pending VAD events without blocking
    pub fn try_recv_event(&self) -> Option<VadEvent> {
        self.event_receiver.try_recv().ok()
    }
}

/// VAD processing loop
///
/// Reads audio from the dedicated VAD ring buffer, processes through VAD, and sends events.
/// The ring buffer is populated by the audio callback alongside the primary recording buffer.
fn process_vad(
    ring_buffer: Arc<AudioRingBuffer>,
    stop_signal: Arc<AtomicBool>,
    config: VadConfig,
    source_rate: u32,
    source_channels: usize,
    event_tx: Sender<VadEvent>,
    auto_stop_flag: Arc<AtomicBool>,
) {
    let mut vad = VoiceActivityDetector::new(config.clone());
    let frame_size = vad.frame_size();

    // Buffer for reading from ring buffer
    let mut read_buffer = vec![0.0f32; 4096];
    // Buffer for accumulating samples for VAD frames
    let mut vad_buffer: Vec<f32> = Vec::with_capacity(frame_size * 4);

    while !stop_signal.load(Ordering::SeqCst) {
        // Check for auto-stop condition
        if let Some(event) = vad.check_auto_stop() {
            tracing::info!("VAD auto-stop triggered");
            let _ = event_tx.try_send(event);
            auto_stop_flag.store(true, Ordering::SeqCst);
            break;
        }

        // Read available audio from the shared ring buffer
        let read = ring_buffer.read(&mut read_buffer);
        if read > 0 {
            // Downsample to 16kHz mono for VAD
            let mono_16k =
                downsample_to_mono_f32(&read_buffer[..read], source_rate, source_channels);
            vad_buffer.extend(mono_16k);

            // Process complete frames
            while vad_buffer.len() >= frame_size {
                let frame: Vec<f32> = vad_buffer.drain(..frame_size).collect();

                match vad.process_frame_f32(&frame) {
                    Ok(Some(event)) => {
                        tracing::debug!("VAD event: {:?}", event);
                        let _ = event_tx.try_send(event);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        tracing::warn!("VAD processing error: {}", e);
                    }
                }
            }
        } else {
            // No data available, sleep briefly
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    tracing::debug!("VAD processing thread exiting");
}
