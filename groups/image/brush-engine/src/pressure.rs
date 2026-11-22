//! Fixed pressure response and projected-disc tilt geometry.

/// Size in pixels and opacity in the unit interval.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PressureResponse {
    pub size: f32,
    pub opacity: f32,
}

/// Design knots at pressure 0, 0.5 and 1, interpolated linearly.
/// These are brush design values, not configurable resource limits.
pub const PRESSURE_TABLE: [PressureResponse; 3] = [
    PressureResponse {
        size: 1.0,
        opacity: 0.0,
    },
    PressureResponse {
        size: 32.5,
        opacity: 0.5,
    },
    PressureResponse {
        size: 64.0,
        opacity: 1.0,
    },
];

/// Invalid dynamics inputs are rejected rather than clamped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicsError {
    InvalidPressure,
    InvalidTilt,
    InvalidRotation,
}

impl std::fmt::Display for DynamicsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidPressure => "pressure must be finite and in 0..=1",
            Self::InvalidTilt => "tilt vector must be finite with magnitude at most 90 degrees",
            Self::InvalidRotation => "rotation must be finite and in 0..=359 degrees",
        })
    }
}

impl std::error::Error for DynamicsError {}

/// The fixed piecewise-linear pressure curve.
#[derive(Clone, Copy, Debug, Default)]
pub struct PressureCurve;

impl PressureCurve {
    pub fn sample(self, pressure: f32) -> Result<PressureResponse, DynamicsError> {
        if !pressure.is_finite() || !(0.0..=1.0).contains(&pressure) {
            return Err(DynamicsError::InvalidPressure);
        }
        let (index, fraction) = if pressure <= 0.5 {
            (0, f64::from(pressure) * 2.0)
        } else {
            (1, (f64::from(pressure) - 0.5) * 2.0)
        };
        let a = PRESSURE_TABLE[index];
        let b = PRESSURE_TABLE[index + 1];
        let interpolate =
            |a: f32, b: f32| (f64::from(a) + (f64::from(b) - f64::from(a)) * fraction) as f32;
        Ok(PressureResponse {
            size: interpolate(a.size, b.size),
            opacity: interpolate(a.opacity, b.opacity),
        })
    }
}

/// Unit-major-axis ellipse. Multiply both axes by the pressure-mapped size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TiltEllipse {
    pub major_axis: f32,
    pub minor_axis: f32,
    /// Long-axis direction in degrees, normalized to [0, 360).
    pub angle_degrees: f32,
}

impl TiltEllipse {
    /// Tilt components form a vector in degrees: its magnitude is the disc's
    /// inclination and its direction is the projected long-axis direction.
    /// Inclinations through 90 degrees are supported, including an edge-on disc.
    /// Rotation is the decoded pen rotation in 0..=359 degrees.
    pub fn from_degrees(tilt_x: f32, tilt_y: f32, rotation: f32) -> Result<Self, DynamicsError> {
        let x = f64::from(tilt_x);
        let y = f64::from(tilt_y);
        let inclination = x.hypot(y);
        if !inclination.is_finite() || inclination > 90.0 {
            return Err(DynamicsError::InvalidTilt);
        }
        if !rotation.is_finite() || !(0.0..=359.0).contains(&rotation) {
            return Err(DynamicsError::InvalidRotation);
        }
        let direction = if inclination == 0.0 {
            0.0
        } else {
            y.atan2(x).to_degrees()
        };
        let angle = (direction + f64::from(rotation)).rem_euclid(360.0) as f32;
        Ok(Self {
            major_axis: 1.0,
            minor_axis: if inclination == 90.0 {
                0.0
            } else {
                inclination.to_radians().cos() as f32
            },
            // Rounding to f32 can round a direction just below 360 up to 360.
            angle_degrees: if angle == 360.0 { 0.0 } else { angle },
        })
    }
}
