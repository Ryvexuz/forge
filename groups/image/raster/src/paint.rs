//! Hard, round brush strokes on sparse tiles.

use std::collections::BTreeSet;

use crate::blend::source_over;
use crate::tile::{Tile, TileError, TileStore, TILE_SIZE};

/// Paints the closed capsule around an integer-coordinate line segment.
///
/// Pixels are sampled at integer coordinates; points at exactly `radius` are
/// included. A zero-length segment is a disk, and zero radius includes samples
/// on the segment. Colour is normalized premultiplied RGBA, validated before any
/// mutation. Each covered pixel receives source-over exactly once.
///
/// Returns only coordinates whose stored pixels changed. Integer endpoints and
/// radius bound geometry to the `i32`/`u32` domains; differences are computed in
/// `i64` before conversion to `f64`, so neither subtraction nor squared distance
/// can overflow. Work is proportional to the capsule's bounding-box area.
pub fn stamp_line(
    store: &mut TileStore,
    from: (i32, i32),
    to: (i32, i32),
    radius: u32,
    colour: [f32; 4],
) -> Result<BTreeSet<(i32, i32)>, TileError> {
    // Use the tile's public validator rather than duplicate its channel rules.
    let mut validation = Tile::default();
    validation.set_pixel(0, 0, colour)?;
    let mut dirty = BTreeSet::new();
    if colour[3] == 0.0 {
        return Ok(dirty);
    }

    let radius = i64::from(radius);
    let min_x = i64::from(from.0.min(to.0)) - radius;
    let max_x = i64::from(from.0.max(to.0)) + radius;
    let min_y = i64::from(from.1.min(to.1)) - radius;
    let max_y = i64::from(from.1.max(to.1)) + radius;
    let size = TILE_SIZE as i64;
    let dx = i64::from(to.0) - i64::from(from.0);
    let dy = i64::from(to.1) - i64::from(from.1);
    // Integer dot and cross products preserve cancellation even at the ends of
    // the coordinate domain. Only the final squared comparison uses f64.
    let length_squared = i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy);
    let radius_squared = (radius as f64) * (radius as f64);

    for ty in min_y.div_euclid(size)..=max_y.div_euclid(size) {
        for tx in min_x.div_euclid(size)..=max_x.div_euclid(size) {
            // Expanded i32 pixel coordinates still fit i32 tile coordinates.
            let coord = (tx as i32, ty as i32);
            let mut tile = store.tile_at(coord.0, coord.1).cloned().unwrap_or_default();
            let mut changed = false;
            for y in min_y.max(ty * size)..=max_y.min((ty + 1) * size - 1) {
                for x in min_x.max(tx * size)..=max_x.min((tx + 1) * size - 1) {
                    let px = i128::from(x - i64::from(from.0));
                    let py = i128::from(y - i64::from(from.1));
                    let dot = px * i128::from(dx) + py * i128::from(dy);
                    let distance_squared = if dot <= 0 {
                        (px * px + py * py) as f64
                    } else if dot >= length_squared {
                        let qx = px - i128::from(dx);
                        let qy = py - i128::from(dy);
                        (qx * qx + qy * qy) as f64
                    } else {
                        let cross = (px * i128::from(dy) - py * i128::from(dx)) as f64;
                        cross * cross / length_squared as f64
                    };
                    if distance_squared <= radius_squared {
                        let local_x = x.rem_euclid(size) as usize;
                        let local_y = y.rem_euclid(size) as usize;
                        let old = tile.pixel(local_x, local_y)?;
                        let new = source_over(old, colour);
                        if old != new {
                            tile.set_pixel(local_x, local_y, new)?;
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                store.write_tile(coord.0, coord.1, tile);
                dirty.insert(coord);
            }
        }
    }
    Ok(dirty)
}
