//! Synthetic video with exact, half-open source-frame intervals.

use std::ops::Range;

use crate::driver::{DriverError, TrackDriver};
use crate::time::{Rate, Time, TimeError, TICKS_PER_SECOND};

/// An eight-pixel grayscale image, encoding the frame index in little-endian order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    pub pixels: [u8; 8],
}

/// A synthetic clip. All positions and trim offsets are nonnegative ticks.
#[derive(Clone, Debug)]
pub struct VideoDriver {
    rate: Rate,
    start: Time,
    end: Time,
    trim: Time,
}

impl VideoDriver {
    pub fn new(rate: Rate, start: Time, len: Time, trim: Time) -> Result<Self, TimeError> {
        if rate.num == 0 || rate.den == 0 {
            return Err(TimeError::BadRate);
        }
        if start.0 < 0 || len.0 <= 0 || trim.0 < 0 {
            return Err(TimeError::Overflow);
        }
        let end = Time(start.0.checked_add(len.0).ok_or(TimeError::Overflow)?);
        trim.0.checked_add(len.0).ok_or(TimeError::Overflow)?;
        // Validate the greatest source index before accepting the clip.
        let last = i128::from(trim.0 + len.0 - 1) * i128::from(rate.num)
            / (i128::from(TICKS_PER_SECOND) * i128::from(rate.den));
        u64::try_from(last).map_err(|_| TimeError::Overflow)?;
        Ok(Self {
            rate,
            start,
            end,
            trim,
        })
    }

    /// Select floor((trim + time - start) * rate / ticks_per_second).
    /// Thus an exact boundary belongs to the later frame.
    pub fn frame_at(&self, time: Time) -> Result<Frame, DriverError> {
        if time.0 < self.start.0 || time.0 >= self.end.0 {
            return Err(DriverError::InvalidRange);
        }
        let source = i128::from(self.trim.0) + i128::from(time.0 - self.start.0);
        let index = source * i128::from(self.rate.num)
            / (i128::from(TICKS_PER_SECOND) * i128::from(self.rate.den));
        let index = u64::try_from(index).map_err(|_| DriverError::EvaluationFailed)?;
        Ok(Frame {
            pixels: index.to_le_bytes(),
        })
    }
}

impl TrackDriver for VideoDriver {
    fn kind(&self) -> &str {
        "video"
    }

    /// Emit eight grayscale scalars per tick in the requested half-open range.
    /// This tick cadence is independent of the caller's display frame rate.
    fn evaluate(&self, range: Range<Time>, output: &mut [f64]) -> Result<(), DriverError> {
        if range.start.0 < self.start.0 || range.end.0 > self.end.0 || range.start.0 > range.end.0 {
            return Err(DriverError::InvalidRange);
        }
        let count = usize::try_from(range.end.0 - range.start.0)
            .ok()
            .and_then(|count| count.checked_mul(8))
            .ok_or(DriverError::BufferTooSmall)?;
        if output.len() < count {
            return Err(DriverError::BufferTooSmall);
        }
        for (tick, pixels) in (range.start.0..range.end.0).zip(output[..count].as_chunks_mut::<8>().0)
        {
            let frame = self.frame_at(Time(tick))?;
            for (out, pixel) in pixels.iter_mut().zip(frame.pixels) {
                *out = f64::from(pixel);
            }
        }
        Ok(())
    }
}
