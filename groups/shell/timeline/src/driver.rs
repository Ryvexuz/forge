//! Driver contracts only; this module does not implement evaluation.

use std::ops::Range;

use crate::time::Time;

/// A failure reported by a track driver.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DriverError {
    /// The requested time range is invalid.
    InvalidRange,
    /// The caller's output buffer cannot hold the requested evaluation.
    BufferTooSmall,
    /// The driver could not evaluate its source data.
    EvaluationFailed,
}

/// An evaluator for one track kind.
///
/// Drivers own their domain-specific configuration. Sample layout and sampling
/// cadence are defined by the concrete driver and agreed with its caller.
pub trait TrackDriver {
    /// Return the kind identifier matched by a track's `kind` field.
    fn kind(&self) -> &str;

    /// Evaluate the half-open tick range into caller-owned scalar storage.
    ///
    /// Implementations must reject reversed ranges with
    /// [`DriverError::InvalidRange`] and insufficient storage with
    /// [`DriverError::BufferTooSmall`]. Other evaluation failures must be
    /// reported as [`DriverError::EvaluationFailed`], not silently ignored.
    /// Validation must finish before the output buffer is modified.
    fn evaluate(&self, range: Range<Time>, output: &mut [f64]) -> Result<(), DriverError>;
}
