use constraint_sketch::constraint::Constraint;
use constraint_sketch::dof::{Status, analyse, analyse_at};
use constraint_sketch::format::parse_sketch;
use constraint_sketch::solve::{SolveOptions, solve};

const FIXTURE: &str = include_str!("fixtures/sketch30_v1.txt");
const POSITION_TOLERANCE: f64 = 1e-9;

fn options() -> SolveOptions {
    SolveOptions {
        max_iterations: 64,
        residual_tolerance: 1e-12,
        rank_tolerance: 1e-12,
    }
}

#[test]
fn solve_30_constraint_reference_converges() {
    let sketch = parse_sketch(FIXTURE).expect("valid reference fixture");
    assert_eq!(sketch.targets.len(), 15);
    assert_eq!(sketch.start.len(), 15);
    assert_eq!(sketch.constraints.len(), 30);
    for constraint in &sketch.constraints[..3] {
        assert!(matches!(constraint, Constraint::Fix { .. }));
    }
    assert!(
        sketch.start.iter().zip(&sketch.targets).any(|(a, b)| {
            (a.x - b.x).abs() > 0.05 || (a.y - b.y).abs() > 0.05
        }),
        "reference start must be perturbed"
    );
    let mut points = sketch.start.clone();
    assert_eq!(solve(&mut points, &sketch.constraints, options()), Ok(()));
    for (p, (point, target)) in points.iter().zip(&sketch.targets).enumerate() {
        for (coord, actual, expected) in
            [("x", point.x, target.x), ("y", point.y, target.y)]
        {
            let difference = (actual - expected).abs();
            assert!(
                actual.is_finite() && difference <= POSITION_TOLERANCE,
                "point {p} coord {coord} off={difference:e}"
            );
        }
    }
    let mut max_residual = 0.0_f64;
    for residual in sketch.constraints.iter().flat_map(|c| c.residual(&points)) {
        assert!(residual.is_finite(), "nonfinite residual");
        max_residual = max_residual.max(residual.abs());
    }
    assert!(
        max_residual <= options().residual_tolerance,
        "max residual={max_residual:e}"
    );
}

#[test]
fn reference30_is_fully_constrained_at_the_solution() {
    let sketch = parse_sketch(FIXTURE).expect("valid reference fixture");
    let reference = analyse(&sketch).expect("reference analysis");
    assert_eq!(reference.status, Status::FullyConstrained);
    assert_eq!(reference.unknowns, 30);
    assert_eq!(reference.equations, 30);
    assert_eq!(reference.rank, 30);
    assert_eq!(reference.dof, 0);
    assert!(reference.redundant.is_empty());
    let mut points = sketch.start.clone();
    assert_eq!(solve(&mut points, &sketch.constraints, options()), Ok(()));
    let solved = analyse_at(&points, &sketch.constraints).expect("solved analysis");
    assert_eq!(solved.status, reference.status);
    assert_eq!(solved.rank, reference.rank);
    assert_eq!(solved.dof, reference.dof);
}
