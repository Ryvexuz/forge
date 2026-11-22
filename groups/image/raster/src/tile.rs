//! Premultiplied RGBA tiles and sparse tile storage.

use std::collections::HashMap;
use std::fmt;

/// Width and height of a tile in pixels.
pub const TILE_SIZE: usize = 256;
/// Number of pixels in a tile.
pub const TILE_PIXEL_COUNT: usize = TILE_SIZE * TILE_SIZE;

/// Invalid tile data or pixel coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileError {
    /// A tile must contain exactly [`TILE_PIXEL_COUNT`] pixels.
    WrongPixelCount { actual: usize },
    /// Channels must be finite and satisfy `0 <= R,G,B <= A <= 1`.
    InvalidPixel { index: usize },
    /// Coordinates must be inside a tile.
    OutOfBounds { x: usize, y: usize },
}

impl fmt::Display for TileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongPixelCount { actual } => {
                write!(f, "expected {TILE_PIXEL_COUNT} pixels, got {actual}")
            }
            Self::InvalidPixel { index } => write!(f, "invalid premultiplied pixel at {index}"),
            Self::OutOfBounds { x, y } => write!(f, "tile pixel ({x}, {y}) is out of bounds"),
        }
    }
}

impl std::error::Error for TileError {}

/// A heap-backed 256 × 256 tile of row-major, premultiplied `f32` RGBA.
///
/// This storage accepts normalized channels only: finite values satisfying
/// `0 <= R,G,B <= A <= 1`. Mutation is validated before changing the tile.
#[derive(Clone, Debug, PartialEq)]
pub struct Tile {
    pixels: Vec<[f32; 4]>,
}

impl Default for Tile {
    fn default() -> Self {
        Self {
            pixels: vec![[0.0; 4]; TILE_PIXEL_COUNT],
        }
    }
}

impl Tile {
    /// Constructs a tile without changing any channel values.
    pub fn from_pixels(pixels: Vec<[f32; 4]>) -> Result<Self, TileError> {
        if pixels.len() != TILE_PIXEL_COUNT {
            return Err(TileError::WrongPixelCount {
                actual: pixels.len(),
            });
        }
        for (index, &pixel) in pixels.iter().enumerate() {
            Self::validate_pixel(pixel, index)?;
        }
        Ok(Self { pixels })
    }

    fn validate_pixel(pixel: [f32; 4], index: usize) -> Result<(), TileError> {
        let alpha = pixel[3];
        if !pixel.iter().all(|channel| channel.is_finite())
            || !(0.0..=1.0).contains(&alpha)
            || !pixel[..3]
                .iter()
                .all(|&channel| (0.0..=alpha).contains(&channel))
        {
            return Err(TileError::InvalidPixel { index });
        }
        Ok(())
    }

    fn index(x: usize, y: usize) -> Result<usize, TileError> {
        if x >= TILE_SIZE || y >= TILE_SIZE {
            return Err(TileError::OutOfBounds { x, y });
        }
        Ok(y * TILE_SIZE + x)
    }

    /// Returns all pixels in row-major order.
    pub fn pixels(&self) -> &[[f32; 4]] {
        &self.pixels
    }

    pub fn pixel(&self, x: usize, y: usize) -> Result<[f32; 4], TileError> {
        Ok(self.pixels[Self::index(x, y)?])
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, pixel: [f32; 4]) -> Result<(), TileError> {
        let index = Self::index(x, y)?;
        Self::validate_pixel(pixel, index)?;
        self.pixels[index] = pixel;
        Ok(())
    }

    /// Whether every channel is zero (fully transparent premultiplied RGBA).
    pub fn is_empty(&self) -> bool {
        self.pixels.iter().all(|pixel| *pixel == [0.0; 4])
    }
}

/// Sparse, unbounded tile-coordinate storage, independent of canvas dimensions.
///
/// Missing coordinates represent transparent tiles and allocate no pixel data.
#[derive(Clone, Debug, Default)]
pub struct TileStore {
    tiles: HashMap<(i32, i32), Tile>,
}

impl TileStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of stored (nonempty) tiles.
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    /// Borrows a painted tile; absent coordinates return `None`.
    pub fn tile_at(&self, x: i32, y: i32) -> Option<&Tile> {
        self.tiles.get(&(x, y))
    }

    /// Replaces a tile exactly, or removes the entry if the tile is empty.
    pub fn write_tile(&mut self, x: i32, y: i32, tile: Tile) {
        if tile.is_empty() {
            self.tiles.remove(&(x, y));
        } else {
            self.tiles.insert((x, y), tile);
        }
    }
}
