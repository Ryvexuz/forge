use constraint_sketch::constraint::{Axis, Constraint};
use constraint_sketch::geom::Point;
use constraint_sketch::solve::{householder_qr, solve, SolveError, SolveOptions};

fn options() -> SolveOptions {
    SolveOptions {
        max_iterations: 64,
        residual_tolerance: 1e-10,
        rank_tolerance: 1e-12,
    }
}

fn triangle() -> (Vec<Point>, Vec<Constraint>) {
    (
        vec![
            Point { x: 0.2, y: -0.1 },
            Point { x: 3.5, y: 0.3 },
            Point { x: 0.4, y: 3.6 },
        ],
        vec![
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
            Constraint::Distance {
                a: 0,
                b: 1,
                distance: 3.0,
            },
            Constraint::Distance {
                a: 0,
                b: 2,
                distance: 4.0,
            },
            Constraint::Distance {
                a: 1,
                b: 2,
                distance: 5.0,
            },
        ],
    )
}

#[test]
fn solve_reference_triangle() {
    let (mut points, constraints) = triangle();
    solve(&mut points, &constraints, options()).unwrap();
    // The positive-y start selects the upper mirror of the analytic triangle.
    for (point, expected) in points.iter().zip([(0.0, 0.0), (3.0, 0.0), (0.0, 4.0)]) {
        assert!((point.x - expected.0).abs() <= options().residual_tolerance);
        assert!((point.y - expected.1).abs() <= options().residual_tolerance);
    }
}

#[test]
fn missing_anchor_is_rank_deficient() {
    let (mut points, constraints) = triangle();
    let original = points.clone();
    assert_eq!(
        solve(&mut points, &constraints[3..], options()),
        Err(SolveError::RankDeficient)
    );
    assert_eq!(points, original);
}

#[test]
fn iteration_exhaustion_is_transactional() {
    let (mut points, constraints) = triangle();
    let original = points.clone();
    let mut settings = options();
    settings.max_iterations = 1;
    assert_eq!(
        solve(&mut points, &constraints, settings),
        Err(SolveError::DidNotConverge)
    );
    assert_eq!(points, original);
}

#[test]
fn qr_known_overdetermined_solution_and_scaling() {
    // Exact independent solution x=(2,-1), with a redundant third equation.
    for scale in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX / 16.0] {
        let a = vec![vec![scale, 0.0], vec![0.0, scale], vec![scale, scale]];
        let b = vec![2.0 * scale, -scale, scale];
        let x = householder_qr(a, b, options().rank_tolerance).unwrap();
        assert!((x[0] - 2.0).abs() <= options().residual_tolerance);
        assert!((x[1] + 1.0).abs() <= options().residual_tolerance);
    }
}

#[test]
fn invalid_constraint_is_reported_without_applying_updates() {
    let (mut points, _) = triangle();
    let original = points.clone();
    let constraints = [Constraint::Fix {
        point: usize::MAX,
        axis: Axis::X,
        value: 0.0,
    }];
    assert_eq!(
        solve(&mut points, &constraints, options()),
        Err(SolveError::InvalidInput)
    );
    assert_eq!(points, original);
}

#[test]
fn qr_rejects_bad_inputs_and_dependent_columns() {
    assert_eq!(
        householder_qr(vec![vec![f64::NAN]], vec![1.0], 1e-12),
        Err(SolveError::InvalidInput)
    );
    assert_eq!(
        householder_qr(vec![vec![1.0], vec![]], vec![1.0, 1.0], 1e-12),
        Err(SolveError::InvalidInput)
    );
    assert_eq!(
        householder_qr(vec![vec![1.0, 1.0], vec![2.0, 2.0]], vec![1.0, 2.0], 1e-12),
        Err(SolveError::RankDeficient)
    );
}
