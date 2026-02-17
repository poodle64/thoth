//! Audio metering for real-time level visualisation
//!
//! Provides RMS and peak level calculation for UI feedback.

use serde::Serialize;

/// Audio level data emitted to frontend
#[derive(Debug, Clone, Serialize)]
pub struct AudioLevel {
    /// RMS (root mean square) level, normalised 0.0-1.0
    pub rms: f32,
    /// Peak level with decay, normalised 0.0-1.0
    pub peak: f32,
    /// dB level (for display), typically -60 to 0
    pub db: f32,
}

/// Real-time audio meter
pub struct AudioMeter {
    peak: f32,
    decay_rate: f32,
    min_db: f32,
}

impl Default for AudioMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioMeter {
    /// Create a new audio meter
    ///
    /// Default decay rate gives ~300ms peak hold at 30Hz updates
    pub fn new() -> Self {
        Self {
            peak: 0.0,
            decay_rate: 0.95,
            min_db: -60.0,
        }
    }

    /// Create a meter with custom decay rate
    ///
    /// `decay_rate` should be between 0.0 and 1.0
    /// Higher values = slower decay
    pub fn with_decay(decay_rate: f32) -> Self {
        Self {
            peak: 0.0,
            decay_rate: decay_rate.clamp(0.0, 0.999),
            min_db: -60.0,
        }
    }

    /// Process audio samples and return levels
    pub fn process(&mut self, samples: &[f32]) -> AudioLevel {
        if samples.is_empty() {
            return AudioLevel {
                rms: 0.0,
                peak: self.peak * self.decay_rate,
                db: self.min_db,
            };
        }

        // Calculate RMS
        let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
        let rms = (sum_sq / samples.len() as f32).sqrt();

        // Find peak in this buffer
        let sample_peak = samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max);

        // Update peak with decay
        self.peak = if sample_peak > self.peak {
            sample_peak
        } else {
            self.peak * self.decay_rate
        };

        // Calculate dB (with floor)
        let db = if rms > 0.0 {
            (20.0 * rms.log10()).max(self.min_db)
        } else {
            self.min_db
        };

        AudioLevel {
            rms: rms.min(1.0),
            peak: self.peak.min(1.0),
            db,
        }
    }

    /// Reset the meter
    pub fn reset(&mut self) {
        self.peak = 0.0;
    }
}

/// Calculate RMS level for a buffer of samples
pub fn calculate_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Calculate peak level for a buffer of samples
pub fn calculate_peak(samples: &[f32]) -> f32 {
    samples.iter().map(|s| s.abs()).fold(0.0f32, f32::max)
}

/// Convert linear amplitude to decibels
pub fn amplitude_to_db(amplitude: f32) -> f32 {
    if amplitude > 0.0 {
        20.0 * amplitude.log10()
    } else {
        -f32::INFINITY
    }
}

/// Convert decibels to linear amplitude
pub fn db_to_amplitude(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meter_new() {
        let meter = AudioMeter::new();
        assert_eq!(meter.peak, 0.0);
    }

    #[test]
    fn test_process_silence() {
        let mut meter = AudioMeter::new();
        let samples = vec![0.0f32; 1024];
        let level = meter.process(&samples);

        assert_eq!(level.rms, 0.0);
        assert_eq!(level.peak, 0.0);
        assert_eq!(level.db, -60.0);
    }

    #[test]
    fn test_process_full_scale() {
        let mut meter = AudioMeter::new();
        let samples = vec![1.0f32; 1024];
        let level = meter.process(&samples);

        assert!((level.rms - 1.0).abs() < 0.001);
        assert!((level.peak - 1.0).abs() < 0.001);
        assert!((level.db - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_process_sine_wave() {
        let mut meter = AudioMeter::new();
        // Generate a 1kHz sine wave at unit amplitude
        let samples: Vec<f32> = (0..1024)
            .map(|i| (2.0 * std::f32::consts::PI * i as f32 / 1024.0 * 10.0).sin())
            .collect();

        let level = meter.process(&samples);

        // RMS of a sine wave is amplitude / sqrt(2) ≈ 0.707
        assert!((level.rms - 0.707).abs() < 0.1, "RMS should be ~0.707");
        assert!((level.peak - 1.0).abs() < 0.1, "Peak should be ~1.0");
    }

    #[test]
    fn test_peak_decay() {
        let mut meter = AudioMeter::with_decay(0.9);

        // Process a loud signal
        let loud = vec![0.8f32; 512];
        meter.process(&loud);

        // Then silence
        let silence = vec![0.0f32; 512];
        let level1 = meter.process(&silence);
        let level2 = meter.process(&silence);
        let level3 = meter.process(&silence);

        // Peak should decay
        assert!(level1.peak > level2.peak);
        assert!(level2.peak > level3.peak);
    }

    #[test]
    fn test_calculate_rms() {
        let samples = vec![0.5f32; 100];
        let rms = calculate_rms(&samples);
        assert!((rms - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_calculate_peak() {
        let samples = vec![0.1, 0.5, 0.3, 0.8, 0.2];
        let peak = calculate_peak(&samples);
        assert!((peak - 0.8).abs() < 0.001);
    }

    #[test]
    fn test_amplitude_to_db() {
        assert!((amplitude_to_db(1.0) - 0.0).abs() < 0.001);
        assert!((amplitude_to_db(0.5) - (-6.02)).abs() < 0.1);
        assert!((amplitude_to_db(0.1) - (-20.0)).abs() < 0.1);
    }

    #[test]
    fn test_db_to_amplitude() {
        assert!((db_to_amplitude(0.0) - 1.0).abs() < 0.001);
        assert!((db_to_amplitude(-6.02) - 0.5).abs() < 0.01);
        assert!((db_to_amplitude(-20.0) - 0.1).abs() < 0.01);
    }

    #[test]
    fn test_reset() {
        let mut meter = AudioMeter::new();

        let loud = vec![0.9f32; 512];
        meter.process(&loud);
        assert!(meter.peak > 0.8);

        meter.reset();
        assert_eq!(meter.peak, 0.0);
    }
}
