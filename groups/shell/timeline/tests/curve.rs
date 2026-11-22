use timeline::curve::{Curve, CurveError};
use timeline::time::Time;
use timeline::track::{Interp, Keyframe};

fn keys(a: f64, b: f64, interp: Interp) -> Vec<Keyframe> {
    vec![
        Keyframe {
            time: Time(-600),
            value: a,
            interp,
            handles: Some([(-400.0, -1.0), (400.0, 3.0)]),
        },
        Keyframe {
            time: Time(600),
            value: b,
            interp,
            handles: Some([(-400.0, -1.0), (400.0, 3.0)]),
        },
    ]
}

#[test]
fn curve_matches_independent_golden() {
    let mut lines = include_str!("fixtures/curve_golden.csv").lines();
    assert_eq!(lines.next(), Some("pair,mode,sample,value"));
    let pairs = [(0.0, 1.0), (-7.0, 4.0), (12.0, -3.0)];
    for (pair, (a, b)) in pairs.into_iter().enumerate() {
        for (mode, interp) in [
            ("step", Interp::Step),
            ("linear", Interp::Linear),
            ("bezier", Interp::Bezier),
            ("hermite", Interp::Hermite),
        ] {
            let curve = Curve::new(keys(a, b, interp)).unwrap();
            for sample in 0..=1000 {
                let row = lines.next().expect("missing golden sample");
                let fields: Vec<_> = row.split(',').collect();
                assert_eq!(fields.len(), 4);
                assert_eq!(fields[0].parse::<usize>().unwrap(), pair);
                assert_eq!(fields[1], mode);
                assert_eq!(fields[2].parse::<usize>().unwrap(), sample);
                let expected = fields[3].parse::<f64>().unwrap();
                let actual = curve.evaluate(-600.0 + sample as f64 * 1.2).unwrap();
                assert!(expected.is_finite());
                assert!(
                    (actual - expected).abs() <= 1e-9,
                    "pair={pair} mode={mode} sample={sample}: actual={actual} expected={expected}"
                );
            }
        }
    }
    assert_eq!(lines.next(), None);
}

#[test]
fn curve_rejects_bad_input() {
    assert_eq!(Curve::new(vec![]).unwrap_err(), CurveError::Empty);
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for index in 0..2 {
            let mut input = keys(0.0, 1.0, Interp::Linear);
            input[index].value = bad;
            assert_eq!(Curve::new(input).unwrap_err(), CurveError::NonFiniteValue);
        }
        let curve = Curve::new(keys(0.0, 1.0, Interp::Linear)).unwrap();
        assert_eq!(curve.evaluate(bad), Err(CurveError::NonFinitePosition));
        let mut input = keys(0.0, 1.0, Interp::Bezier);
        input[1].handles.as_mut().unwrap()[0].1 = bad;
        assert_eq!(Curve::new(input).unwrap_err(), CurveError::InvalidHandle);
    }
    for (time, error) in [
        (-600, CurveError::DuplicateTime),
        (-601, CurveError::OutOfOrder),
    ] {
        let mut input = keys(0.0, 1.0, Interp::Linear);
        input[1].time = Time(time);
        assert_eq!(Curve::new(input).unwrap_err(), error);
    }
    for (index, handle, x) in [(0, 1, -1.0), (0, 1, 1201.0), (1, 0, 1.0)] {
        let mut input = keys(0.0, 1.0, Interp::Bezier);
        input[index].handles.as_mut().unwrap()[handle].0 = x;
        assert_eq!(Curve::new(input).unwrap_err(), CurveError::InvalidHandle);
    }
    for interp in [
        Interp::Step,
        Interp::Linear,
        Interp::Bezier,
        Interp::Hermite,
    ] {
        let curve = Curve::new(keys(-7.0, 4.0, interp)).unwrap();
        assert_eq!(curve.evaluate(-f64::MAX), Ok(-7.0));
        assert_eq!(curve.evaluate(f64::MAX), Ok(4.0));
        assert_eq!(curve.evaluate(-600.0), Ok(-7.0));
        assert_eq!(curve.evaluate(600.0), Ok(4.0));
    }
    for interp in [Interp::Bezier, Interp::Hermite] {
        let mut input = keys(0.0, 1.0, interp);
        input[0].handles = None;
        assert_eq!(Curve::new(input).unwrap_err(), CurveError::MissingHandles);
    }
}

#[test]
fn curve_solves_nonlinear_bezier_x() {
    let mut input = keys(0.0, 1.0, Interp::Bezier);
    input[0].handles = Some([(0.0, 0.0), (0.0, 1.0 / 3.0)]);
    input[1].handles = Some([(-1200.0, 2.0 / 3.0), (0.0, 0.0)]);
    let curve = Curve::new(input).unwrap();
    // x(t)=t^3, y(t)=t, so x=1/8 must produce y=1/2.
    assert!((curve.evaluate(-450.0).unwrap() - 0.5).abs() <= 1e-9);
}

#[test]
fn curve_numeric_edges_fail_closed() {
    for (a, b) in [
        (f64::MAX, -f64::MAX),
        (f64::MIN_POSITIVE, -f64::MIN_POSITIVE),
        (f64::from_bits(1), -f64::from_bits(1)),
    ] {
        let curve = Curve::new(keys(a, b, Interp::Linear)).unwrap();
        // Exact symmetry gives a zero midpoint independently of magnitude.
        assert_eq!(curve.evaluate(0.0), Ok(0.0));
        assert_eq!(curve.evaluate(-600.0), Ok(a));
        assert_eq!(curve.evaluate(600.0), Ok(b));
    }
    let mut input = keys(0.0, 1.0, Interp::Hermite);
    input[0].handles.as_mut().unwrap()[1] = (f64::from_bits(1), f64::MAX);
    assert_eq!(Curve::new(input).unwrap_err(), CurveError::NumericRange);
    let mut input = keys(0.0, 1.0, Interp::Hermite);
    input[0].handles.as_mut().unwrap()[1].0 = 0.0;
    assert_eq!(Curve::new(input).unwrap_err(), CurveError::InvalidHandle);
    let mut input = keys(0.0, 1.0, Interp::Linear);
    input[0].time = Time(i64::MAX - 1);
    input[1].time = Time(i64::MAX);
    assert_eq!(Curve::new(input).unwrap_err(), CurveError::NumericRange);
}
