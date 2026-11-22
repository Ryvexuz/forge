use scene::instance::{matrix_from_angle, InstanceError, Instances};

#[test]
fn instances_update_matches_reference() {
    let mut instances = Instances::with_capacity(1000);
    let mut reference = Vec::with_capacity(1000);
    for i in 0..1000_u32 {
        let angle = (i as f32 - 500.0) / 128.0;
        let speed = (i as f32 - 499.0) / 8192.0;
        instances.push(i % 7, i, angle, speed).unwrap();
        reference.push((angle, speed));
    }
    for frame in 0..10 {
        instances.update_frame(frame).unwrap();
        // Exactly one f32 addition, then separate f32 sin and cos calls.
        // No wrapping, multiplication by frame, or fused operations.
        for (i, (angle, speed)) in reference.iter_mut().enumerate() {
            *angle += *speed;
            let s = angle.sin();
            let c = angle.cos();
            let expected = [c, 0.0, s, 0.0, 0.0, 1.0, 0.0, 0.0, -s, 0.0, c, 0.0];
            for (element, expected) in expected.iter().enumerate() {
                assert_eq!(
                    instances.matrices()[i][element],
                    *expected,
                    "frame {frame}, instance {i}, element {element}"
                );
            }
            assert_eq!(instances.angles()[i], *angle);
            assert_eq!(instances.speeds()[i], *speed);
            assert_eq!(instances.layers()[i], i as u32 % 7);
            assert_eq!(instances.nodes()[i], i as u32);
        }
    }
}

#[test]
fn invalid_inputs_and_overflow_leave_store_unchanged() {
    let mut instances = Instances::with_capacity(0);
    instances.update_frame(0).unwrap();
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert_eq!(
            matrix_from_angle(invalid),
            Err(InstanceError::NonFiniteAngle)
        );
        assert_eq!(
            instances.push(0, 0, invalid, 0.0),
            Err(InstanceError::NonFiniteAngle)
        );
        assert_eq!(
            instances.push(0, 0, 0.0, invalid),
            Err(InstanceError::NonFiniteSpeed)
        );
        assert!(instances.matrices().is_empty());
        assert!(instances.angles().is_empty());
        assert!(instances.speeds().is_empty());
        assert!(instances.layers().is_empty());
        assert!(instances.nodes().is_empty());
    }
    instances.push(1, 2, 0.0, 1.0).unwrap();
    instances.push(3, 4, f32::MAX, f32::MAX).unwrap();
    let before = instances.matrices().to_vec();
    assert_eq!(
        instances.update_frame(u32::MAX),
        Err(InstanceError::AngleOverflow)
    );
    assert_eq!(instances.matrices(), before);
    assert_eq!(instances.angles(), [0.0, f32::MAX]);
}

#[test]
fn finite_extremes_and_cancellation() {
    let tiny = f32::from_bits(1);
    for (angle, speed) in [
        (tiny, tiny),
        (-tiny, tiny),
        (f32::MIN_POSITIVE, -tiny),
        (f32::MAX, -f32::MAX),
        (-f32::MAX, f32::MAX),
        (f32::MAX, 0.0),
    ] {
        let mut instances = Instances::with_capacity(1);
        instances.push(u32::MAX, u32::MAX, angle, speed).unwrap();
        instances.update_frame(u32::MAX).unwrap();
        let expected_angle = (f64::from(angle) + f64::from(speed)) as f32;
        assert_eq!(instances.angles(), [expected_angle]);
        let s = expected_angle.sin();
        let c = expected_angle.cos();
        assert_eq!(
            instances.matrices()[0],
            [c, 0.0, s, 0.0, 0.0, 1.0, 0.0, 0.0, -s, 0.0, c, 0.0]
        );
    }
}
