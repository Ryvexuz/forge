use brush_engine::stabilizer::{
    Point, Stabilizer, StabilizerError, DEFAULT_WINDOW, MAX_COORDINATE,
};

#[test]
fn stabilizer_lag_bounded() {
    let mut stabilizer = Stabilizer::new();
    assert_eq!(DEFAULT_WINDOW, 16);
    // The requested 16 px / 1 px limits are estimates, not measurements.
    // A 16-report window at an estimated 200 Hz spans about 80 ms.
    // An endpoint linear fit avoids the 75 px lag a box average would have
    // at 10 px/report; the prescribed 16 px bound is kept unchanged.
    let mut last = Point { x: 0.0, y: 0.0 };
    for i in 0..500 {
        last = Point {
            x: f64::from(i) * 10.0,
            y: 0.0,
        };
        let output = stabilizer.push(last).unwrap();
        if i >= 16 {
            assert!((output.x - last.x).hypot(output.y - last.y) <= 16.0);
        }
    }
    let endpoint = stabilizer.pen_up().expect("pen-up must emit endpoint");
    assert!((endpoint.x - last.x).hypot(endpoint.y - last.y) <= 1.0);
}

#[test]
fn flush_is_exact_at_a_corner_and_resets() {
    let mut stabilizer = Stabilizer::default();
    assert_eq!(stabilizer.pen_up(), None);
    for _ in 0..DEFAULT_WINDOW {
        stabilizer.push(Point { x: 0.0, y: 0.0 }).unwrap();
    }
    let corner = Point { x: 100.0, y: -50.0 };
    assert_ne!(stabilizer.push(corner).unwrap(), corner);
    assert_eq!(stabilizer.pen_up(), Some(corner));
    assert_eq!(stabilizer.pen_up(), None);
    let next = Point { x: -7.0, y: 3.0 };
    assert_eq!(stabilizer.push(next).unwrap(), next);
}

#[test]
fn invalid_reports_do_not_change_the_endpoint() {
    let mut stabilizer = Stabilizer::new();
    let valid = Point { x: 2.0, y: -3.0 };
    stabilizer.push(valid).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for point in [Point { x: bad, y: 0.0 }, Point { x: 0.0, y: bad }] {
            assert_eq!(
                stabilizer.push(point),
                Err(StabilizerError::NonFiniteCoordinate)
            );
        }
    }
    for bad in [f64::MAX, -f64::MAX] {
        assert_eq!(
            stabilizer.push(Point { x: bad, y: 0.0 }),
            Err(StabilizerError::CoordinateOutOfRange)
        );
    }
    assert_eq!(stabilizer.pen_up(), Some(valid));
}

#[test]
fn numeric_band_matches_two_sample_endpoint_reference() {
    // For two reports the exact least-squares endpoint is the second report.
    // This analytic reference is independent of the implementation's weights.
    for magnitude in [
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::from(f32::from_bits(1)),
        1.0,
        MAX_COORDINATE,
    ] {
        let mut stabilizer = Stabilizer::new();
        stabilizer
            .push(Point {
                x: magnitude,
                y: -magnitude,
            })
            .unwrap();
        let expected = Point {
            x: -magnitude,
            y: magnitude,
        };
        assert_eq!(stabilizer.push(expected).unwrap(), expected);
        assert_eq!(stabilizer.pen_up(), Some(expected));
    }
}
