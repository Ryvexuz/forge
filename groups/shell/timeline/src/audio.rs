//! Mono synthetic audio. Tick positions round to the nearest sample, with exact
//! halves rounded down (toward negative infinity), for both clip boundaries.
//! Clips are half-open; phase is computed in f64 from the clip-local sample
//! index, never carried between blocks. Fades use min(i / in, (len - 1 - i) / out,
//! 1), omitting a ratio when its fade length is zero. Tracks sum without clipping.

use std::ops::Range;

use crate::driver::{DriverError, TrackDriver};
use crate::time::{Time, TICKS_PER_SECOND};

/// A validated sine source. Frequency is nonnegative and at most Nyquist when
/// attached to a driver; amplitude is in [-1, 1], bounding each summand.
#[derive(Clone, Copy, Debug)]
pub struct Tone {
    frequency: f64,
    amplitude: f64,
}

/// Construct a source, rejecting non-finite or out-of-band parameters.
pub fn tone(freq_hz: f64, amplitude: f64) -> Result<Tone, DriverError> {
    if !freq_hz.is_finite() || freq_hz < 0.0 || !amplitude.is_finite() || amplitude.abs() > 1.0 {
        return Err(DriverError::EvaluationFailed);
    }
    Ok(Tone {
        frequency: freq_hz,
        amplitude,
    })
}

#[derive(Debug)]
struct Clip {
    start: i64,
    end: i64,
    source: Tone,
    fade_in: u64,
    fade_out: u64,
}

/// A mono mixer; clips from any tracks may be added in stable summation order.
#[derive(Debug)]
pub struct AudioDriver {
    sample_rate: u32,
    clips: Vec<Clip>,
}

impl AudioDriver {
    /// The sample rate must be nonzero.
    pub fn new(sample_rate: u32) -> Result<Self, DriverError> {
        if sample_rate == 0 {
            return Err(DriverError::EvaluationFailed);
        }
        Ok(Self {
            sample_rate,
            clips: Vec::new(),
        })
    }

    fn sample(&self, time: Time) -> Result<i64, DriverError> {
        let ticks = i128::from(TICKS_PER_SECOND);
        let numerator = i128::from(time.0) * i128::from(self.sample_rate);
        let floor = numerator.div_euclid(ticks);
        let rounded = floor + i128::from(numerator.rem_euclid(ticks) * 2 > ticks);
        i64::try_from(rounded).map_err(|_| DriverError::InvalidRange)
    }

    /// Add a clip with absolute tick boundaries and fade lengths in samples.
    /// Reversed boundaries, above-Nyquist tones, and fades longer than the
    /// rounded clip are rejected before changing the mixer.
    pub fn add_clip(
        &mut self,
        range: Range<Time>,
        source: Tone,
        fade_in: u64,
        fade_out: u64,
    ) -> Result<(), DriverError> {
        if range.start.0 > range.end.0 {
            return Err(DriverError::InvalidRange);
        }
        let start = self.sample(range.start)?;
        let end = self.sample(range.end)?;
        let length = i128::from(end) - i128::from(start);
        if source.frequency > f64::from(self.sample_rate) / 2.0
            || i128::from(fade_in.max(fade_out)) > length
        {
            return Err(DriverError::EvaluationFailed);
        }
        self.clips.push(Clip {
            start,
            end,
            source,
            fade_in,
            fade_out,
        });
        Ok(())
    }

    /// Render consecutive absolute sample indices. Every value is validated
    /// before any caller storage is changed, including on summation overflow.
    pub fn render(&self, start: i64, output: &mut [f64]) -> Result<(), DriverError> {
        let end = i128::from(start) + output.len() as i128;
        if end > i128::from(i64::MAX) {
            return Err(DriverError::InvalidRange);
        }
        let value_at = |index: i64| {
            let mut sum = 0.0;
            for clip in &self.clips {
                if index < clip.start || index >= clip.end {
                    continue;
                }
                let local = (i128::from(index) - i128::from(clip.start)) as f64;
                let remaining = (i128::from(clip.end) - 1 - i128::from(index)) as f64;
                let mut gain = 1.0_f64;
                if clip.fade_in != 0 {
                    gain = gain.min(local / clip.fade_in as f64);
                }
                if clip.fade_out != 0 {
                    gain = gain.min(remaining / clip.fade_out as f64);
                }
                // Reduce cycles before multiplying by tau to keep phase finite.
                let cycles =
                    (local * (clip.source.frequency / f64::from(self.sample_rate))).rem_euclid(1.0);
                sum += (std::f64::consts::TAU * cycles).sin() * clip.source.amplitude * gain;
            }
            sum
        };
        for offset in 0..output.len() {
            if !value_at((i128::from(start) + offset as i128) as i64).is_finite() {
                return Err(DriverError::EvaluationFailed);
            }
        }
        for (offset, value) in output.iter_mut().enumerate() {
            *value = value_at((i128::from(start) + offset as i128) as i64);
        }
        Ok(())
    }
}

impl TrackDriver for AudioDriver {
    fn kind(&self) -> &str {
        "audio"
    }

    fn evaluate(&self, range: Range<Time>, output: &mut [f64]) -> Result<(), DriverError> {
        if range.start.0 > range.end.0 {
            return Err(DriverError::InvalidRange);
        }
        let start = self.sample(range.start)?;
        let end = self.sample(range.end)?;
        let length = usize::try_from(i128::from(end) - i128::from(start))
            .map_err(|_| DriverError::InvalidRange)?;
        if output.len() < length {
            return Err(DriverError::BufferTooSmall);
        }
        self.render(start, &mut output[..length])
    }
}
