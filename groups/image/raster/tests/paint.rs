use std::collections::BTreeSet;

use raster::paint::stamp_line;
use raster::tile::{TileError, TileStore};

const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];

#[test]
fn stroke_marks_only_dirty_tiles() {
    let mut store = TileStore::new();
    let dirty = stamp_line(&mut store, (100, 100), (600, 100), 8, RED).unwrap();
    assert_eq!(dirty, BTreeSet::from([(0, 0), (1, 0), (2, 0)]));
    assert_eq!(store.len(), 3);
    let tile = store.tile_at(0, 0).unwrap();
    assert_eq!(tile.pixel(92, 100).unwrap(), RED);
    assert_eq!(tile.pixel(100, 108).unwrap(), RED);
    assert_eq!(tile.pixel(92, 92).unwrap(), [0.0; 4]);
    assert_eq!(tile.pixel(100, 109).unwrap(), [0.0; 4]);
    assert!(stamp_line(&mut store, (600, 100), (100, 100), 8, RED)
        .unwrap()
        .is_empty());
}

#[test]
fn disk_crosses_negative_tile_boundaries() {
    let mut store = TileStore::new();
    let dirty = stamp_line(&mut store, (0, 0), (0, 0), 1, RED).unwrap();
    assert_eq!(dirty, BTreeSet::from([(-1, 0), (0, -1), (0, 0)]));
    assert!(store.tile_at(-1, -1).is_none());
    assert_eq!(store.tile_at(-1, 0).unwrap().pixel(255, 0).unwrap(), RED);
}

#[test]
fn diagonal_has_round_caps_and_blends_once() {
    let mut store = TileStore::new();
    let colour = [0.5, 0.0, 0.0, 0.5];
    stamp_line(&mut store, (10, 10), (20, 20), 1, colour).unwrap();
    let tile = store.tile_at(0, 0).unwrap();
    assert_eq!(tile.pixel(15, 15).unwrap(), colour);
    assert_eq!(tile.pixel(15, 16).unwrap(), colour);
    assert_eq!(tile.pixel(15, 17).unwrap(), [0.0; 4]);
    assert_eq!(tile.pixel(9, 9).unwrap(), [0.0; 4]);
    stamp_line(&mut store, (10, 10), (20, 20), 1, colour).unwrap();
    assert_eq!(
        store.tile_at(0, 0).unwrap().pixel(15, 15).unwrap(),
        [0.75, 0.0, 0.0, 0.75]
    );
}

#[test]
fn invalid_colour_is_atomic_and_transparent_strokes_allocate_nothing() {
    let mut store = TileStore::new();
    for colour in [
        [f32::NAN, 0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0, f32::INFINITY],
        [-0.1, 0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0, 0.5],
    ] {
        assert_eq!(
            stamp_line(&mut store, (0, 0), (10, 10), 1, colour),
            Err(TileError::InvalidPixel { index: 0 })
        );
        assert!(store.is_empty());
    }
    assert!(stamp_line(&mut store, (0, 0), (10, 10), 1, [0.0; 4])
        .unwrap()
        .is_empty());
    assert!(store.is_empty());
}

#[test]
fn zero_radius_at_coordinate_extremes_and_subnormal_colour() {
    let mut store = TileStore::new();
    let tiny = f32::from_bits(1);
    let colour = [tiny, 0.0, 0.0, tiny];
    for point in [(i32::MIN, i32::MIN), (i32::MAX, i32::MAX)] {
        let dirty = stamp_line(&mut store, point, point, 0, colour).unwrap();
        let coord = (point.0.div_euclid(256), point.1.div_euclid(256));
        assert_eq!(dirty, BTreeSet::from([coord]));
        assert_eq!(
            store
                .tile_at(coord.0, coord.1)
                .unwrap()
                .pixel(
                    point.0.rem_euclid(256) as usize,
                    point.1.rem_euclid(256) as usize
                )
                .unwrap(),
            colour
        );
    }
}
