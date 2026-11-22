//! Playback follows the absolute audio position, never accumulated video deltas.

use crate::time::{Time, TimeError, TICKS_PER_SECOND};

pub trait AudioClock {
    fn time(&self) -> Result<Time, TimeError>;
}

#[derive(Clone, Copy, Debug)]
pub struct Playhead {
    time: Time,
}

impl Default for Playhead {
    fn default() -> Self {
        Self::new()
    }
}

impl Playhead {
    pub fn new() -> Self {
        Self { time: Time(0) }
    }

    pub fn time(&self) -> Time {
        self.time
    }

    pub fn sync(&mut self, clock: &impl AudioClock) -> Result<Time, TimeError> {
        let time = clock.time()?;
        if time.0 < 0 {
            return Err(TimeError::Overflow);
        }
        self.time = time;
        Ok(time)
    }
}

/// An absolute sample counter; fractional ticks are rounded down only on read.
#[derive(Clone, Debug)]
pub struct FakeAudioClock {
    sample_rate: u32,
    samples: u64,
}

impl FakeAudioClock {
    pub fn new(sample_rate: u32) -> Result<Self, TimeError> {
        if sample_rate == 0 {
            return Err(TimeError::BadRate);
        }
        Ok(Self {
            sample_rate,
            samples: 0,
        })
    }

    pub fn advance(&mut self, samples: u64) -> Result<(), TimeError> {
        let next = self
            .samples
            .checked_add(samples)
            .ok_or(TimeError::Overflow)?;
        self.time_for(next)?;
        self.samples = next;
        Ok(())
    }

    fn time_for(&self, samples: u64) -> Result<Time, TimeError> {
        let ticks = u128::from(samples) * TICKS_PER_SECOND as u128 / u128::from(self.sample_rate);
        i64::try_from(ticks)
            .map(Time)
            .map_err(|_| TimeError::Overflow)
    }
}

impl AudioClock for FakeAudioClock {
    fn time(&self) -> Result<Time, TimeError> {
        self.time_for(self.samples)
    }
}
