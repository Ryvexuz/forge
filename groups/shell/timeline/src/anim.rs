//! Clip-local animation sampling; no scene objects are accessed.

use std::ops::Range;

use crate::curve::Curve;
use crate::driver::{DriverError, TrackDriver};
use crate::time::Time;
use crate::track::{Channel, Clip};

/// Receives evaluated scalar properties in channel order.
pub trait AnimationSink {
    fn write(&mut self, path: &str, value: f64);
}

/// A validated clip and its ordered target curves.
pub struct AnimationDriver {
    clip: Clip,
    end: Time,
    channels: Vec<(String, Curve)>,
}

impl AnimationDriver {
    /// Validate the clip interval, source interval, targets and curves.
    pub fn new(clip: Clip, channels: Vec<Channel>) -> Result<Self, DriverError> {
        if clip.len.0 <= 0 {
            return Err(DriverError::InvalidRange);
        }
        let end = clip
            .start
            .checked_add(clip.len)
            .map_err(|_| DriverError::InvalidRange)?;
        clip.source_offset
            .checked_add(clip.len)
            .map_err(|_| DriverError::InvalidRange)?;
        let channels = channels
            .into_iter()
            .map(|channel| {
                if channel.target.is_empty() || channel.target.chars().any(char::is_control) {
                    return Err(DriverError::EvaluationFailed);
                }
                let curve = Curve::new(channel.keys).map_err(|_| DriverError::EvaluationFailed)?;
                Ok((channel.target, curve))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            clip,
            end,
            channels,
        })
    }

    /// Sample at the range start, then publish only after all channels succeed.
    pub fn write_to(
        &self,
        range: Range<Time>,
        sink: &mut impl AnimationSink,
    ) -> Result<(), DriverError> {
        let mut values = vec![0.0; self.channels.len()];
        self.evaluate(range, &mut values)?;
        for ((path, _), value) in self.channels.iter().zip(values) {
            sink.write(path, value);
        }
        Ok(())
    }
}

impl TrackDriver for AnimationDriver {
    fn kind(&self) -> &str {
        "animation"
    }

    /// One scalar per channel at `range.start`; the range must lie in the clip.
    /// Extra output slots are untouched, as is all output on any error.
    fn evaluate(&self, range: Range<Time>, output: &mut [f64]) -> Result<(), DriverError> {
        if range.start.0 >= range.end.0
            || range.start.0 < self.clip.start.0
            || range.end.0 > self.end.0
        {
            return Err(DriverError::InvalidRange);
        }
        if output.len() < self.channels.len() {
            return Err(DriverError::BufferTooSmall);
        }
        let position = range
            .start
            .checked_sub(self.clip.start)
            .and_then(|time| time.checked_add(self.clip.source_offset))
            .map_err(|_| DriverError::InvalidRange)?;
        let values = self
            .channels
            .iter()
            .map(|(_, curve)| {
                curve
                    .evaluate(position.0 as f64)
                    .map_err(|_| DriverError::EvaluationFailed)
            })
            .collect::<Result<Vec<_>, _>>()?;
        output[..values.len()].copy_from_slice(&values);
        Ok(())
    }
}
