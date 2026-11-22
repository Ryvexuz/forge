//! Sparse normalized mask planes and binary selections.

use std::collections::HashMap;
use std::fmt;

use crate::tile::TILE_PIXEL_COUNT;

/// Invalid plane data or alpha input. Rejected writes leave storage unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaskError {
    WrongValueCount { actual: usize },
    InvalidValue { index: usize },
    InvalidAlpha,
}

impl fmt::Display for MaskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongValueCount { actual } => {
                write!(f, "expected {TILE_PIXEL_COUNT} values, got {actual}")
            }
            Self::InvalidValue { index } => write!(f, "invalid plane value at {index}"),
            Self::InvalidAlpha => write!(f, "alpha factors must be finite and in [0, 1]"),
        }
    }
}

impl std::error::Error for MaskError {}

fn normalized(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

/// Product of normalized layer alpha, mask coverage and selection coverage.
///
/// All factors must be finite and in `[0, 1]`. Multiplication is left
/// associative in `f32`; very small products may underflow to zero.
pub fn compose_alpha(layer: f32, mask: f32, selection: f32) -> Result<f32, MaskError> {
    if ![layer, mask, selection].into_iter().all(normalized) {
        return Err(MaskError::InvalidAlpha);
    }
    Ok(layer * mask * selection)
}

#[derive(Clone, Debug, Default)]
struct Plane {
    tiles: HashMap<(i32, i32), Vec<f32>>,
}

impl Plane {
    fn write_tile(
        &mut self,
        x: i32,
        y: i32,
        values: Vec<f32>,
        binary: bool,
    ) -> Result<(), MaskError> {
        if values.len() != TILE_PIXEL_COUNT {
            return Err(MaskError::WrongValueCount {
                actual: values.len(),
            });
        }
        for (index, &value) in values.iter().enumerate() {
            if !normalized(value) || (binary && value != 0.0 && value != 1.0) {
                return Err(MaskError::InvalidValue { index });
            }
        }
        if values.iter().all(|&value| value == 0.0) {
            self.tiles.remove(&(x, y));
        } else {
            self.tiles.insert((x, y), values);
        }
        Ok(())
    }
}

/// Sparse tile-shaped `f32` coverage, with finite values in `[0, 1]`.
///
/// Missing tiles represent zero coverage and allocate no pixel data.
#[derive(Clone, Debug, Default)]
pub struct Mask {
    plane: Plane,
}

impl Mask {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.plane.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plane.tiles.is_empty()
    }

    /// Row-major coverage, or `None` for an implicit all-zero tile.
    pub fn tile_at(&self, x: i32, y: i32) -> Option<&[f32]> {
        self.plane.tiles.get(&(x, y)).map(Vec::as_slice)
    }

    /// Validates and replaces a complete tile; zero tiles remove the entry.
    pub fn write_tile(&mut self, x: i32, y: i32, values: Vec<f32>) -> Result<(), MaskError> {
        self.plane.write_tile(x, y, values, false)
    }
}

/// Sparse selection coverage: each stored value is exactly zero or one.
///
/// Like [`Mask`], missing tiles mean zero coverage.
#[derive(Clone, Debug, Default)]
pub struct Selection {
    plane: Plane,
}

impl Selection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.plane.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plane.tiles.is_empty()
    }

    pub fn tile_at(&self, x: i32, y: i32) -> Option<&[f32]> {
        self.plane.tiles.get(&(x, y)).map(Vec::as_slice)
    }

    /// Validates and replaces a complete binary tile; zero tiles remove it.
    pub fn write_tile(&mut self, x: i32, y: i32, values: Vec<f32>) -> Result<(), MaskError> {
        self.plane.write_tile(x, y, values, true)
    }
}
