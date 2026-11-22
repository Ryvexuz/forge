//! Tick-based time and rational-rate data; no conversion methods yet.

/// The number of timeline ticks in one second.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;

/// A signed position or duration in timeline ticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Time(pub i64);

/// A rational rate, stored without validation.
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
