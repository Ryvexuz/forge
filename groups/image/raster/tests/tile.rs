use raster::tile::{Tile, TileError, TileStore, TILE_PIXEL_COUNT, TILE_SIZE};

#[test]
fn tile_store_round_trip() {
    let palette = [
        [0.125, 0.25, 0.5, 0.75],
        [1.0, 0.0, 0.5, 1.0],
        [0.0; 4],
        [f32::from_bits(1), 0.0, 0.0, f32::from_bits(1)],
    ];
    let expected: Vec<_> = (0..TILE_PIXEL_COUNT)
        .map(|index| palette[index % palette.len()])
        .collect();
    let mut store = TileStore::new();
    store.write_tile(-7, 11, Tile::from_pixels(expected.clone()).unwrap());
    assert_eq!(store.len(), 1);
    let actual = store.tile_at(-7, 11).unwrap();
    // Compare bits as well as values, including every alpha channel.
    for (actual, expected) in actual.pixels().iter().zip(&expected) {
        assert_eq!(actual.map(f32::to_bits), expected.map(f32::to_bits));
    }
    assert_eq!(actual.pixels().len(), expected.len());
    assert!(store.tile_at(11, -7).is_none());

    let mut replacement = Tile::default();
    replacement
        .set_pixel(TILE_SIZE - 1, TILE_SIZE - 1, [0.25, 0.0, 0.5, 0.5])
        .unwrap();
    store.write_tile(-7, 11, replacement.clone());
    assert_eq!(store.tile_at(-7, 11), Some(&replacement));
    assert_eq!(store.len(), 1);
}

#[test]
fn tile_store_sparse_empty_costs_nothing() {
    let store = TileStore::new();
    let canvas_size = 16_384;
    for y in 0..canvas_size / TILE_SIZE {
        for x in 0..canvas_size / TILE_SIZE {
            assert!(store.tile_at(x as i32, y as i32).is_none());
        }
    }
    // len() reads the underlying map length, not a canvas-size estimate.
    assert_eq!(store.len(), 0);
    assert!(store.is_empty());
}

#[test]
fn empty_writes_remove_entries_and_extreme_coordinates_work() {
    let mut store = TileStore::new();
    store.write_tile(0, 0, Tile::default());
    assert_eq!(store.len(), 0);
    let mut painted = Tile::default();
    painted.set_pixel(0, 0, [0.0, 0.0, 0.0, 1.0]).unwrap();
    store.write_tile(i32::MIN, i32::MAX, painted.clone());
    store.write_tile(0, 0, painted.clone());
    assert_eq!(store.tile_at(i32::MIN, i32::MAX), Some(&painted));
    store.write_tile(i32::MIN, i32::MAX, Tile::default());
    assert!(store.tile_at(i32::MIN, i32::MAX).is_none());
    assert_eq!(store.len(), 1);
    assert_eq!(store.tile_at(0, 0), Some(&painted));
}

#[test]
fn invalid_pixels_and_coordinates_are_rejected_without_mutation() {
    assert_eq!(
        Tile::from_pixels(Vec::new()),
        Err(TileError::WrongPixelCount { actual: 0 })
    );
    assert_eq!(
        Tile::from_pixels(vec![[0.0; 4]; TILE_PIXEL_COUNT + 1]),
        Err(TileError::WrongPixelCount {
            actual: TILE_PIXEL_COUNT + 1
        })
    );
    let mut tile = Tile::default();
    for invalid in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, f32::INFINITY, 0.0, 1.0],
        [0.0, 0.0, f32::NEG_INFINITY, 1.0],
        [-f32::from_bits(1), 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.5],
        [0.0, 0.0, 0.0, -1.0],
        [0.0, 0.0, 0.0, f32::MAX],
        [0.0, 0.0, 0.0, f32::NAN],
    ] {
        assert_eq!(
            tile.set_pixel(0, 0, invalid),
            Err(TileError::InvalidPixel { index: 0 })
        );
        assert!(tile.is_empty());
        let mut pixels = vec![[0.0; 4]; TILE_PIXEL_COUNT];
        pixels[TILE_PIXEL_COUNT - 1] = invalid;
        assert_eq!(
            Tile::from_pixels(pixels),
            Err(TileError::InvalidPixel {
                index: TILE_PIXEL_COUNT - 1
            })
        );
    }
    for (x, y) in [(TILE_SIZE, 0), (0, TILE_SIZE), (usize::MAX, usize::MAX)] {
        let error = Err(TileError::OutOfBounds { x, y });
        assert_eq!(tile.set_pixel(x, y, [0.0; 4]), error);
        assert_eq!(tile.pixel(x, y), Err(TileError::OutOfBounds { x, y }));
        assert!(tile.is_empty());
    }
}
