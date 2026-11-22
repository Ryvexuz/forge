use brush_engine::dab::{DabError, RoundDab};
use brush_engine::smudge::smudge;

fn totals(pixels: &[u8]) -> [u64; 4] {
    let mut sums = [0; 4];
    for pixel in pixels.as_chunks::<4>().0 {
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    sums
}

#[test]
fn smudge_conserves_paint_mass() {
    let (width, height) = (32, 12);
    let mut pixels = vec![0; width * height * 4];
    RoundDab::new(8.0, 1.0, 1.0, 1.0)
        .unwrap()
        .stamp(&mut pixels, width, height, [6.0, 6.0], [231, 117, 53])
        .unwrap();
    let before = totals(&pixels);
    let original = pixels.clone();
    let dab = RoundDab::new(8.0, 1.0, 0.5, 0.75).unwrap();
    let mut previous = [6, 6];
    for x in (7..30).chain((0..29).rev()) {
        let next = [x, 6];
        smudge(&dab, &mut pixels, width, height, previous, next).unwrap();
        assert_eq!(totals(&pixels), before);
        previous = next;
    }
    assert_ne!(pixels, original, "the stroke must actually move paint");
    let after = totals(&pixels);
    let before_rgb: u64 = before[..3].iter().sum();
    let after_rgb: u64 = after[..3].iter().sum();
    assert!(before_rgb.abs_diff(after_rgb) <= (width * height) as u64);
}

#[test]
fn pickup_mix_and_deposit_match_independent_reference() {
    let dab = RoundDab::new(1.0, 1.0, 1.0, 1.0).unwrap();
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            let mut pixels = [a, b, a, b, b, a, b, a];
            smudge(&dab, &mut pixels, 2, 1, [0, 0], [1, 0]).unwrap();
            // Independent f64 reference: half the signed difference, ties away
            // from zero. Byte inputs and halves are exactly representable.
            let transfer = ((f64::from(a) - f64::from(b)) / 2.0).round() as i32;
            let low = (i32::from(a) - transfer) as u8;
            let high = (i32::from(b) + transfer) as u8;
            assert_eq!(pixels, [low, high, low, high, high, low, high, low]);
        }
    }
}

#[test]
fn invalid_inputs_are_atomic_and_zero_coverage_is_unchanged() {
    let dab = RoundDab::new(1.0, 1.0, 1.0, 1.0).unwrap();
    let mut pixels = [31; 8];
    assert_eq!(
        smudge(&dab, &mut pixels, usize::MAX, 2, [0, 0], [1, 0]),
        Err(DabError::InvalidBuffer)
    );
    assert_eq!(
        smudge(&dab, &mut pixels, 2, 1, [0, 0], [usize::MAX, 0]),
        Err(DabError::InvalidCenter)
    );
    assert_eq!(pixels, [31; 8]);
    for diameter in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let dab = RoundDab::new(diameter, 1.0, 1.0, 0.0).unwrap();
        let mut pixels = [255, 0, 1, 255, 0, 255, 254, 0];
        let before = pixels;
        smudge(&dab, &mut pixels, 2, 1, [0, 0], [1, 0]).unwrap();
        assert_eq!(pixels, before);
    }
}
