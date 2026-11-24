use raster::blend;
use raster::layer::{composite_document, composite_pixel, BlendMode, Document, Layer, LayerError};
use raster::mask::Mask;
use raster::tile::{Tile, TileStore, TILE_PIXEL_COUNT, TILE_SIZE};

const TABLE: [fn(f32, f32) -> f32; 12] = [
    blend::normal,
    blend::multiply,
    blend::screen,
    blend::overlay,
    blend::darken,
    blend::lighten,
    blend::color_dodge,
    blend::color_burn,
    blend::hard_light,
    blend::soft_light,
    blend::difference,
    blend::exclusion,
];

fn h(i: u32, x: u32, y: u32, k: u32) -> u32 {
    let mut v = i.wrapping_mul(0x9E37_79B9)
        ^ x.wrapping_mul(0x85EB_CA6B)
        ^ y.wrapping_mul(0xC2B2_AE35)
        ^ k.wrapping_mul(0x27D4_EB2F);
    v ^= v >> 15;
    v = v.wrapping_mul(0x2C1B_3C6D);
    v ^= v >> 12;
    v
}

fn layer_pixel(i: u32, x: u32, y: u32) -> [f32; 4] {
    if (i + x / 256 + 3 * (y / 256)).is_multiple_of(3) || h(i, x, y, 4).is_multiple_of(11) {
        return [0.0; 4];
    }
    let a = (64 + h(i, x, y, 3) % 193) as f32 / 256.0;
    [
        a * ((32 + h(i, x, y, 0) % 193) as f32 / 256.0),
        a * ((32 + h(i, x, y, 1) % 193) as f32 / 256.0),
        a * ((32 + h(i, x, y, 2) % 193) as f32 / 256.0),
        a,
    ]
}

fn mask_value(i: u32, x: u32, y: u32) -> f32 {
    (h(i, x, y, 5) % 5) as f32 / 4.0
}

fn opacity(i: u32) -> f32 {
    0.5 + (i % 6) as f32 * 0.1
}

fn recipe() -> Document {
    let mut doc = Document::new(768, 512).unwrap();
    for i in 0..50 {
        let mut tiles = TileStore::new();
        let mut mask = Mask::new();
        for ty in 0..2 {
            for tx in 0..3 {
                let mut pixels = Vec::with_capacity(TILE_PIXEL_COUNT);
                let mut values = Vec::with_capacity(TILE_PIXEL_COUNT);
                for y in 0..256 {
                    for x in 0..256 {
                        let (cx, cy) = (tx * 256 + x, ty * 256 + y);
                        pixels.push(layer_pixel(i, cx, cy));
                        values.push(mask_value(i, cx, cy));
                    }
                }
                tiles.write_tile(tx as i32, ty as i32, Tile::from_pixels(pixels).unwrap());
                if (tx, ty) != (i % 3, (i / 3) % 2) {
                    mask.write_tile(tx as i32, ty as i32, values).unwrap();
                }
            }
        }
        doc.push_layer(
            Layer::new(
                tiles,
                opacity(i),
                BlendMode::ALL[i as usize % 12],
                if i % 2 == 1 { Some(mask) } else { None },
            )
            .unwrap(),
        );
    }
    doc
}

// Dense W3C two-step reference: no document stores or compositor helpers.
fn reference(x: u32, y: u32) -> [f32; 4] {
    let mut b = [0.0_f32; 4];
    for i in 0..50 {
        let s = layer_pixel(i, x, y);
        let mask = if i % 2 == 0 {
            1.0
        } else if (x / 256, y / 256) == (i % 3, (i / 3) % 2) {
            0.0
        } else {
            mask_value(i, x, y)
        };
        let coverage = opacity(i) * mask * 1.0;
        let alpha = s[3] * coverage;
        let _scaled_colours = [s[0] * coverage, s[1] * coverage, s[2] * coverage];
        if alpha == 0.0 {
            continue;
        }
        let ab = b[3];
        for k in 0..3 {
            let cb = if ab == 0.0 {
                0.0_f32
            } else {
                (b[k] / ab).min(1.0)
            };
            let cs = (s[k] / s[3]).min(1.0);
            let blended = (1.0 - ab) * cs + ab * TABLE[i as usize % 12](cb, cs);
            b[k] = alpha * blended + (1.0 - alpha) * b[k];
        }
        b[3] = alpha + ab * (1.0 - alpha);
    }
    b
}

fn read(store: &TileStore, x: u32, y: u32) -> [f32; 4] {
    store
        .tile_at((x / 256) as i32, (y / 256) as i32)
        .map_or([0.0; 4], |t| {
            t.pixel((x % 256) as usize, (y % 256) as usize)
                .unwrap()
        })
}

fn bits(pixel: [f32; 4]) -> [u32; 4] {
    pixel.map(f32::to_bits)
}

fn grid() -> Vec<[f32; 4]> {
    let mut pixels = Vec::new();
    for a in [0.0, 1.0 / 256.0, 0.5, 255.0 / 256.0, 1.0] {
        for r in [0.0, a / 2.0, a] {
            for g in [0.0, a / 2.0, a] {
                for b in [0.0, a / 2.0, a] {
                    pixels.push([r, g, b, a]);
                }
            }
        }
    }
    pixels
}

#[test]
fn composite_50_layers_matches_dense_reference() {
    let output = composite_document(&recipe()).unwrap();
    let mut maximum = 0.0_f32;
    let mut location = (0, 0, 0);
    for y in 0..512 {
        for x in 0..768 {
            let actual = read(&output, x, y);
            let expected = reference(x, y);
            for k in 0..4 {
                assert!(actual[k].is_finite() && expected[k].is_finite());
                let diff = (actual[k] - expected[k]).abs();
                if diff > maximum {
                    maximum = diff;
                    location = (x, y, k);
                }
            }
        }
    }
    println!("composite_50_layers: max_abs_diff={maximum}");
    assert!(
        maximum <= 1e-5,
        "maximum at {location:?}; modes repeat {:?}",
        BlendMode::ALL
    );
}

#[test]
fn dropping_one_layer_changes_output() {
    let mut doc = recipe();
    let before = composite_document(&doc).unwrap();
    doc.remove_layer(49);
    let after = composite_document(&doc).unwrap();
    let mut maximum = 0.0_f32;
    for y in 0..512 {
        for x in 0..768 {
            let a = read(&before, x, y);
            let b = read(&after, x, y);
            for k in 0..4 {
                maximum = maximum.max((a[k] - b[k]).abs());
            }
        }
    }
    println!("dropping_one_layer: max_abs_diff={maximum}");
    assert!(maximum > 1e-2);
}

#[test]
fn blend_modes_map_in_order_and_normal_is_source_over() {
    for (j, mode) in BlendMode::ALL.into_iter().enumerate() {
        for (b, s) in [(0.2, 0.7), (0.7, 0.2), (0.6, 0.9)] {
            assert_eq!(mode.channel()(b, s).to_bits(), TABLE[j](b, s).to_bits());
        }
    }
    for b in grid() {
        for s in grid() {
            assert_eq!(
                bits(composite_pixel(b, s, BlendMode::Normal, 1.0)),
                bits(blend::source_over(b, s))
            );
        }
    }
}

fn full_tile() -> Tile {
    Tile::from_pixels(vec![[0.5, 0.25, 0.125, 1.0]; TILE_PIXEL_COUNT]).unwrap()
}

fn single_layer(positions: &[(i32, i32)], mode: BlendMode, mask: Option<Mask>) -> Layer {
    let mut tiles = TileStore::new();
    for &(x, y) in positions {
        tiles.write_tile(x, y, full_tile());
    }
    Layer::new(tiles, 1.0, mode, mask).unwrap()
}

#[test]
fn zero_coverage_leaves_the_backdrop_bit_exact() {
    let mut doc = Document::new(768, 512).unwrap();
    doc.push_layer(single_layer(&[(0, 0)], BlendMode::Normal, None));
    let base = composite_document(&doc).unwrap();
    for mode in BlendMode::ALL {
        for b in grid() {
            for s in grid() {
                assert_eq!(bits(composite_pixel(b, s, mode, 0.0)), bits(b));
            }
            assert_eq!(bits(composite_pixel(b, [0.0; 4], mode, 1.0)), bits(b));
        }
        for top in [
            single_layer(&[(0, 0)], mode, Some(Mask::new())),
            single_layer(&[(-1, 0), (3, 0)], mode, None),
        ] {
            let mut modified = doc.clone();
            modified.push_layer(top);
            let output = composite_document(&modified).unwrap();
            assert_eq!(base.len(), output.len());
            for y in 0..512 {
                for x in 0..768 {
                    assert_eq!(bits(read(&base, x, y)), bits(read(&output, x, y)));
                }
            }
        }
    }
}

#[test]
fn composite_output_stays_valid_premultiplied() {
    for mode in BlendMode::ALL {
        for coverage in [1.0, 0.5] {
            for b in grid() {
                for s in grid() {
                    let out = composite_pixel(b, s, mode, coverage);
                    assert!(out.iter().all(|v| v.is_finite()));
                    assert!((0.0..=1.0).contains(&out[3]));
                    assert!(out[..3].iter().all(|v| (0.0..=out[3]).contains(v)));
                }
            }
        }
    }
    assert!(composite_document(&recipe()).is_ok());
}

#[test]
fn composite_clamps_out_of_range_sums() {
    // White unassociated channels make Multiply's blend exactly 1. In real
    // arithmetic both colour and alpha equal as + ab * (1 - as), but the
    // three-term colour sum rounds one ULP above alpha for these inputs.
    // Independent f32 arithmetic gives colour 0x3f00fe82, alpha 0x3f00fe81.
    let ab = 1.0_f32 / 257.0;
    let source_alpha = 129.0_f32 / 257.0;
    let backdrop = [ab; 4];
    let source = [source_alpha; 4];
    let out = composite_pixel(backdrop, source, BlendMode::Multiply, 1.0);
    assert_eq!(out[3].to_bits(), 0x3f00_fe81);
    assert!(out.iter().all(|value| value.is_finite()));
    assert!((0.0..=1.0).contains(&out[3]));
    for colour in &out[..3] {
        assert!((0.0..=out[3]).contains(colour), "{out:?}");
        assert_eq!(colour.to_bits(), out[3].to_bits());
    }
}

#[test]
fn canvas_edges_and_outside_tiles_are_ignored() {
    let mut doc = Document::new(300, 10).unwrap();
    doc.push_layer(single_layer(
        &[(0, 0), (1, 0), (1, 1), (-1, 0), (2, 0)],
        BlendMode::Normal,
        None,
    ));
    let output = composite_document(&doc).unwrap();
    assert_eq!(output.len(), 2);
    let edge = output.tile_at(1, 0).unwrap();
    assert_eq!(edge.pixel(43, 9).unwrap(), [0.5, 0.25, 0.125, 1.0]);
    assert_eq!(edge.pixel(44, 0).unwrap(), [0.0; 4]);
    assert_eq!(output.tile_at(0, 0).unwrap().pixel(0, 10).unwrap(), [0.0; 4]);
    for (x, y) in [(1, 1), (-1, 0), (2, 0)] {
        assert!(output.tile_at(x, y).is_none());
    }
    assert_eq!(TILE_SIZE, 256);
}

#[test]
fn document_and_layer_refuse_bad_input() {
    for (width, height) in [(0, 512), (768, 0)] {
        assert!(matches!(
            Document::new(width, height),
            Err(LayerError::ZeroCanvas { width: w, height: h }) if w == width && h == height
        ));
    }
    for opacity in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1, 1.5] {
        assert!(matches!(
            Layer::new(TileStore::new(), opacity, BlendMode::Normal, None),
            Err(LayerError::InvalidOpacity)
        ));
    }
    for opacity in [0.0, 1.0] {
        assert!(Layer::new(TileStore::new(), opacity, BlendMode::Normal, None).is_ok());
    }
}
