//! Checked integer time arithmetic and exact rational-rate conversions.

/// The number of timeline ticks in one second.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// A signed position or duration in timeline ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Time(pub i64);

/// A rational rate. Conversions validate even directly constructed values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rate {
    /// Numerator of the rate.
    pub num: u32,
    /// Denominator of the rate.
    pub den: u32,
}

/// Errors reserved for time conversion and rate validation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimeError {
    /// A time calculation cannot be represented.
    Overflow,
    /// A rate is invalid.
    BadRate,
}

impl Rate {
    /// Constructs a positive rate with an integral number of ticks per unit.
    ///
    /// Rates finer than the tick resolution, or not exactly representable in
    /// ticks, are rejected rather than rounded. Fractions need not be reduced.
    pub fn new(num: i64, den: i64) -> Result<Self, TimeError> {
        let rate = Self {
            num: u32::try_from(num).map_err(|_| TimeError::BadRate)?,
            den: u32::try_from(den).map_err(|_| TimeError::BadRate)?,
        };
        rate.ticks_per_unit()?;
        Ok(rate)
    }

    fn ticks_per_unit(self) -> Result<i64, TimeError> {
        if self.num == 0 || self.den == 0 {
            return Err(TimeError::BadRate);
        }
        // i64 ticks * u32 denominator fits in i128, even for public literals.
        let ticks = i128::from(TICKS_PER_SECOND) * i128::from(self.den);
        let num = i128::from(self.num);
        if ticks % num != 0 {
            return Err(TimeError::BadRate);
        }
        i64::try_from(ticks / num).map_err(|_| TimeError::BadRate)
    }
}

impl Time {
    /// Adds ticks without wrapping.
    pub fn checked_add(self, other: Self) -> Result<Self, TimeError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Subtracts ticks without wrapping.
    pub fn checked_sub(self, other: Self) -> Result<Self, TimeError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Multiplies ticks by an integer without wrapping.
    pub fn checked_mul(self, factor: i64) -> Result<Self, TimeError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or(TimeError::Overflow)
    }

    /// Converts a signed frame index into exact ticks.
    pub fn from_frames(frames: i64, rate: Rate) -> Result<Self, TimeError> {
        Self(rate.ticks_per_unit()?).checked_mul(frames)
    }

    /// Converts ticks to a frame index, discarding partial frames toward zero.
    pub fn to_frames(self, rate: Rate) -> Result<i64, TimeError> {
        Ok(self.0 / rate.ticks_per_unit()?)
    }

    /// Converts a signed sample index into exact ticks.
    pub fn from_samples(samples: i64, sample_rate: i64) -> Result<Self, TimeError> {
        Self::from_frames(samples, Rate::new(sample_rate, 1)?)
    }

    /// Converts ticks to a sample index, discarding partial samples toward zero.
    pub fn to_samples(self, sample_rate: i64) -> Result<i64, TimeError> {
        self.to_frames(Rate::new(sample_rate, 1)?)
    }
}
