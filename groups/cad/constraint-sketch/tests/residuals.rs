use constraint_sketch::constraint::{Axis, Constraint};
use constraint_sketch::geom::Point;

fn cases() -> Vec<(Constraint, Vec<f64>)> {
    use Constraint::*;
    vec![
        (Coincident { a: 0, b: 1 }, vec![-3.0, -4.0]),
        (
            Distance {
                a: 0,
                b: 1,
                distance: 2.0,
            },
            vec![3.0],
        ),
        (
            Angle {
                a: 0,
                b: 2,
                c: 0,
                d: 3,
                angle: 0.5,
            },
            vec![std::f64::consts::FRAC_PI_2 - 0.5],
        ),
        (
            Parallel {
                a: 0,
                b: 2,
                c: 0,
                d: 3,
            },
            vec![6.0],
        ),
        (
            Tangent {
                start: 0,
                contact: 1,
                center: 2,
            },
            vec![19.0],
        ),
        (
            Fix {
                point: 1,
                axis: Axis::X,
                value: 1.0,
            },
            vec![2.0],
        ),
        (
            Fix {
                point: 1,
                axis: Axis::Y,
                value: 1.0,
            },
            vec![3.0],
        ),
    ]
}

#[test]
fn residuals_match_hand_values() {
    // (3,4) has length 5; (2,0) x (0,3) = 6;
    // (3,4) dot ((3,4)-(2,0)) = 3 + 16 = 19.
    let pts = [
        Point { x: 0.0, y: 0.0 },
        Point { x: 3.0, y: 4.0 },
        Point { x: 2.0, y: 0.0 },
        Point { x: 0.0, y: 3.0 },
    ];
    for (constraint, expected) in cases() {
        let actual = constraint.residual(&pts);
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.iter().zip(expected) {
            assert!(
                (actual - expected).abs() <= 1e-12,
                "{constraint:?}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn jacobian_matches_finite_difference() {
    // Deterministic pseudo-random points; no dependency or ambient RNG state.
    let mut state = 17_u32;
    for _ in 0..24 {
        let mut pts = [Point { x: 0.0, y: 0.0 }; 4];
        for p in &mut pts {
            let mut sample = || {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f64::from(state) / f64::from(u32::MAX) * 4.0 - 2.0
            };
            p.x = sample();
            p.y = sample();
        }
        for (constraint, _) in cases() {
            let rows = constraint.jacobian_rows(&pts);
            assert_eq!(rows.len(), constraint.residual(&pts).len());
            for column in 0..pts.len() * 2 {
                let mut plus = pts;
                let mut minus = pts;
                let h = 1e-6;
                if column % 2 == 0 {
                    plus[column / 2].x += h;
                    minus[column / 2].x -= h;
                } else {
                    plus[column / 2].y += h;
                    minus[column / 2].y -= h;
                }
                let rp = constraint.residual(&plus);
                let rm = constraint.residual(&minus);
                for (i, row) in rows.iter().enumerate() {
                    assert_eq!(row.len(), pts.len() * 2);
                    let numerical = (rp[i] - rm[i]) / (2.0 * h);
                    let analytic = row[column];
                    assert!(
                        (analytic - numerical).abs() <= 1e-6 * (1.0 + numerical.abs()),
                        "{constraint:?} row {i} column {column}: {analytic} != {numerical}"
                    );
                }
            }
        }
    }
}

#[test]
#[should_panic(expected = "invalid constraint input")]
fn rejects_degenerate_distance() {
    Constraint::Distance {
        a: 0,
        b: 0,
        distance: 0.0,
    }
    .jacobian_rows(&[Point { x: 0.0, y: 0.0 }]);
}

#[test]
#[should_panic(expected = "invalid constraint input")]
fn rejects_nonfinite_coordinate() {
    Constraint::Fix {
        point: 0,
        axis: Axis::X,
        value: 0.0,
    }
    .residual(&[Point {
        x: f64::NAN,
        y: 0.0,
    }]);
}
