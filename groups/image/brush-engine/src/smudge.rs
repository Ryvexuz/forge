//! Conservative smudging of tightly packed RGBA8 channel values.

use crate::dab::{DabError, RoundDab};

/// Move a dab from `from` to `to`, mixing sampled brush colour into the
/// destination at fixed half strength, scaled by the dab's coverage.
///
/// Both centres must be inside the layer. Footprint pairs crossing an edge
/// are skipped. Each deposit has an equal, opposite pickup at the source:
/// all four raw channel sums are conserved exactly, including rounding.
/// This conserves stored channel levels, not alpha-weighted optical energy.
/// Overlapping pairs are processed in row-major source order.
pub fn smudge(
    dab: &RoundDab,
    pixels: &mut [u8],
    width: usize,
    height: usize,
    from: [usize; 2],
    to: [usize; 2],
) -> Result<(), DabError> {
    if width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(pixels.len()) {
        return Err(DabError::InvalidBuffer);
    }
    if from[0] >= width || to[0] >= width || from[1] >= height || to[1] >= height {
        return Err(DabError::InvalidCenter);
    }
    if from == to {
        return Ok(());
    }
    let mut mask = vec![0; pixels.len()];
    dab.stamp(
        &mut mask,
        width,
        height,
        [from[0] as f64, from[1] as f64],
        [0; 3],
    )?;
    for (index, coverage) in mask.as_chunks::<4>().0.iter().enumerate() {
        let translate = |coordinate: usize, start: usize, end: usize| {
            if end >= start {
                coordinate.checked_add(end - start)
            } else {
                coordinate.checked_sub(start - end)
            }
        };
        let Some(x) = translate(index % width, from[0], to[0]).filter(|&x| x < width) else {
            continue;
        };
        let Some(y) = translate(index / width, from[1], to[1]).filter(|&y| y < height) else {
            continue;
        };
        let source = index * 4;
        let destination = (y * width + x) * 4;
        for channel in 0..4 {
            let a = i32::from(pixels[source + channel]);
            let b = i32::from(pixels[destination + channel]);
            // 255 coverage levels times the fixed mixing denominator of two.
            // Round symmetrically; the same integer transfer is used at both ends.
            let numerator = (a - b) * i32::from(coverage[3]);
            let transfer = (numerator + numerator.signum() * 255) / 510;
            pixels[source + channel] = (a - transfer) as u8;
            pixels[destination + channel] = (b + transfer) as u8;
        }
    }
    Ok(())
}
