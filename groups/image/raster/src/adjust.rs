//! Pure adjustments of straight RGB, with unchanged alpha and premultiplied output.

use crate::tile::{Tile, TileError};
use std::fmt;

/// Parameters are validated by [`Adjustment::apply`], including direct enum construction.
#[derive(Clone, Debug, PartialEq)]
pub enum Adjustment {
    /// `clamp((channel - 0.5) * contrast + 0.5 + brightness)`.
    /// Brightness must be finite; contrast must be finite and nonnegative.
    BrightnessContrast { brightness: f32, contrast: f32 },
    /// Samples at `i / 255`, linearly interpolated. Entries must be in `[0, 1]`.
    Curve { table: Box<[f32; 256]> },
    /// HSV hue rotation in degrees and saturation multiplier (clamped to one).
    /// Hue must be finite; saturation must be finite and nonnegative.
    HueSaturation { hue: f32, saturation: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdjustmentError {
    InvalidBrightness,
    InvalidContrast,
    InvalidCurveEntry { index: usize },
    InvalidHue,
    InvalidSaturation,
    InvalidOutput(TileError),
}

impl fmt::Display for AdjustmentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "adjustment failed: {self:?}")
    }
}

impl std::error::Error for AdjustmentError {}

impl Adjustment {
    /// Returns a new tile, never modifying the source.
    ///
    /// All finite `f32` brightness and hue values are accepted; contrast and
    /// saturation accept all nonnegative finite `f32` values. Arithmetic uses
    /// `f64` so products and unpremultiplication cannot overflow in this domain.
    pub fn apply(&self, source: &Tile) -> Result<Tile, AdjustmentError> {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                if !brightness.is_finite() {
                    return Err(AdjustmentError::InvalidBrightness);
                }
                if !contrast.is_finite() || *contrast < 0.0 {
                    return Err(AdjustmentError::InvalidContrast);
                }
            }
            Self::Curve { table } => {
                for (index, value) in table.iter().enumerate() {
                    if !value.is_finite() || !(0.0..=1.0).contains(value) {
                        return Err(AdjustmentError::InvalidCurveEntry { index });
                    }
                }
            }
            Self::HueSaturation { hue, saturation } => {
                if !hue.is_finite() {
                    return Err(AdjustmentError::InvalidHue);
                }
                if !saturation.is_finite() || *saturation < 0.0 {
                    return Err(AdjustmentError::InvalidSaturation);
                }
            }
        }
        let pixels = source
            .pixels()
            .iter()
            .map(|&pixel| {
                let alpha = f64::from(pixel[3]);
                if alpha == 0.0 {
                    return pixel;
                }
                let rgb = [0, 1, 2].map(|i| f64::from(pixel[i]) / alpha);
                let adjusted = match self {
                    Self::BrightnessContrast {
                        brightness,
                        contrast,
                    } => rgb.map(|c| {
                        ((c - 0.5) * f64::from(*contrast) + 0.5 + f64::from(*brightness))
                            .clamp(0.0, 1.0)
                    }),
                    Self::Curve { table } => rgb.map(|c| {
                        let position = c * 255.0;
                        let lower = (position.floor() as usize).min(254);
                        let weight = position - lower as f64;
                        f64::from(table[lower]) * (1.0 - weight)
                            + f64::from(table[lower + 1]) * weight
                    }),
                    Self::HueSaturation { hue, saturation } => {
                        rotate_hsv(rgb, f64::from(*hue), f64::from(*saturation))
                    }
                };
                [
                    (adjusted[0] * alpha) as f32,
                    (adjusted[1] * alpha) as f32,
                    (adjusted[2] * alpha) as f32,
                    pixel[3],
                ]
            })
            .collect();
        // Tile validates all output channels before the result can be used.
        Tile::from_pixels(pixels).map_err(AdjustmentError::InvalidOutput)
    }
}

fn rotate_hsv(rgb: [f64; 3], degrees: f64, multiplier: f64) -> [f64; 3] {
    let [r, g, b] = rgb;
    let maximum = r.max(g).max(b);
    let minimum = r.min(g).min(b);
    let delta = maximum - minimum;
    let hue = if delta == 0.0 {
        0.0
    } else if maximum == r {
        ((g - b) / delta).rem_euclid(6.0)
    } else if maximum == g {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    let saturation = if maximum == 0.0 {
        0.0
    } else {
        (delta / maximum * multiplier).clamp(0.0, 1.0)
    };
    let h = (hue + degrees.rem_euclid(360.0) / 60.0).rem_euclid(6.0);
    let chroma = maximum * saturation;
    let x = chroma * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let base = maximum - chroma;
    let channels = match h.floor() as u8 {
        0 => [chroma, x, 0.0],
        1 => [x, chroma, 0.0],
        2 => [0.0, chroma, x],
        3 => [0.0, x, chroma],
        4 => [x, 0.0, chroma],
        _ => [chroma, 0.0, x],
    };
    channels.map(|c| (c + base).clamp(0.0, 1.0))
}

pub fn brightness_contrast(
    source: &Tile,
    brightness: f32,
    contrast: f32,
) -> Result<Tile, AdjustmentError> {
    Adjustment::BrightnessContrast {
        brightness,
        contrast,
    }
    .apply(source)
}

pub fn curve(source: &Tile, table: &[f32; 256]) -> Result<Tile, AdjustmentError> {
    Adjustment::Curve {
        table: Box::new(*table),
    }
    .apply(source)
}

pub fn hue_saturation(source: &Tile, hue: f32, saturation: f32) -> Result<Tile, AdjustmentError> {
    Adjustment::HueSaturation { hue, saturation }.apply(source)
}
