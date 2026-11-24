use constraint_sketch::constraint::{Axis, Constraint};
use constraint_sketch::dof::{self, analyse, analyse_at, Analysis, DofError, Status};
use constraint_sketch::format::Sketch;
use constraint_sketch::geom::Point;
use constraint_sketch::solve::{householder_qr, SolveError};

fn fix(point: usize, axis: Axis, value: f64) -> Constraint {
    Constraint::Fix { point, axis, value }
}

fn distance(a: usize, b: usize, distance: f64) -> Constraint {
    Constraint::Distance { a, b, distance }
}

fn sketch(targets: Vec<Point>, constraints: Vec<Constraint>) -> Sketch {
    Sketch {
        start: targets.clone(),
        targets,
        constraints,
    }
}

fn triangle() -> Sketch {
    sketch(
        vec![
            Point { x: 0.0, y: 0.0 },
            Point { x: 3.0, y: 0.0 },
            Point { x: 0.0, y: 4.0 },
        ],
        vec![
            fix(0, Axis::X, 0.0),
            fix(0, Axis::Y, 0.0),
            fix(1, Axis::Y, 0.0),
            distance(0, 1, 3.0),
            distance(0, 2, 4.0),
            distance(1, 2, 5.0),
        ],
    )
}

fn expected(
    unknowns: usize,
    equations: usize,
    rank: usize,
    status: Status,
    redundant: Vec<usize>,
    residual_inf: f64,
) -> Analysis {
    Analysis {
        status,
        unknowns,
        equations,
        rank,
        dof: unknowns - rank,
        redundant,
        residual_inf,
    }
}

#[test]
fn dof_counts_match_reference() {
    let mut s = triangle();
    assert_eq!(
        analyse(&s),
        Ok(expected(6, 6, 6, Status::FullyConstrained, vec![], 0.0))
    );
    s.constraints.remove(2);
    assert_eq!(
        analyse(&s),
        Ok(expected(6, 5, 5, Status::Underconstrained, vec![], 0.0))
    );
    let mut s = triangle();
    s.constraints.push(distance(0, 1, 3.0));
    assert_eq!(
        analyse(&s),
        Ok(expected(6, 7, 6, Status::Overconstrained, vec![6], 0.0))
    );
    let s = sketch(
        vec![Point { x: 1.0, y: 2.0 }; 2],
        vec![
            fix(0, Axis::X, 1.0),
            fix(0, Axis::Y, 2.0),
            Constraint::Coincident { a: 0, b: 1 },
        ],
    );
    assert_eq!(
        analyse(&s),
        Ok(expected(4, 4, 4, Status::FullyConstrained, vec![], 0.0))
    );
    assert_eq!(Status::FullyConstrained.as_str(), "fully_constrained");
    assert_eq!(Status::Underconstrained.as_str(), "underconstrained");
    assert_eq!(Status::Overconstrained.as_str(), "overconstrained");
}

#[test]
fn overconstrained_detected_with_names() {
    let mut s = triangle();
    // Inserting at 3 shifts the original Distance(0,2) from 4 to 5.
    s.constraints.insert(3, distance(0, 2, 4.0));
    assert_eq!(
        analyse(&s),
        Ok(expected(6, 7, 6, Status::Overconstrained, vec![5], 0.0))
    );
    let s = sketch(
        vec![Point { x: 0.0, y: 0.0 }, Point { x: 0.0, y: 1.0 }],
        vec![
            fix(0, Axis::X, 0.0),
            fix(0, Axis::Y, 0.0),
            fix(1, Axis::X, 0.0),
            Constraint::Coincident { a: 0, b: 1 },
        ],
    );
    assert_eq!(
        analyse(&s),
        Ok(expected(4, 5, 4, Status::Overconstrained, vec![3], 1.0))
    );
}

#[test]
fn rotation_alone_leaves_one_dof_at_the_solution() {
    let mut s = triangle();
    s.constraints.remove(2);
    s.start[2] = Point { x: 1.5, y: 0.0 };
    assert_eq!(
        analyse(&s),
        Ok(expected(6, 5, 5, Status::Underconstrained, vec![], 0.0))
    );
    // Two fixes and two independent x distances give rank 4.
    // Row 4 = row 2 - row 3 at this collinear start, leaving two DOF.
    assert_eq!(
        analyse_at(&s.start, &s.constraints),
        Ok(expected(6, 5, 4, Status::Overconstrained, vec![4], 3.5))
    );
}

#[test]
fn consistent_and_conflicting_duplicates() {
    for (target, residual) in [(3.0, 0.0), (4.0, 1.0)] {
        let mut s = triangle();
        s.constraints.push(distance(0, 1, target));
        assert_eq!(
            analyse(&s),
            Ok(expected(6, 7, 6, Status::Overconstrained, vec![6], residual))
        );
    }
}

#[test]
fn empty_and_free_sketches() {
    assert_eq!(
        analyse(&sketch(vec![], vec![])),
        Ok(expected(0, 0, 0, Status::FullyConstrained, vec![], 0.0))
    );
    assert_eq!(
        analyse(&sketch(triangle().targets, vec![])),
        Ok(expected(6, 0, 0, Status::Underconstrained, vec![], 0.0))
    );
}

#[test]
fn invalid_sketch_is_an_error_not_a_panic() {
    let p = Point { x: 0.0, y: 0.0 };
    for s in [
        sketch(vec![p], vec![distance(0, 1, 1.0)]),
        sketch(vec![p, p], vec![distance(0, 1, 0.0)]),
        sketch(
            vec![Point {
                x: f64::NAN,
                y: 0.0,
            }],
            vec![],
        ),
        sketch(vec![Point { x: 0.0, y: 1e101 }], vec![]),
        sketch(
            vec![Point {
                x: f64::INFINITY,
                y: 0.0,
            }],
            vec![],
        ),
        sketch(vec![p], vec![fix(0, Axis::X, f64::NAN)]),
    ] {
        assert_eq!(analyse(&s), Err(DofError::InvalidInput));
        assert_eq!(
            analyse_at(&s.targets, &s.constraints),
            Err(DofError::InvalidInput)
        );
    }
}

#[test]
fn rank_tolerance_matches_solver_policy() {
    assert_eq!(dof::RANK_TOLERANCE, 1e-12);
    for (e, rank, status, redundant) in [
        (1e-9_f64, 4, Status::FullyConstrained, vec![]),
        (1e-14_f64, 3, Status::Overconstrained, vec![3]),
    ] {
        let s = sketch(
            vec![Point { x: 0.0, y: 0.0 }, Point { x: e, y: 1.0 }],
            vec![
                fix(0, Axis::X, 0.0),
                fix(0, Axis::Y, 0.0),
                fix(1, Axis::Y, 1.0),
                distance(0, 1, (e * e + 1.0).sqrt()),
            ],
        );
        assert_eq!(
            analyse(&s),
            Ok(expected(4, 4, rank, status, redundant, 0.0))
        );
        let rows = s
            .constraints
            .iter()
            .flat_map(|c| c.jacobian_rows(&s.targets))
            .collect();
        let solved = householder_qr(rows, vec![0.0; 4], dof::RANK_TOLERANCE);
        if rank == 4 {
            assert_eq!(solved, Ok(vec![0.0; 4]));
        } else {
            assert_eq!(solved, Err(SolveError::RankDeficient));
        }
    }
}

#[test]
fn zero_rows_and_coordinate_boundaries() {
    // Exact coordinate fixes have the identity Jacobian at every scale.
    // The self-coincidence contributes two zero rows before either fix.
    for coordinate in [0.0, f64::from_bits(1), 1e-100, -1e100, 1e100] {
        let s = sketch(
            vec![Point {
                x: coordinate,
                y: coordinate,
            }],
            vec![
                Constraint::Coincident { a: 0, b: 0 },
                fix(0, Axis::X, coordinate),
                fix(0, Axis::Y, coordinate),
            ],
        );
        let original = s.clone();
        let reference = Ok(expected(2, 4, 2, Status::Overconstrained, vec![0], 0.0));
        assert_eq!(analyse(&s), reference);
        assert_eq!(analyse_at(&s.targets, &s.constraints), reference);
        assert_eq!(s, original);
    }
}
