use constraint_sketch::constraint::{Axis, Constraint};
use constraint_sketch::format::{parse_sketch, write_sketch, FormatError};

const FIXTURE: &str = include_str!("fixtures/sketch30_v1.txt");

#[test]
fn format_round_trip_byte_identical() {
    assert_eq!(FIXTURE.lines().count(), 48);
    assert!(!FIXTURE.contains('\r'));
    let sketch = parse_sketch(FIXTURE).unwrap();
    assert_eq!(sketch.targets.len(), 15);
    assert_eq!(sketch.start.len(), 15);
    assert_eq!(sketch.constraints.len(), 30);
    assert_eq!(
        &sketch.constraints[..3],
        &[
            Constraint::Fix {
                point: 0,
                axis: Axis::X,
                value: 0.0,
            },
            Constraint::Fix {
                point: 0,
                axis: Axis::Y,
                value: 0.0,
            },
            Constraint::Fix {
                point: 1,
                axis: Axis::Y,
                value: 0.0,
            },
        ]
    );
    assert_eq!(write_sketch(&sketch).as_bytes(), FIXTURE.as_bytes());
}

#[test]
fn format_rejects_unknown_version() {
    assert_eq!(
        parse_sketch(&FIXTURE.replacen("schema=1", "schema=2", 1)),
        Err(FormatError::UnknownVersion(2))
    );
}

#[test]
fn format_named_errors() {
    assert_eq!(parse_sketch(""), Err(FormatError::Empty));
    assert_eq!(
        parse_sketch("points 15\n..."),
        Err(FormatError::MissingHeader)
    );
    for (old, new, error) in [
        (
            "distance 0 1 5.000000000000",
            "distance 0 1",
            FormatError::WrongFieldCount { line: 22 },
        ),
        (
            "fix 0 x 0.000000000000",
            "fix 0 x NaN",
            FormatError::NonFinite { line: 19 },
        ),
        (
            "distance 0 1 5.000000000000",
            "distance 0 99 5.000000000000",
            FormatError::BadIndex { line: 22 },
        ),
        (
            "angle 12 13 13 14 -1.570796326795\n",
            "",
            FormatError::CountMismatch { line: 48 },
        ),
    ] {
        assert!(FIXTURE.contains(old));
        assert_eq!(parse_sketch(&FIXTURE.replacen(old, new, 1)), Err(error));
    }
}

#[test]
fn fixture_targets_satisfy_constraints() {
    let sketch = parse_sketch(FIXTURE).unwrap();
    assert_eq!(sketch.constraints.len(), 30);
    for (i, constraint) in sketch.constraints.iter().enumerate() {
        let residuals = constraint.residual(&sketch.targets);
        assert!(!residuals.is_empty());
        for residual in residuals {
            assert!(residual.abs() <= 1e-9, "constraint {i}: {residual}");
        }
    }
}

#[test]
fn format_hostile_inputs() {
    for (text, error) in [
        ("schema=1\npoints +1\n", FormatError::BadSection { line: 2 }),
        (
            "schema=1\npoints 0\nconstraints 0\nextra\n",
            FormatError::CountMismatch { line: 4 },
        ),
        (
            "schema=1\npoints 0\nconstraints 0",
            FormatError::BadNumber { line: 3 },
        ),
        ("schema=1\r\n", FormatError::BadNumber { line: 1 }),
        (
            "schema=1\npoints 1\np 1 0 0 0 0\n",
            FormatError::BadIndex { line: 3 },
        ),
        (
            "schema=1\npoints 1\np 0 inf 0 0 0\n",
            FormatError::NonFinite { line: 3 },
        ),
        (
            "schema=1\npoints 1\np 0 1e999 0 0 0\n",
            FormatError::NonFinite { line: 3 },
        ),
        (
            "schema=1\npoints 0\nconstraints 1\n# comment\n",
            FormatError::UnknownKind { line: 4 },
        ),
        (
            "schema=1\npoints 1\np 0  0 0 0 0\n",
            FormatError::WrongFieldCount { line: 3 },
        ),
    ] {
        assert_eq!(parse_sketch(text), Err(error), "{text:?}");
    }
    let huge = "9".repeat(100_000);
    assert_eq!(
        parse_sketch(&format!("schema=1\npoints {huge}\n")),
        Err(FormatError::BadSection { line: 2 })
    );
    let zero = "schema=1\npoints 0\nconstraints 0\n";
    assert_eq!(write_sketch(&parse_sketch(zero).unwrap()), zero);
    for (value, expected) in [
        ("-0", -0.0_f64),
        ("5e-324", f64::from_bits(1)),
        ("1.7976931348623157e308", f64::MAX),
        ("-1e-300", -1e-300),
    ] {
        let text = format!("schema=1\npoints 1\np 0 {value} 0 0 0\nconstraints 0\n");
        let sketch = parse_sketch(&text).unwrap();
        assert_eq!(sketch.targets[0].x.to_bits(), expected.to_bits());
    }
}
