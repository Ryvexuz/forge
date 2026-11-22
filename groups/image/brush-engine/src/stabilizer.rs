//! Causal, endpoint-evaluated linear smoothing with an exact pen-up flush.

use std::collections::VecDeque;
use std::fmt;

/// Design default, in pen reports (not a time duration).
pub const DEFAULT_WINDOW: usize = 16;

/// Coordinate band leaves ample headroom for the regression's f64 arithmetic.
/// All finite f32 coordinates, including subnormals, are representable here.
pub const MAX_COORDINATE: f64 = f32::MAX as f64;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StabilizerError {
    NonFiniteCoordinate,
    CoordinateOutOfRange,
    NonFiniteOutput,
}

impl fmt::Display for StabilizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NonFiniteCoordinate => "stabilizer coordinates must be finite",
            Self::CoordinateOutOfRange => "stabilizer coordinate exceeds the supported band",
            Self::NonFiniteOutput => "stabilizer output must be finite",
        })
    }
}

impl std::error::Error for StabilizerError {}

/// Fits a line to the most recent reports and evaluates it at the newest report.
/// Unlike a box average, this preserves constant-speed motion without window lag.
/// Reports are assumed equally spaced in time. Pen-up emits the raw endpoint and
/// resets the window, so separate strokes cannot influence each other.
#[derive(Clone, Debug, Default)]
pub struct Stabilizer {
    samples: VecDeque<Point>,
}

impl Stabilizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rejects invalid input before changing the window.
    pub fn push(&mut self, point: Point) -> Result<Point, StabilizerError> {
        if !point.x.is_finite() || !point.y.is_finite() {
            return Err(StabilizerError::NonFiniteCoordinate);
        }
        if point.x.abs() > MAX_COORDINATE || point.y.abs() > MAX_COORDINATE {
            return Err(StabilizerError::CoordinateOutOfRange);
        }

        // Stage the bounded window so even an output failure is transactional.
        let mut samples = self.samples.clone();
        if samples.len() == DEFAULT_WINDOW {
            samples.pop_front();
        }
        samples.push_back(point);
        let n = samples.len() as f64;
        let center = (n - 1.0) / 2.0;
        let variance = n * (n * n - 1.0) / 12.0;
        let mut output = Point { x: 0.0, y: 0.0 };
        for (i, sample) in samples.iter().enumerate() {
            let weight = if samples.len() == 1 {
                1.0
            } else {
                1.0 / n + center * (i as f64 - center) / variance
            };
            output.x += weight * sample.x;
            output.y += weight * sample.y;
        }
        if !output.x.is_finite() || !output.y.is_finite() {
            return Err(StabilizerError::NonFiniteOutput);
        }
        self.samples = samples;
        Ok(output)
    }

    /// Emits the last accepted input exactly, even when the fit misses a corner.
    /// Empty or repeated pen-up calls emit nothing.
    pub fn pen_up(&mut self) -> Option<Point> {
        let endpoint = self.samples.back().copied();
        self.samples.clear();
        endpoint
    }
}
