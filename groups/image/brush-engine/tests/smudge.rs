use brush_engine::dab::{DabError, RoundDab};
use brush_engine::smudge::{smudge_stroke, Stroke};

fn totals(pixels: &[u8]) -> [u64; 4] {
    let mut sums = [0; 4];
    for pixel in pixels.as_chunks::<4>().0 {
        for channel in 0..4 {
            sums[channel] += u64::from(pixel[channel]);
        }
    }
    sums
}

fn patch(width: usize, height: usize, colour: [u8; 3]) -> Vec<u8> {
    let mut pixels = vec![0; width * height * 4];
    RoundDab::new(8.0, 1.0, 1.0, 1.0)
        .unwrap()
        .stamp(&mut pixels, width, height, [6.0, 6.0], colour)
        .unwrap();
    pixels
}

#[test]
fn smudge_conserves_paint_mass() {
    let (width, height) = (32, 12);
    let mut pixels = patch(width, height, [231, 117, 53]);
    let before = totals(&pixels);
    let original = pixels.clone();
    let dab = RoundDab::new(8.0, 1.0, 0.5, 0.75).unwrap();
    let centers: Vec<[usize; 2]> = (6..30).chain((0..29).rev()).map(|x| [x, 6]).collect();
    smudge_stroke(&dab, &mut pixels, width, height, &centers).unwrap();
    assert_ne!(pixels, original, "the stroke must actually move paint");
    let after = totals(&pixels);
    let before_rgb: u64 = before[..3].iter().sum();
    let after_rgb: u64 = after[..3].iter().sum();
    assert!(
        before_rgb.abs_diff(after_rgb) <= (width * height) as u64,
        "before {before_rgb} after {after_rgb}"
    );
}

#[test]
fn canvas_plus_load_is_exact_at_every_dab() {
    let (width, height) = (32, 12);
    let mut pixels = patch(width, height, [231, 117, 53]);
    let dab = RoundDab::new(8.0, 1.0, 0.5, 0.75).unwrap();
    let before = totals(&pixels);
    let mut stroke = Stroke::begin(&dab, &mut pixels, width, height, [6, 6]).unwrap();
    for x in (7..30).chain((0..29).rev()) {
        stroke
            .dab(&dab, &mut pixels, width, height, [x, 6])
            .unwrap();
        let held = stroke.held();
        for c in 0..4 {
            assert_eq!(totals(&pixels)[c] as i64 + held[c], before[c] as i64);
        }
    }
}

#[test]
fn stroke_carries_red_onto_blank_paper() {
    let (width, height) = (32, 12);
    let mut pixels = patch(width, height, [255, 0, 0]);
    let dab = RoundDab::new(8.0, 1.0, 0.5, 0.75).unwrap();
    let mut stroke = Stroke::begin(&dab, &mut pixels, width, height, [6, 6]).unwrap();
    for x in 7..24 {
        stroke
            .dab(&dab, &mut pixels, width, height, [x, 6])
            .unwrap();
    }
    let channel_sum = |channel: usize| -> u32 {
        (14..24)
            .map(|x| u32::from(pixels[(6 * width + x) * 4 + channel]))
            .sum()
    };
    assert!(channel_sum(0) > 0, "no red carried onto the paper");
    assert_eq!(channel_sum(1), 0);
    assert_eq!(channel_sum(2), 0);
}

#[test]
fn invalid_inputs_are_atomic_and_empty_stroke_is_refused() {
    let dab = RoundDab::new(1.0, 1.0, 1.0, 1.0).unwrap();
    let mut pixels = [31; 8];
    assert_eq!(
        smudge_stroke(&dab, &mut pixels, usize::MAX, 2, &[[0, 0]]),
        Err(DabError::InvalidBuffer)
    );
    assert_eq!(
        smudge_stroke(&dab, &mut pixels, 2, 1, &[[0, 0], [usize::MAX, 0]]),
        Err(DabError::InvalidCenter)
    );
    assert_eq!(
        smudge_stroke(&dab, &mut pixels, 2, 1, &[]),
        Err(DabError::InvalidCenter)
    );
    for diameter in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let dab = RoundDab::new(diameter, 1.0, 1.0, 0.0).unwrap();
        let mut pixels = [255, 0, 1, 255, 0, 255, 254, 0];
        let before = pixels;
        smudge_stroke(&dab, &mut pixels, 2, 1, &[[0, 0], [1, 0]]).unwrap();
        assert_eq!(pixels, before);
    }
}

#[test]
fn bad_later_centre_leaves_canvas_untouched() {
    let dab = RoundDab::new(6.0, 1.0, 1.0, 1.0).unwrap();
    for centres in [
        vec![[6, 6], [7, 6], [usize::MAX, 0]],
        vec![[6, 6], [12, 6]],
        vec![[6, 6], [6, 12], [7, 7]],
    ] {
        let mut pixels = patch(12, 12, [200, 100, 50]);
        let before = pixels.clone();
        assert_eq!(
            smudge_stroke(&dab, &mut pixels, 12, 12, &centres),
            Err(DabError::InvalidCenter)
        );
        assert_eq!(pixels, before, "centres {centres:?}");
    }
}
