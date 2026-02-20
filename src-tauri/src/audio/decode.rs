//! Audio file decoding and conversion to 16kHz mono WAV
//!
//! Uses symphonia for format-agnostic decoding (MP3, M4A, OGG, FLAC, WAV)
//! and the existing AudioConverter (rubato) for high-quality resampling.

use crate::audio::format::AudioConverter;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Maximum file size for import (500 MB)
const MAX_FILE_SIZE: u64 = 500 * 1024 * 1024;

/// Target sample rate for transcription
const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Chunk size for the rubato resampler (frames per call)
const RESAMPLE_CHUNK_SIZE: usize = 1024;

/// Check cancellation every N packets
const CANCEL_CHECK_INTERVAL: u32 = 50;

/// Decode an audio file to 16kHz mono WAV suitable for transcription.
///
/// Supports WAV, MP3, M4A (AAC), OGG Vorbis, and FLAC formats.
/// Returns the audio duration in seconds on success.
pub fn decode_audio_to_wav(
    input_path: &Path,
    output_path: &Path,
    cancel: &AtomicBool,
) -> Result<f64, String> {
    // Validate file exists and check size
    let metadata = std::fs::metadata(input_path).map_err(|e| format!("Cannot read file: {}", e))?;

    if metadata.len() > MAX_FILE_SIZE {
        return Err(format!(
            "File is too large ({:.0} MB). Maximum supported size is {} MB.",
            metadata.len() as f64 / (1024.0 * 1024.0),
            MAX_FILE_SIZE / (1024 * 1024)
        ));
    }

    // Fast path: if already 16kHz mono WAV, copy directly
    if is_target_format_wav(input_path) {
        tracing::info!("Audio file is already 16kHz mono WAV, copying directly");
        std::fs::copy(input_path, output_path)
            .map_err(|e| format!("Failed to copy WAV file: {}", e))?;
        return get_wav_duration(output_path);
    }

    // Open the file and create a media source stream
    let file =
        std::fs::File::open(input_path).map_err(|e| format!("Failed to open audio file: {}", e))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    // Hint the format from file extension
    let mut hint = Hint::new();
    if let Some(ext) = input_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    // Probe the format
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("Unsupported audio format: {}", e))?;

    let mut format = probed.format;

    // Find the first audio track with a decodeable codec
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| "No supported audio track found in file".to_string())?;

    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let source_rate = codec_params
        .sample_rate
        .ok_or_else(|| "Cannot determine sample rate from audio file".to_string())?;
    let source_channels = codec_params.channels.map(|c| c.count()).unwrap_or(1);

    tracing::info!(
        "Decoding: {}Hz, {} channels -> {}Hz mono",
        source_rate,
        source_channels,
        TARGET_SAMPLE_RATE
    );

    // Create the decoder
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Unsupported audio codec: {}", e))?;

    // Create the resampler (handles arbitrary sample rate ratios)
    let mut converter = AudioConverter::new(
        source_rate,
        TARGET_SAMPLE_RATE,
        source_channels,
        RESAMPLE_CHUNK_SIZE,
    )
    .map_err(|e| format!("Failed to create resampler: {}", e))?;

    // Open the output WAV file
    let wav_spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET_SAMPLE_RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut wav_writer = hound::WavWriter::create(output_path, wav_spec)
        .map_err(|e| format!("Failed to create output WAV file: {}", e))?;

    // Decode loop with chunk buffering for rubato
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut resample_buffer: Vec<f32> = Vec::new();
    let mut total_source_frames: u64 = 0;
    let mut packet_count: u32 = 0;
    let frames_per_chunk = RESAMPLE_CHUNK_SIZE * source_channels;

    loop {
        // Periodic cancellation check
        if packet_count % CANCEL_CHECK_INTERVAL == 0 && cancel.load(Ordering::Relaxed) {
            // Clean up partial output
            drop(wav_writer);
            let _ = std::fs::remove_file(output_path);
            return Err("Import cancelled".to_string());
        }
        packet_count += 1;

        // Get next packet
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break; // End of stream
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(format!("Error reading audio: {}", e)),
        };

        // Skip packets from other tracks
        if packet.track_id() != track_id {
            continue;
        }

        // Decode the packet
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::IoError(_)) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Decode error: {}", e)),
        };

        // Convert to interleaved f32 via SampleBuffer
        let spec = *decoded.spec();
        let num_frames = decoded.capacity();

        let sbuf =
            sample_buf.get_or_insert_with(|| SampleBuffer::<f32>::new(num_frames as u64, spec));

        // Recreate if capacity changed
        if sbuf.capacity() < num_frames {
            *sbuf = SampleBuffer::<f32>::new(num_frames as u64, spec);
        }

        sbuf.copy_interleaved_ref(decoded);
        let samples = sbuf.samples();
        let frame_count = samples.len() / source_channels.max(1);
        total_source_frames += frame_count as u64;

        // Accumulate in resample buffer
        resample_buffer.extend_from_slice(samples);

        // Drain in exact chunk sizes for rubato
        while resample_buffer.len() >= frames_per_chunk {
            let chunk: Vec<f32> = resample_buffer.drain(..frames_per_chunk).collect();
            let resampled = converter
                .process_to_i16(&chunk)
                .map_err(|e| format!("Resampling error: {}", e))?;

            for &sample in &resampled {
                wav_writer
                    .write_sample(sample)
                    .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
            }
        }
    }

    // Flush remaining samples in the buffer (pad with zeros to fill a chunk)
    if !resample_buffer.is_empty() {
        resample_buffer.resize(frames_per_chunk, 0.0);
        let resampled = converter
            .process_to_i16(&resample_buffer)
            .map_err(|e| format!("Resampling error during flush: {}", e))?;

        for &sample in &resampled {
            wav_writer
                .write_sample(sample)
                .map_err(|e| format!("Failed to write WAV sample: {}", e))?;
        }
    }

    wav_writer
        .finalize()
        .map_err(|e| format!("Failed to finalise WAV file: {}", e))?;

    let duration = if source_rate > 0 {
        total_source_frames as f64 / source_rate as f64
    } else {
        0.0
    };

    tracing::info!(
        "Decoded {:.2}s of audio to {}",
        duration,
        output_path.display()
    );
    Ok(duration)
}

/// Check if a WAV file is already in 16kHz mono i16 format (fast path).
fn is_target_format_wav(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if !ext.eq_ignore_ascii_case("wav") {
        return false;
    }

    match hound::WavReader::open(path) {
        Ok(reader) => {
            let spec = reader.spec();
            spec.sample_rate == TARGET_SAMPLE_RATE
                && spec.channels == 1
                && spec.sample_format == hound::SampleFormat::Int
                && spec.bits_per_sample == 16
        }
        Err(_) => false,
    }
}

/// Get the duration of a WAV file from its header.
fn get_wav_duration(path: &Path) -> Result<f64, String> {
    let reader =
        hound::WavReader::open(path).map_err(|e| format!("Failed to read WAV file: {}", e))?;
    let spec = reader.spec();
    let num_samples = reader.len() as f64;
    let duration = num_samples / (spec.sample_rate as f64 * spec.channels as f64);
    Ok(duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn test_file_size_rejection() {
        // Create a temp file path that doesn't exist but has known metadata issues
        let cancel = AtomicBool::new(false);
        let result = decode_audio_to_wav(
            Path::new("/nonexistent/file.mp3"),
            Path::new("/tmp/out.wav"),
            &cancel,
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Cannot read file"));
    }

    #[test]
    fn test_cancellation() {
        let cancel = AtomicBool::new(true); // Pre-cancelled
                                            // Use a file that exists but will be cancelled immediately
        let input = tempfile::NamedTempFile::new().unwrap();
        let output = tempfile::NamedTempFile::new().unwrap();

        // Write a minimal valid WAV header so it gets past the size check
        // but the decode loop checks cancellation
        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(input.path(), spec).unwrap();
            for _ in 0..44100 {
                writer.write_sample(0i16).unwrap();
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let result = decode_audio_to_wav(input.path(), output.path(), &cancel);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cancelled"));
    }

    #[test]
    fn test_wav_fast_path() {
        let cancel = AtomicBool::new(false);

        // Create a 16kHz mono WAV (target format)
        let input = tempfile::NamedTempFile::with_suffix(".wav").unwrap();
        let output = tempfile::NamedTempFile::with_suffix(".wav").unwrap();

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(input.path(), spec).unwrap();
            // Write 1 second of silence
            for _ in 0..16000 {
                writer.write_sample(0i16).unwrap();
            }
            writer.finalize().unwrap();
        }

        let result = decode_audio_to_wav(input.path(), output.path(), &cancel);
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert!(
            (duration - 1.0).abs() < 0.01,
            "Expected ~1.0s, got {}",
            duration
        );

        // Verify output is valid WAV
        let reader = hound::WavReader::open(output.path()).unwrap();
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.spec().channels, 1);
    }

    #[test]
    fn test_wav_resample() {
        let cancel = AtomicBool::new(false);

        // Create a 44.1kHz stereo WAV (needs conversion)
        let input = tempfile::NamedTempFile::with_suffix(".wav").unwrap();
        let output = tempfile::NamedTempFile::with_suffix(".wav").unwrap();

        let spec = hound::WavSpec {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut writer = hound::WavWriter::create(input.path(), spec).unwrap();
            // Write ~1 second of a simple tone (stereo)
            for i in 0..44100 {
                let sample = ((i as f64 * 440.0 * 2.0 * std::f64::consts::PI / 44100.0).sin()
                    * 16000.0) as i16;
                writer.write_sample(sample).unwrap();
                writer.write_sample(sample).unwrap();
            }
            writer.finalize().unwrap();
        }

        let result = decode_audio_to_wav(input.path(), output.path(), &cancel);
        assert!(result.is_ok());
        let duration = result.unwrap();
        assert!(
            (duration - 1.0).abs() < 0.05,
            "Expected ~1.0s, got {}",
            duration
        );

        // Verify output is 16kHz mono
        let reader = hound::WavReader::open(output.path()).unwrap();
        assert_eq!(reader.spec().sample_rate, 16000);
        assert_eq!(reader.spec().channels, 1);
    }
}
