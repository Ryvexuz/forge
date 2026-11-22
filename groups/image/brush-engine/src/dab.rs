//! Round dabs sampled at integer pixel coordinates, with straight-alpha source-over.

/// Invalid parameters are rejected before any destination pixels are changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DabError {
    InvalidDiameter,
    InvalidPressure,
    InvalidHardness,
    InvalidOpacity,
    InvalidCenter,
    InvalidBuffer,
}

#[derive(Debug, Clone, Copy)]
pub struct RoundDab {
    diameter: f64,
    pressure: f64,
    hardness: f64,
    opacity: f64,
}

impl RoundDab {
    /// Diameter must be positive and finite. The other parameters are finite
    /// fractions in [0, 1]. Pressure scales diameter linearly.
    pub fn new(
        diameter: f64,
        pressure: f64,
        hardness: f64,
        opacity: f64,
    ) -> Result<Self, DabError> {
        if !diameter.is_finite() || diameter <= 0.0 {
            return Err(DabError::InvalidDiameter);
        }
        for (value, error) in [
            (pressure, DabError::InvalidPressure),
            (hardness, DabError::InvalidHardness),
            (opacity, DabError::InvalidOpacity),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(error);
            }
        }
        Ok(Self {
            diameter,
            pressure,
            hardness,
            opacity,
        })
    }

    /// Stamp RGB ink into tightly packed RGBA8. Hardness is the fraction of
    /// radius with full coverage; the remainder falls linearly to zero.
    /// The radius boundary is excluded, including at hardness 1.
    pub fn stamp(
        &self,
        pixels: &mut [u8],
        width: usize,
        height: usize,
        center: [f64; 2],
        color: [u8; 3],
    ) -> Result<(), DabError> {
        if width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(pixels.len()) {
            return Err(DabError::InvalidBuffer);
        }
        if !center.iter().all(|v| v.is_finite()) {
            return Err(DabError::InvalidCenter);
        }
        // Multiplication only by fractions cannot overflow. Underflow denotes
        // a dab smaller than the representable radius, which has no coverage.
        let radius = (self.diameter * self.pressure) * 0.5;
        if radius == 0.0 || self.opacity == 0.0 {
            return Ok(());
        }
        for (i, pixel) in pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let dx = i % width;
            let dy = i / width;
            let distance = (dx as f64 - center[0]).hypot(dy as f64 - center[1]);
            if distance >= radius {
                continue;
            }
            let normalized = distance / radius;
            let coverage = if normalized <= self.hardness {
                1.0
            } else {
                (1.0 - normalized) / (1.0 - self.hardness)
            };
            let source_alpha = coverage * self.opacity;
            let retained_alpha = f64::from(pixel[3]) / 255.0 * (1.0 - source_alpha);
            let alpha = source_alpha + retained_alpha;
            if alpha > 0.0 {
                for channel in 0..3 {
                    pixel[channel] = ((f64::from(color[channel]) * source_alpha
                        + f64::from(pixel[channel]) * retained_alpha)
                        / alpha)
                        .round() as u8;
                }
                pixel[3] = (alpha * 255.0).round() as u8;
            }
        }
        Ok(())
    }
}
