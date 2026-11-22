use raster::blend;
use raster::mask::{compose_alpha, Mask, MaskError, Selection};
use raster::tile::TILE_PIXEL_COUNT;

#[test]
fn mask_and_selection_compose() {
    for layer in [0.0_f32, 0.5, 1.0] {
        for mask in [0.0_f32, 0.5, 1.0] {
            for selection in [0.0_f32, 0.5, 1.0] {
                let expected = (f64::from(layer) * f64::from(mask) * f64::from(selection)) as f32;
                assert_eq!(compose_alpha(layer, mask, selection), Ok(expected));
            }
        }
    }

    let modes: [fn(f32, f32) -> f32; 12] = [
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
    for mode in modes {
        let alpha = compose_alpha(1.0, 0.0, 1.0).unwrap();
        let channel = mode(0.5, 1.0) * alpha;
        let source = [channel, channel, channel, alpha];
        assert_eq!(blend::source_over([0.0; 4], source), [0.0; 4]);
        assert_eq!(blend::source_over([0.25; 4], source), [0.25; 4]);
    }
}

#[test]
fn sparse_planes_validate_before_replacing() {
    let mut mask = Mask::new();
    let mut selection = Selection::new();
    assert!(mask.is_empty());
    assert!(selection.is_empty());
    assert_eq!(mask.tile_at(0, 0), None);
    assert_eq!(selection.tile_at(0, 0), None);
    let coordinate = (i32::MIN, i32::MAX);
    mask.write_tile(coordinate.0, coordinate.1, vec![0.5; TILE_PIXEL_COUNT])
        .unwrap();
    selection
        .write_tile(coordinate.0, coordinate.1, vec![1.0; TILE_PIXEL_COUNT])
        .unwrap();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 2.0] {
        let mut values = vec![0.0; TILE_PIXEL_COUNT];
        values[TILE_PIXEL_COUNT - 1] = bad;
        let expected = Err(MaskError::InvalidValue {
            index: TILE_PIXEL_COUNT - 1,
        });
        assert_eq!(
            mask.write_tile(coordinate.0, coordinate.1, values.clone()),
            expected
        );
        assert_eq!(
            selection.write_tile(coordinate.0, coordinate.1, values),
            expected
        );
        for factors in [(bad, 1.0, 1.0), (1.0, bad, 1.0), (1.0, 1.0, bad)] {
            assert_eq!(
                compose_alpha(factors.0, factors.1, factors.2),
                Err(MaskError::InvalidAlpha)
            );
        }
    }
    assert!(selection
        .write_tile(0, 0, vec![0.5; TILE_PIXEL_COUNT])
        .is_err());
    for count in [0, TILE_PIXEL_COUNT - 1, TILE_PIXEL_COUNT + 1] {
        assert_eq!(
            mask.write_tile(0, 0, vec![0.0; count]),
            Err(MaskError::WrongValueCount { actual: count })
        );
        assert_eq!(
            selection.write_tile(0, 0, vec![0.0; count]),
            Err(MaskError::WrongValueCount { actual: count })
        );
    }
    assert_eq!(mask.len(), 1);
    assert_eq!(selection.len(), 1);
    assert_eq!(mask.tile_at(coordinate.0, coordinate.1).unwrap()[0], 0.5);
    assert_eq!(
        selection.tile_at(coordinate.0, coordinate.1).unwrap()[0],
        1.0
    );
    mask.write_tile(coordinate.0, coordinate.1, vec![0.0; TILE_PIXEL_COUNT])
        .unwrap();
    selection
        .write_tile(coordinate.0, coordinate.1, vec![0.0; TILE_PIXEL_COUNT])
        .unwrap();
    assert!(mask.is_empty());
    assert!(selection.is_empty());
}

#[test]
fn alpha_numeric_boundaries() {
    let tiny = f32::from_bits(1);
    for factors in [
        (tiny, 1.0, 1.0),
        (tiny, 0.5, 1.0),
        (f32::MIN_POSITIVE, 0.5, 1.0),
        (1.0, 1.0, 1.0),
        (f32::from_bits(1.0_f32.to_bits() - 1), 1.0, 1.0),
        (0.0, tiny, 1.0),
    ] {
        let expected = (f64::from(factors.0) * f64::from(factors.1) * f64::from(factors.2)) as f32;
        assert_eq!(compose_alpha(factors.0, factors.1, factors.2), Ok(expected));
    }
}
