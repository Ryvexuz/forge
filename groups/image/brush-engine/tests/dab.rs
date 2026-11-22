use brush_engine::dab::{DabError, RoundDab};
use brush_engine::ppm::{self, Image};

#[test]
fn dab_stamp_matches_reference_profile() {
    let mut rgba = vec![0; 64 * 64 * 4];
    RoundDab::new(64.0, 1.0, 1.0, 1.0)
        .unwrap()
        .stamp(&mut rgba, 64, 64, [32.0, 32.0], [255; 3])
        .unwrap();
    let image = Image {
        width: 64,
        height: 64,
        rgb: rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[3]; 3])
            .collect(),
    };
    let mut encoded = Vec::new();
    ppm::write(&mut encoded, &image).unwrap();
    let golden = include_bytes!("golden/round_dab_64.ppm");
    assert_eq!(encoded.as_slice(), golden.as_slice());
    assert_eq!(rgba[(32 * 64 + 32) * 4 + 3], 255);
    assert_eq!(rgba[(32 * 64) * 4 + 3], 0);
    // Independent integer-circle reference also checks every golden pixel.
    let decoded = ppm::read(golden).unwrap();
    for y in 0i32..64 {
        for x in 0i32..64 {
            let expected = if (x - 32).pow(2) + (y - 32).pow(2) < 32 * 32 {
                255
            } else {
                0
            };
            assert_eq!(
                decoded.rgb[((y * 64 + x) * 3) as usize..][..3],
                [expected; 3]
            );
        }
    }
}

#[test]
fn pressure_falloff_opacity_and_compositing() {
    let mut pixels = vec![0; 9 * 4];
    RoundDab::new(16.0, 0.5, 0.5, 0.5)
        .unwrap()
        .stamp(&mut pixels, 9, 1, [0.0, 0.0], [200, 100, 50])
        .unwrap();
    // Effective radius four: plateau through two, half coverage at three.
    assert_eq!(pixels[3], 128);
    assert_eq!(pixels[2 * 4 + 3], 128);
    assert_eq!(pixels[3 * 4 + 3], 64);
    assert_eq!(pixels[4 * 4 + 3], 0);
    let mut opaque = [0, 0, 200, 255];
    RoundDab::new(2.0, 1.0, 1.0, 0.5)
        .unwrap()
        .stamp(&mut opaque, 1, 1, [0.0, 0.0], [200, 0, 0])
        .unwrap();
    assert_eq!(opaque, [100, 0, 100, 255]);
}

#[test]
fn validation_and_extreme_finite_inputs() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
        assert_eq!(
            RoundDab::new(value, 1.0, 1.0, 1.0).unwrap_err(),
            DabError::InvalidDiameter
        );
    }
    for value in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(RoundDab::new(1.0, value, 1.0, 1.0).is_err());
        assert!(RoundDab::new(1.0, 1.0, value, 1.0).is_err());
        assert!(RoundDab::new(1.0, 1.0, 1.0, value).is_err());
    }
    for diameter in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let dab = RoundDab::new(diameter, 1.0, 1.0, 1.0).unwrap();
        let mut pixel = [0; 4];
        dab.stamp(&mut pixel, 1, 1, [0.0; 2], [255; 3]).unwrap();
        // The smallest diameter underflows to radius zero; others cover origin.
        assert_eq!(
            pixel[3],
            if diameter == f64::from_bits(1) {
                0
            } else {
                255
            }
        );
        dab.stamp(&mut pixel, 1, 1, [-f64::MAX; 2], [255; 3])
            .unwrap();
    }
    let dab = RoundDab::new(2.0, 1.0, 1.0, 1.0).unwrap();
    let mut pixel = [7; 4];
    assert_eq!(
        dab.stamp(&mut pixel, 1, 1, [f64::NAN, 0.0], [0; 3]),
        Err(DabError::InvalidCenter)
    );
    assert_eq!(
        dab.stamp(&mut pixel, usize::MAX, 2, [0.0; 2], [0; 3]),
        Err(DabError::InvalidBuffer)
    );
    assert_eq!(pixel, [7; 4]);
}

#[test]
fn ppm_binary_boundaries_and_errors() {
    let bytes = b"P6\n# comment\n1 1\n255\n\n#\0";
    let image = ppm::read(bytes).unwrap();
    assert_eq!(image.rgb, b"\n#\0");
    let mut encoded = Vec::new();
    ppm::write(&mut encoded, &image).unwrap();
    assert_eq!(ppm::read(&encoded).unwrap(), image);
    for bad in [
        &b""[..],
        b"P3\n1 1\n255\nabc",
        b"P6\n0 1\n255\n",
        b"P6\n-1 1\n255\nabc",
        b"P6\n1 1\n256\nabc",
        b"P6\n1 1\n255\nab",
        b"P6\n1 1\n255\nabcd",
        b"P6\n999999999999999999999999999999 1\n255\n",
        b"P6\n1 1\n255# missing separator",
    ] {
        assert!(ppm::read(bad).is_err(), "{bad:?}");
    }
    let invalid = Image {
        width: usize::MAX,
        height: 2,
        rgb: Vec::new(),
    };
    assert!(ppm::write(Vec::new(), &invalid).is_err());
    assert!(ppm::write(std::io::Cursor::new(&mut [0u8; 1][..]), &image).is_err());
}
