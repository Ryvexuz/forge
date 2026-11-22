use raster::adjust::{brightness_contrast, curve, hue_saturation, Adjustment, AdjustmentError};
use raster::tile::{Tile, TILE_PIXEL_COUNT};

fn bits(tile: &Tile) -> Vec<[u32; 4]> {
    tile.pixels().iter().map(|p| p.map(f32::to_bits)).collect()
}

// Independent dense reference: HSV reconstruction uses the triangular-wave
// channel formula rather than the implementation's sector selection.
fn reference(pixel: [f32; 4], adjustment: &Adjustment) -> [f64; 4] {
    let a = f64::from(pixel[3]);
    if a == 0.0 {
        return pixel.map(f64::from);
    }
    let rgb = [0, 1, 2].map(|i| f64::from(pixel[i]) / a);
    let result = match adjustment {
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
        } => rgb.map(|v| {
            ((v - 0.5) * f64::from(*contrast) + 0.5 + f64::from(*brightness)).clamp(0.0, 1.0)
        }),
        Adjustment::Curve { table } => rgb.map(|v| {
            let x = (v * 255.0).clamp(0.0, 255.0);
            let lo = x as usize;
            let hi = (lo + 1).min(255);
            f64::from(table[lo]) + (f64::from(table[hi]) - f64::from(table[lo])) * x.fract()
        }),
        Adjustment::HueSaturation { hue, saturation } => {
            let max = rgb.into_iter().fold(0.0_f64, f64::max);
            let min = rgb.into_iter().fold(1.0_f64, f64::min);
            let d = max - min;
            let mut h = 0.0;
            if d != 0.0 {
                let index = rgb.iter().position(|&v| v == max).unwrap();
                h = match index {
                    0 => (rgb[1] - rgb[2]) / d,
                    1 => 2.0 + (rgb[2] - rgb[0]) / d,
                    _ => 4.0 + (rgb[0] - rgb[1]) / d,
                };
            }
            h = (h / 6.0 + f64::from(*hue).rem_euclid(360.0) / 360.0).rem_euclid(1.0);
            let s = if max == 0.0 {
                0.0
            } else {
                (d / max * f64::from(*saturation)).min(1.0)
            };
            [5.0, 3.0, 1.0].map(|offset| {
                let k = (h * 6.0 + offset).rem_euclid(6.0);
                max * (1.0 - s * k.min(4.0 - k).clamp(0.0, 1.0))
            })
        }
    };
    [result[0] * a, result[1] * a, result[2] * a, a]
}

#[test]
fn adjustments_are_non_destructive_round_trip() {
    let pixels = (0..TILE_PIXEL_COUNT)
        .map(|i| {
            let a = match i % 7 {
                0 => 0.0,
                1 => f32::from_bits(1),
                2 => f32::MIN_POSITIVE,
                _ => (i % 257) as f32 / 256.0,
            };
            [
                (i % 256) as f32 / 255.0 * a,
                ((i / 256) % 256) as f32 / 255.0 * a,
                ((i * 73) % 256) as f32 / 255.0 * a,
                a,
            ]
        })
        .collect();
    let source = Tile::from_pixels(pixels).unwrap();
    let original = bits(&source);
    let adjustments = [
        Adjustment::BrightnessContrast {
            brightness: -0.13,
            contrast: 1.7,
        },
        Adjustment::Curve {
            table: Box::new(std::array::from_fn(|i| ((i as f64 / 255.0).powi(2)) as f32)),
        },
        Adjustment::HueSaturation {
            hue: -137.0,
            saturation: 1.4,
        },
        Adjustment::BrightnessContrast {
            brightness: f32::MAX,
            contrast: f32::MAX,
        },
        Adjustment::BrightnessContrast {
            brightness: -0.5,
            contrast: f32::from_bits(1),
        },
        Adjustment::HueSaturation {
            hue: f32::MAX,
            saturation: f32::MAX,
        },
        Adjustment::HueSaturation {
            hue: f32::from_bits(1),
            saturation: f32::from_bits(1),
        },
    ];
    for adjustment in adjustments {
        let output = adjustment.apply(&source).unwrap();
        for (index, (&input, actual)) in source.pixels().iter().zip(output.pixels()).enumerate() {
            let expected = reference(input, &adjustment);
            for channel in 0..4 {
                assert!(
                    (f64::from(actual[channel]) - expected[channel]).abs() <= 1e-6,
                    "{adjustment:?}, pixel {index}, channel {channel}: {} != {}",
                    actual[channel],
                    expected[channel]
                );
            }
            assert_eq!(actual[3].to_bits(), input[3].to_bits());
        }
        drop(output); // Removing the derived view exposes the untouched source.
        assert_eq!(bits(&source), original);
    }
}

#[test]
fn invalid_parameters_fail_even_on_transparent_tiles() {
    let tile = Tile::default();
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            brightness_contrast(&tile, invalid, 1.0),
            Err(AdjustmentError::InvalidBrightness)
        );
        assert_eq!(
            hue_saturation(&tile, invalid, 1.0),
            Err(AdjustmentError::InvalidHue)
        );
    }
    for invalid in [-1.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            brightness_contrast(&tile, 0.0, invalid),
            Err(AdjustmentError::InvalidContrast)
        );
        assert_eq!(
            hue_saturation(&tile, 0.0, invalid),
            Err(AdjustmentError::InvalidSaturation)
        );
    }
    for index in 0..256 {
        for invalid in [-0.1, 1.1, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut table = [0.0; 256];
            table[index] = invalid;
            assert_eq!(
                curve(&tile, &table),
                Err(AdjustmentError::InvalidCurveEntry { index })
            );
        }
    }
}

#[test]
fn curve_clamps_at_the_alpha_boundary_and_tiles_refuse_channels_above_alpha() {
    // A channel above alpha would give a table position above 255; Tile refuses it,
    // so the curve arm only ever sees positions in [0, 255] (and clamps anyway).
    let mut over = vec![[0.0; 4]; TILE_PIXEL_COUNT];
    over[5] = [0.6, 0.1, 0.1, 0.5];
    assert!(Tile::from_pixels(over).is_err());
    let mut pixels = vec![[0.0; 4]; TILE_PIXEL_COUNT];
    pixels[0] = [0.5, 0.25, 0.0, 0.5];
    pixels[1] = [1.0, 1.0, 1.0, 1.0];
    let source = Tile::from_pixels(pixels).unwrap();
    let adjustment = Adjustment::Curve {
        table: Box::new(std::array::from_fn(|i| {
            (i as f32 / 255.0) * 0.5 + 0.25 * (i % 2) as f32
        })),
    };
    let output = adjustment.apply(&source).unwrap();
    for (index, (&input, actual)) in source
        .pixels()
        .iter()
        .zip(output.pixels())
        .enumerate()
        .take(2)
    {
        let expected = reference(input, &adjustment);
        for channel in 0..4 {
            assert!(
                (f64::from(actual[channel]) - expected[channel]).abs() <= 1e-6,
                "pixel {index} channel {channel}"
            );
        }
    }
}
