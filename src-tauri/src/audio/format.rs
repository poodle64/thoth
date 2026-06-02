//! Audio format conversion using rubato resampler
//!
//! Provides high-quality resampling from device sample rate (typically 48kHz)
//! to 16kHz for transcription, including stereo to mono conversion.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

/// Audio format converter for resampling and channel conversion
pub struct AudioConverter {
    resampler: SincFixedIn<f32>,
    source_channels: usize,
}

impl AudioConverter {
    /// Create a new audio converter
    ///
    /// # Arguments
    /// * `source_rate` - Source sample rate (e.g., 48000)
    /// * `target_rate` - Target sample rate (typically 16000)
    /// * `source_channels` - Number of source channels (1 or 2)
    /// * `chunk_size` - Size of input chunks in frames (e.g., 1024)
    pub fn new(
        source_rate: u32,
        target_rate: u32,
        source_channels: usize,
        chunk_size: usize,
    ) -> Result<Self, rubato::ResamplerConstructionError> {
        let params = SincInterpolationParameters {
            sinc_len: 256,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Linear,
            oversampling_factor: 256,
            window: WindowFunction::BlackmanHarris2,
        };

        let resampler = SincFixedIn::new(
            target_rate as f64 / source_rate as f64,
            2.0, // max_resample_ratio_relative
            params,
            chunk_size,
            1, // mono output
        )?;

        Ok(Self {
            resampler,
            source_channels,
        })
    }

    /// Process audio samples
    ///
    /// Converts stereo to mono, resamples to target rate, and returns f32 samples.
    pub fn process(&mut self, input: &[f32]) -> Result<Vec<f32>, rubato::ResampleError> {
        let mono = self.to_mono(input);
        let waves_in = [mono];
        let waves_out = self.resampler.process(&waves_in, None)?;
        Ok(waves_out.into_iter().next().unwrap_or_default())
    }

    /// Process and convert to i16
    ///
    /// This is a convenience method that calls `process` and converts the output
    /// to 16-bit signed integers suitable for WAV files.
    pub fn process_to_i16(&mut self, input: &[f32]) -> Result<Vec<i16>, rubato::ResampleError> {
        let resampled = self.process(input)?;
        Ok(f32_to_i16(&resampled))
    }

    /// Finalise the stream: resample a trailing partial chunk (fewer than
    /// `chunk_size` frames) and then drain the resampler's internal delay line.
    ///
    /// `SincFixedIn` buffers `sinc_len` samples of state, so the final output
    /// frames sit inside the resampler after the last full chunk is fed. Without
    /// this drain the tail of every recording — up to that delay — is silently
    /// lost, which on a long recording can clip a final word. Call this exactly
    /// once at end of stream, after all full chunks have gone through
    /// [`process_to_i16`].
    ///
    /// `leftover` is the remaining interleaved source samples that did not fill a
    /// whole `chunk_size` chunk (may be empty).
    pub fn finish_to_i16(&mut self, leftover: &[f32]) -> Result<Vec<i16>, rubato::ResampleError> {
        let mono = self.to_mono(leftover);
        // `process_partial` emits the resampler's buffered delay-line tail. With a
        // short final block it also resamples that block; with `None` it drains
        // delay only. We must pass `None` (not `Some(&[])`) when there is no
        // leftover: rubato's partial path clears its internal zero-padding for an
        // empty channel, which then fails the fixed-input-size check. An empty
        // leftover is common — the accumulator empties whenever a full chunk is
        // drained, so a recording can stop on an exact chunk boundary.
        let waves_out = if mono.is_empty() {
            self.resampler.process_partial::<Vec<f32>>(None, None)?
        } else {
            let waves_in = [mono];
            self.resampler.process_partial(Some(&waves_in), None)?
        };
        let resampled = waves_out.into_iter().next().unwrap_or_default();
        Ok(f32_to_i16(&resampled))
    }

    /// Downmix interleaved source frames to mono.
    fn to_mono(&self, input: &[f32]) -> Vec<f32> {
        if self.source_channels == 2 {
            stereo_to_mono(input)
        } else {
            input.to_vec()
        }
    }
}

/// Convert f32 samples to i16 with proper scaling
pub fn f32_to_i16(samples: &[f32]) -> Vec<i16> {
    samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect()
}

/// Convert i16 samples to f32 with proper scaling
pub fn i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32 / 32768.0).collect()
}

/// Mix stereo to mono
pub fn stereo_to_mono(samples: &[f32]) -> Vec<f32> {
    samples
        .chunks(2)
        .map(|c| {
            if c.len() == 2 {
                (c[0] + c[1]) / 2.0
            } else {
                c[0]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_converter_new() {
        let converter = AudioConverter::new(48000, 16000, 2, 1024);
        assert!(converter.is_ok());
    }

    #[test]
    fn test_finish_to_i16_empty_leftover_does_not_error() {
        // A recording that stops on an exact chunk boundary leaves an empty
        // leftover. finish_to_i16 must drain the resampler delay via `None`, not
        // pass an empty `Some` (which rubato rejects with InsufficientInputBufferSize).
        let mut converter = AudioConverter::new(48000, 16000, 1, 1024).unwrap();
        // Prime the resampler with a full chunk so it has delay-line state to drain.
        let chunk: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        converter.process_to_i16(&chunk).unwrap();
        // The critical call: empty leftover must succeed, not error.
        let tail = converter
            .finish_to_i16(&[])
            .expect("empty-leftover finalise must not error");
        // It should emit the buffered delay tail (non-empty), proving the drain ran.
        assert!(
            !tail.is_empty(),
            "delay-line drain should emit the buffered tail"
        );
    }

    #[test]
    fn test_finish_to_i16_partial_leftover() {
        // A short trailing block (fewer than chunk_size frames) must resample and
        // emit the tail without error.
        let mut converter = AudioConverter::new(48000, 16000, 1, 1024).unwrap();
        let chunk: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        converter.process_to_i16(&chunk).unwrap();
        let leftover: Vec<f32> = (0..300).map(|i| (i as f32 * 0.02).sin() * 0.3).collect();
        let tail = converter
            .finish_to_i16(&leftover)
            .expect("partial-leftover finalise must not error");
        assert!(!tail.is_empty());
    }

    #[test]
    fn test_stereo_to_mono() {
        let stereo = vec![0.5, -0.5, 0.3, -0.3, 0.1, -0.1];
        let mono = stereo_to_mono(&stereo);
        assert_eq!(mono.len(), 3);
        assert!((mono[0] - 0.0).abs() < 0.0001); // (0.5 + -0.5) / 2
        assert!((mono[1] - 0.0).abs() < 0.0001); // (0.3 + -0.3) / 2
        assert!((mono[2] - 0.0).abs() < 0.0001); // (0.1 + -0.1) / 2
    }

    #[test]
    fn test_f32_to_i16() {
        let f32_samples = vec![1.0, 0.5, 0.0, -0.5, -1.0];
        let i16_samples = f32_to_i16(&f32_samples);

        assert_eq!(i16_samples[0], 32767); // 1.0 -> max positive
        assert_eq!(i16_samples[1], 16383); // 0.5 -> half max
        assert_eq!(i16_samples[2], 0); // 0.0 -> zero
        assert_eq!(i16_samples[3], -16383); // -0.5 -> half min
        assert_eq!(i16_samples[4], -32767); // -1.0 -> max negative
    }

    #[test]
    fn test_i16_to_f32() {
        let i16_samples = vec![32767i16, 0, -32768];
        let f32_samples = i16_to_f32(&i16_samples);

        assert!((f32_samples[0] - 1.0).abs() < 0.0001);
        assert!(f32_samples[1].abs() < 0.0001);
        assert!((f32_samples[2] - (-1.0)).abs() < 0.0001);
    }

    #[test]
    fn test_resampling_produces_output() {
        let mut converter = AudioConverter::new(48000, 16000, 1, 1024).unwrap();

        // Process multiple chunks to account for rubato's internal buffering
        let chunk: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.001).sin()).collect();
        let mut total_output = 0;

        for _ in 0..10 {
            let output = converter.process(&chunk).unwrap();
            total_output += output.len();
        }

        // After 10 chunks of 1024 samples at 48kHz, we should have produced
        // approximately 10240 / 3 ≈ 3413 samples at 16kHz
        // Allow wide tolerance due to rubato's internal buffering
        assert!(
            total_output > 2500,
            "Expected > 2500 samples, got {}",
            total_output
        );
    }

    #[test]
    fn test_process_to_i16() {
        let mut converter = AudioConverter::new(48000, 16000, 1, 1024).unwrap();

        // Generate a simple test signal
        let input: Vec<f32> = (0..3072).map(|i| (i as f32 * 0.01).sin() * 0.5).collect();

        // The conversion must succeed; the `unwrap` is the assertion. rubato may
        // legitimately buffer and return an empty chunk, so output length is not
        // asserted (the previous `!is_empty() || true` was a tautology).
        let _output = converter.process_to_i16(&input).unwrap();
    }

    #[test]
    fn test_stereo_input() {
        let mut converter = AudioConverter::new(48000, 16000, 2, 512).unwrap();

        // Stereo input: 1024 samples = 512 frames
        let input: Vec<f32> = (0..1024).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();

        let output = converter.process(&input).unwrap();

        // Should produce some output
        assert!(!output.is_empty());
    }
}
