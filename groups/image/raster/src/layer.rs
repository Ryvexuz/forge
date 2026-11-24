//! Sparse, bottom-to-top document compositing in linear premultiplied RGBA.
//!
//! Coverage scales source alpha and colours. Zero effective alpha preserves the
//! backdrop bit for bit. Normal uses source-over; other modes blend unassociated
//! channels and combine `cs*(1-ab) + cb*(1-as) + as*ab*B(Cb,Cs)`.
//! A transparent backdrop needs no blend or division. All computed outputs pass
//! through one finish step that clamps alpha to [0,1] and colours to [0,alpha].

use crate::blend;
use crate::mask::{compose_alpha, Mask};
use crate::tile::{Tile, TileError, TileStore, TILE_PIXEL_COUNT, TILE_SIZE};
use std::fmt;

/// The twelve separable W3C modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
}

impl BlendMode {
    /// Stable order used by layer recipes.
    pub const ALL: [Self; 12] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Darken,
        Self::Lighten,
        Self::ColorDodge,
        Self::ColorBurn,
        Self::HardLight,
        Self::SoftLight,
        Self::Difference,
        Self::Exclusion,
    ];

    pub fn channel(self) -> fn(f32, f32) -> f32 {
        match self {
            Self::Normal => blend::normal,
            Self::Multiply => blend::multiply,
            Self::Screen => blend::screen,
            Self::Overlay => blend::overlay,
            Self::Darken => blend::darken,
            Self::Lighten => blend::lighten,
            Self::ColorDodge => blend::color_dodge,
            Self::ColorBurn => blend::color_burn,
            Self::HardLight => blend::hard_light,
            Self::SoftLight => blend::soft_light,
            Self::Difference => blend::difference,
            Self::Exclusion => blend::exclusion,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayerError {
    ZeroCanvas { width: u32, height: u32 },
    InvalidOpacity,
    InvalidOutput(TileError),
}

impl fmt::Display for LayerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroCanvas { width, height } => write!(f, "zero canvas dimension: {width}x{height}"),
            Self::InvalidOpacity => write!(f, "opacity must be finite and in [0, 1]"),
            Self::InvalidOutput(error) => write!(f, "invalid composite output: {error}"),
        }
    }
}

impl std::error::Error for LayerError {}

/// Sparse premultiplied tiles with validated opacity and optional zero-default mask.
#[derive(Clone, Debug)]
pub struct Layer {
    tiles: TileStore,
    opacity: f32,
    mode: BlendMode,
    mask: Option<Mask>,
}

impl Layer {
    pub fn new(
        tiles: TileStore,
        opacity: f32,
        mode: BlendMode,
        mask: Option<Mask>,
    ) -> Result<Self, LayerError> {
        if !opacity.is_finite() || !(0.0..=1.0).contains(&opacity) {
            return Err(LayerError::InvalidOpacity);
        }
        Ok(Self {
            tiles,
            opacity,
            mode,
            mask,
        })
    }

    pub fn tiles(&self) -> &TileStore {
        &self.tiles
    }

    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    pub fn mode(&self) -> BlendMode {
        self.mode
    }

    pub fn mask(&self) -> Option<&Mask> {
        self.mask.as_ref()
    }
}

/// A nonzero canvas and a stack ordered from bottom to top.
#[derive(Clone, Debug)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Result<Self, LayerError> {
        if width == 0 || height == 0 {
            return Err(LayerError::ZeroCanvas { width, height });
        }
        Ok(Self {
            width,
            height,
            layers: Vec::new(),
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn push_layer(&mut self, layer: Layer) {
        self.layers.push(layer);
    }

    /// Panics on an invalid index, like `Vec::remove`.
    pub fn remove_layer(&mut self, index: usize) -> Layer {
        self.layers.remove(index)
    }
}

fn finish(mut out: [f32; 4]) -> [f32; 4] {
    let alpha = out[3].clamp(0.0, 1.0);
    for colour in &mut out[..3] {
        *colour = colour.clamp(0.0, alpha);
    }
    out[3] = alpha;
    out
}

/// One step on valid premultiplied pixels and coverage in [0,1].
/// Inputs are not validated on this hot path.
pub fn composite_pixel(
    backdrop: [f32; 4],
    source: [f32; 4],
    mode: BlendMode,
    coverage: f32,
) -> [f32; 4] {
    let alpha = source[3] * coverage;
    if alpha == 0.0 {
        return backdrop;
    }
    let scaled = [
        source[0] * coverage,
        source[1] * coverage,
        source[2] * coverage,
        alpha,
    ];
    if mode == BlendMode::Normal {
        return finish(blend::source_over(backdrop, scaled));
    }
    let ab = backdrop[3];
    // Keep the no-division path explicit. Removing it is an equivalent mutant:
    // f32::min maps 0/0 to 1 here, and the blend term is multiplied by zero.
    if ab == 0.0 {
        return finish(scaled);
    }
    let mut out = [0.0; 4];
    for k in 0..3 {
        let cb = (backdrop[k] / ab).min(1.0);
        let cs = (source[k] / source[3]).min(1.0);
        out[k] = scaled[k] * (1.0 - ab)
            + backdrop[k] * (1.0 - alpha)
            + alpha * ab * mode.channel()(cb, cs);
    }
    out[3] = alpha + ab * (1.0 - alpha);
    finish(out)
}

/// Composites canvas tiles in row-major order, skipping absent source tiles.
pub fn composite_document(document: &Document) -> Result<TileStore, LayerError> {
    let mut output = TileStore::new();
    // u32 canvas dimensions fit these tile counts and their i32 coordinates.
    let size = TILE_SIZE as u32;
    for ty in 0..document.height.div_ceil(size) {
        for tx in 0..document.width.div_ceil(size) {
            let (ix, iy) = (tx as i32, ty as i32);
            if !document
                .layers()
                .iter()
                .any(|layer| layer.tiles().tile_at(ix, iy).is_some())
            {
                continue;
            }
            let mut pixels = vec![[0.0; 4]; TILE_PIXEL_COUNT];
            for layer in document.layers() {
                let Some(tile) = layer.tiles().tile_at(ix, iy) else {
                    continue;
                };
                let mask = match layer.mask() {
                    Some(mask) => match mask.tile_at(ix, iy) {
                        Some(values) => Some(values),
                        None => continue,
                    },
                    None => None,
                };
                for (index, pixel) in pixels.iter_mut().enumerate() {
                    let x = index % TILE_SIZE;
                    let y = index / TILE_SIZE;
                    if tx * size + x as u32 >= document.width
                        || ty * size + y as u32 >= document.height
                    {
                        continue;
                    }
                    let coverage =
                        compose_alpha(layer.opacity(), mask.map_or(1.0, |m| m[index]), 1.0)
                            .map_err(|_| LayerError::InvalidOpacity)?;
                    *pixel = composite_pixel(*pixel, tile.pixels()[index], layer.mode(), coverage);
                }
            }
            let tile = Tile::from_pixels(pixels).map_err(LayerError::InvalidOutput)?;
            output.write_tile(ix, iy, tile);
        }
    }
    Ok(output)
}
