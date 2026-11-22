//! Smudging of tightly packed RGBA8 channel values with a carried load.
//!
//! The brush carries a load of paint. The first dab lifts the fixed fraction
//! [`MIX`] of the paint under it into the load, so the load starts as the
//! colour under the first dab. At each later dab the canvas pixels under the
//! dab and the load exchange paint by [`MIX`], and the load takes back exactly
//! what the pixels gave up, so canvas total plus load is conserved per dab.
//! [`Stroke::finish`] deposits what the load still holds.

use crate::dab::{DabError, RoundDab};

/// Fixed exchange amount per dab: a pixel moves this fraction (times the dab's
/// coverage) of the way from its colour to the load's colour.
pub const MIX: f64 = 0.5;

/// A smudge stroke in progress: the brush's carried load.
#[derive(Clone, Debug)]
pub struct Stroke {
    /// Total lift weight of the first dab (`MIX` times its coverage sum); the
    /// load colour is `held / weight`.
    weight: f64,
    /// Paint in the load, per channel (an exact integer ledger).
    held: [i64; 4],
    last: [usize; 2],
}

fn footprint(
    dab: &RoundDab,
    pixels: &[u8],
    width: usize,
    height: usize,
    center: [usize; 2],
) -> Result<Vec<(usize, u8)>, DabError> {
    if width.checked_mul(height).and_then(|n| n.checked_mul(4)) != Some(pixels.len()) {
        return Err(DabError::InvalidBuffer);
    }
    if center[0] >= width || center[1] >= height {
        return Err(DabError::InvalidCenter);
    }
    let mut mask = vec![0; pixels.len()];
    dab.stamp(
        &mut mask,
        width,
        height,
        [center[0] as f64, center[1] as f64],
        [0; 3],
    )?;
    Ok(mask
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(_, c)| c[3] > 0)
        .map(|(i, c)| (i * 4, c[3]))
        .collect())
}

impl Stroke {
    /// Start a stroke: lift paint under the first dab at `center` into the load.
    pub fn begin(
        dab: &RoundDab,
        pixels: &mut [u8],
        width: usize,
        height: usize,
        center: [usize; 2],
    ) -> Result<Stroke, DabError> {
        let mut held = [0; 4];
        let mut weight = 0.0;
        for (at, coverage) in footprint(dab, pixels, width, height, center)? {
            let amount = MIX * f64::from(coverage) / 255.0;
            weight += amount;
            for (channel, total) in held.iter_mut().enumerate() {
                let lift = (f64::from(pixels[at + channel]) * amount).round() as u8;
                pixels[at + channel] -= lift;
                *total += i64::from(lift);
            }
        }
        Ok(Stroke {
            weight,
            held,
            last: center,
        })
    }

    /// Paint the load holds, per channel: canvas total plus this is the canvas
    /// total at the start of the stroke.
    pub fn held(&self) -> [i64; 4] {
        self.held
    }

    /// Exchange paint between the pixels under a dab at `center` and the load.
    pub fn dab(
        &mut self,
        dab: &RoundDab,
        pixels: &mut [u8],
        width: usize,
        height: usize,
        center: [usize; 2],
    ) -> Result<(), DabError> {
        let cells = footprint(dab, pixels, width, height, center)?;
        self.last = center;
        if self.weight == 0.0 {
            return Ok(());
        }
        let load: [f64; 4] = std::array::from_fn(|c| self.held[c] as f64 / self.weight);
        for (at, coverage) in cells {
            let amount = MIX * f64::from(coverage) / 255.0;
            for (channel, colour) in load.iter().enumerate() {
                let p = f64::from(pixels[at + channel]);
                let next = (p + amount * (colour - p)).round().clamp(0.0, 255.0) as u8;
                self.held[channel] += i64::from(pixels[at + channel]) - i64::from(next);
                pixels[at + channel] = next;
            }
        }
        Ok(())
    }

    /// End the stroke: deposit the load's ledger evenly under the last dab (a
    /// negative amount is taken back), limited by the 0..=255 range. Returns
    /// what the ledger still holds after rounding and clamping.
    pub fn finish(
        mut self,
        dab: &RoundDab,
        pixels: &mut [u8],
        width: usize,
        height: usize,
    ) -> Result<[i64; 4], DabError> {
        let cells = footprint(dab, pixels, width, height, self.last)?;
        let n = cells.len() as i64;
        if n == 0 {
            return Ok(self.held);
        }
        for channel in 0..4 {
            let share = self.held[channel] / n;
            let extra = self.held[channel] % n;
            for (k, &(at, _)) in cells.iter().enumerate() {
                let bonus = if (k as i64) < extra.abs() {
                    extra.signum()
                } else {
                    0
                };
                let old = i64::from(pixels[at + channel]);
                let new = (old + share + bonus).clamp(0, 255);
                pixels[at + channel] = new as u8;
                self.held[channel] -= new - old;
            }
        }
        Ok(self.held)
    }
}

/// Smudge along `centers` (at least one) and finish the stroke; returns what
/// the load's ledger still holds per channel.
pub fn smudge_stroke(
    dab: &RoundDab,
    pixels: &mut [u8],
    width: usize,
    height: usize,
    centers: &[[usize; 2]],
) -> Result<[i64; 4], DabError> {
    let (first, rest) = centers.split_first().ok_or(DabError::InvalidCenter)?;
    // Validate every centre (and the buffer) before the first dab lifts any paint, so an Err leaves the canvas as it was.
    for &center in centers {
        footprint(dab, pixels, width, height, center)?;
    }
    let mut stroke = Stroke::begin(dab, pixels, width, height, *first)?;
    for &center in rest {
        stroke.dab(dab, pixels, width, height, center)?;
    }
    stroke.finish(dab, pixels, width, height)
}
