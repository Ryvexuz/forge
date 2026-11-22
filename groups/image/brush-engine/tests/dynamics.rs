use brush_engine::pressure::{DynamicsError, PressureCurve, TiltEllipse};

#[test]
fn pressure_maps_size_and_opacity() {
    let curve = PressureCurve;
    let mut previous = curve.sample(0.0).unwrap();
    for raw in 0..=1024 {
        let pressure = raw as f32 / 1024.0;
        let actual = curve.sample(pressure).unwrap();
        // Independent closed-form reference for the specified linear design.
        assert_eq!(actual.size, (1.0 + 63.0 * f64::from(pressure)) as f32);
        assert_eq!(actual.opacity, pressure);
        assert!(actual.size >= previous.size);
        assert!(actual.opacity >= previous.opacity);
        previous = actual;
    }
    // Literal expectations deliberately do not read the implementation's table:
    // mutating its maximum size must fail this test.
    for (pressure, size, opacity) in [(0.0, 1.0, 0.0), (0.5, 32.5, 0.5), (1.0, 64.0, 1.0)] {
        let actual = curve.sample(pressure).unwrap();
        assert_eq!(actual.size, size);
        assert_eq!(actual.opacity, opacity);
    }
}

#[test]
fn tilt_changes_ellipse_angle() {
    let round = TiltEllipse::from_degrees(0.0, 0.0, 0.0).unwrap();
    assert_eq!(round.major_axis, round.minor_axis);
    for (x, y, direction) in [(60.0, 0.0, 0.0), (0.0, 60.0, 90.0)] {
        for rotation in 0..=359 {
            let ellipse = TiltEllipse::from_degrees(x, y, rotation as f32).unwrap();
            assert_eq!(ellipse.major_axis, 1.0);
            assert_eq!(ellipse.minor_axis, 0.5);
            assert!(ellipse.minor_axis < ellipse.major_axis);
            let expected = (direction + f64::from(rotation)).rem_euclid(360.0);
            let delta =
                (f64::from(ellipse.angle_degrees) - expected + 180.0).rem_euclid(360.0) - 180.0;
            assert!(delta.abs() <= 1.0, "angle error: {delta} degrees");
        }
    }
}

#[test]
fn dynamics_reject_invalid_inputs() {
    for pressure in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
        assert_eq!(
            PressureCurve.sample(pressure),
            Err(DynamicsError::InvalidPressure)
        );
    }
    for tilt in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, 90.01] {
        for (x, y) in [(tilt, 0.0), (0.0, tilt)] {
            assert_eq!(
                TiltEllipse::from_degrees(x, y, 0.0),
                Err(DynamicsError::InvalidTilt)
            );
        }
    }
    assert_eq!(
        TiltEllipse::from_degrees(90.0, 90.0, 0.0),
        Err(DynamicsError::InvalidTilt)
    );
    for rotation in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 360.0] {
        assert_eq!(
            TiltEllipse::from_degrees(0.0, 0.0, rotation),
            Err(DynamicsError::InvalidRotation)
        );
    }
}

#[test]
fn numeric_boundaries_remain_finite() {
    for pressure in [
        0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::from_bits(0.5_f32.to_bits() - 1),
        f32::from_bits(0.5_f32.to_bits() + 1),
        f32::from_bits(1.0_f32.to_bits() - 1),
        1.0,
    ] {
        let actual = PressureCurve.sample(pressure).unwrap();
        assert_eq!(actual.size, (1.0 + 63.0 * f64::from(pressure)) as f32);
        assert_eq!(actual.opacity, pressure);
    }
    for (x, y, expected_minor, expected_angle) in [
        (0.0, 0.0, 1.0, 0.0),
        (f32::from_bits(1), 0.0, 1.0, 0.0),
        (0.0, f32::MIN_POSITIVE, 1.0, 90.0),
        (90.0, 0.0, 0.0, 0.0),
        (-90.0, 0.0, 0.0, 180.0),
        (0.0, -90.0, 0.0, 270.0),
    ] {
        let ellipse = TiltEllipse::from_degrees(x, y, 0.0).unwrap();
        assert_eq!(ellipse.major_axis, 1.0);
        assert_eq!(ellipse.minor_axis, expected_minor);
        assert_eq!(ellipse.angle_degrees, expected_angle);
    }
}
