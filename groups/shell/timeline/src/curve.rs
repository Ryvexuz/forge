//! Scalar curves sampled at an `f64` position in ticks, clamped at the endpoints.
//!
//! The left key selects interpolation. Bezier handle x values are time offsets
//! from their key; handle y values follow CSS `cubic-bezier`: normalized
//! progress between the two key values (control value = a + y*(b-a)). Hermite handles describe tangent vectors: outgoing on the left,
//! incoming on the right; their value/time slopes are multiplied by the segment
//! duration, as in glTF CUBICSPLINE. Missing cubic handles are errors.

use crate::track::{Interp, Keyframe};

/// Invalid curve data or a sample that cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveError {
    Empty,
    NonFiniteValue,
    DuplicateTime,
    OutOfOrder,
    MissingHandles,
    InvalidHandle,
    NonFinitePosition,
    NumericRange,
}

/// Validated, immutable keyframes.
#[derive(Clone, Debug)]
pub struct Curve {
    keys: Vec<Keyframe>,
}

impl Curve {
    /// Validate all keys before exposing an evaluator.
    ///
    /// Tick coordinates must remain distinct after conversion to `f64`.
    /// Cubic control values and duration-scaled tangents must be finite.
    pub fn new(keys: Vec<Keyframe>) -> Result<Self, CurveError> {
        if keys.is_empty() {
            return Err(CurveError::Empty);
        }
        for key in &keys {
            if !key.value.is_finite() {
                return Err(CurveError::NonFiniteValue);
            }
            if let Some(handles) = key.handles {
                if handles
                    .iter()
                    .any(|(x, y)| !x.is_finite() || !y.is_finite())
                {
                    return Err(CurveError::InvalidHandle);
                }
            }
        }
        for pair in keys.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if a.time.0 == b.time.0 {
                return Err(CurveError::DuplicateTime);
            }
            if a.time.0 > b.time.0 {
                return Err(CurveError::OutOfOrder);
            }
            let duration = b.time.0 as f64 - a.time.0 as f64;
            if duration <= 0.0 {
                return Err(CurveError::NumericRange);
            }
            match a.interp {
                Interp::Bezier | Interp::Hermite => {
                    let outgoing = a.handles.ok_or(CurveError::MissingHandles)?[1];
                    let incoming = b.handles.ok_or(CurveError::MissingHandles)?[0];
                    if a.interp == Interp::Bezier {
                        if !(0.0..=duration).contains(&outgoing.0)
                            || !(-duration..=0.0).contains(&incoming.0)
                        {
                            return Err(CurveError::InvalidHandle);
                        }
                        let span = b.value - a.value;
                        if !span.is_finite()
                            || !(a.value + outgoing.1 * span).is_finite()
                            || !(a.value + incoming.1 * span).is_finite()
                        {
                            return Err(CurveError::NumericRange);
                        }
                    } else {
                        for (x, y) in [outgoing, incoming] {
                            if x == 0.0 {
                                return Err(CurveError::InvalidHandle);
                            }
                            if !((y / x) * duration).is_finite() {
                                return Err(CurveError::NumericRange);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(Self { keys })
    }

    /// Sample without extrapolation. Non-finite positions are refused.
    pub fn evaluate(&self, position: f64) -> Result<f64, CurveError> {
        if !position.is_finite() {
            return Err(CurveError::NonFinitePosition);
        }
        let first = &self.keys[0];
        let last = &self.keys[self.keys.len() - 1];
        if position <= first.time.0 as f64 {
            return Ok(first.value);
        }
        if position >= last.time.0 as f64 {
            return Ok(last.value);
        }
        let index = self
            .keys
            .partition_point(|key| key.time.0 as f64 <= position);
        let (a, b) = (&self.keys[index - 1], &self.keys[index]);
        let duration = b.time.0 as f64 - a.time.0 as f64;
        let t = (position - a.time.0 as f64) / duration;
        let value = match a.interp {
            Interp::Step => a.value,
            Interp::Linear => lerp(a.value, b.value, t),
            Interp::Bezier => {
                let outgoing = a.handles.ok_or(CurveError::MissingHandles)?[1];
                let incoming = b.handles.ok_or(CurveError::MissingHandles)?[0];
                let x1 = outgoing.0 / duration;
                let x2 = 1.0 + incoming.0 / duration;
                let (mut low, mut high) = (0.0, 1.0);
                // A fixed number exceeds the binary64 significand precision.
                for _ in 0..64 {
                    let mid = low + (high - low) * 0.5;
                    if cubic(0.0, x1, x2, 1.0, mid) < t {
                        low = mid;
                    } else {
                        high = mid;
                    }
                }
                let span = b.value - a.value;
                cubic(
                    a.value,
                    a.value + outgoing.1 * span,
                    a.value + incoming.1 * span,
                    b.value,
                    low + (high - low) * 0.5,
                )
            }
            Interp::Hermite => {
                let outgoing = a.handles.ok_or(CurveError::MissingHandles)?[1];
                let incoming = b.handles.ok_or(CurveError::MissingHandles)?[0];
                let m0 = outgoing.1 / outgoing.0 * duration;
                let m1 = incoming.1 / incoming.0 * duration;
                let u = 1.0 - t;
                // Endpoint blend plus the two glTF duration-scaled tangents.
                lerp(a.value, b.value, t * t * (3.0 - 2.0 * t)) + (u * u * t) * m0
                    - (u * t * t) * m1
            }
        };
        if value.is_finite() {
            Ok(value)
        } else {
            Err(CurveError::NumericRange)
        }
    }
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    (1.0 - t) * a + t * b
}

fn cubic(a: f64, b: f64, c: f64, d: f64, t: f64) -> f64 {
    lerp(
        lerp(lerp(a, b, t), lerp(b, c, t), t),
        lerp(lerp(b, c, t), lerp(c, d, t), t),
        t,
    )
}
