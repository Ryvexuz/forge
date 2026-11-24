use std::collections::HashSet;

use constraint_sketch::format::{Sketch, parse_sketch};
use constraint_sketch::geom::Point;
use constraint_sketch::solve::{SolveOptions, solve};

const FIXTURE: &str = include_str!("fixtures/sketch30_v1.txt");
const BASE_SEED: u64 = 0x0029_0008_5EED_0000;
const STARTS: u64 = 200;
const POSITION_TOLERANCE: f64 = 1e-9;

fn options() -> SolveOptions {
    SolveOptions {
        max_iterations: 64,
        residual_tolerance: 1e-12,
        rank_tolerance: 1e-12,
    }
}

struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in [-0.5, 0.5): 53 high bits to [0, 1), minus 0.5.
    fn offset(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64) - 0.5
    }
}

fn fixture() -> Sketch {
    let sketch = parse_sketch(FIXTURE).expect("valid 30-constraint fixture");
    assert_eq!(sketch.targets.len(), 15);
    assert_eq!(sketch.constraints.len(), 30);
    sketch
}

fn start(sketch: &Sketch, i: u64) -> Vec<Point> {
    let mut generator = SplitMix64(BASE_SEED + i);
    sketch
        .targets
        .iter()
        .map(|target| Point {
            x: target.x + generator.offset(),
            y: target.y + generator.offset(),
        })
        .collect()
}

fn bits(points: &[Point]) -> Vec<u64> {
    points
        .iter()
        .flat_map(|point| [point.x.to_bits(), point.y.to_bits()])
        .collect()
}

#[test]
fn solve_converges_from_random_starts() {
    let sketch = fixture();
    let mut failures = Vec::new();
    for i in 0..STARTS {
        let mut points = start(&sketch, i);
        if let Err(error) = solve(&mut points, &sketch.constraints, options()) {
            failures.push(format!("seed {i}: Err({error:?})"));
            continue;
        }
        let max_residual = sketch
            .constraints
            .iter()
            .flat_map(|constraint| constraint.residual(&points))
            .map(f64::abs)
            .fold(0.0_f64, f64::max);
        let mut deviation = None;
        for (p, (point, target)) in points.iter().zip(&sketch.targets).enumerate() {
            for (coord, actual, expected) in
                [("x", point.x, target.x), ("y", point.y, target.y)]
            {
                let difference = (actual - expected).abs();
                if !difference.is_finite() || difference > POSITION_TOLERANCE {
                    deviation = Some((p, coord, difference));
                    break;
                }
            }
            if deviation.is_some() {
                break;
            }
        }
        if let Some((p, coord, difference)) = deviation {
            failures.push(format!(
                "seed {i}: wrong branch, point {p} coord {coord} off by {difference:e}, \
                 max residual {max_residual:e}"
            ));
        } else if !max_residual.is_finite() || max_residual > options().residual_tolerance {
            failures.push(format!(
                "seed {i}: Ok with excessive max residual {max_residual:e}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {STARTS} starts failed: {}",
        failures.len(),
        failures.iter().take(5).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
fn random_starts_are_distinct_and_cover_the_range() {
    assert_eq!(SplitMix64(0).next_u64(), 0xE220_A839_7B1D_CDAF);
    let sketch = fixture();
    let mut unique = HashSet::new();
    let mut smallest = f64::INFINITY;
    let mut largest = f64::NEG_INFINITY;
    for i in 0..STARTS {
        let points = start(&sketch, i);
        for (p, (point, target)) in points.iter().zip(&sketch.targets).enumerate() {
            for (coord, offset) in [("x", point.x - target.x), ("y", point.y - target.y)] {
                assert!(
                    (-0.5..0.5).contains(&offset),
                    "seed {i}: point {p} coord {coord} offset {offset:e} outside [-0.5, 0.5)"
                );
                smallest = smallest.min(offset);
                largest = largest.max(offset);
            }
        }
        assert!(unique.insert(bits(&points)), "seed {i}: duplicate start");
    }
    assert_eq!(unique.len() as u64, STARTS);
    assert!(
        smallest < -0.49 && largest > 0.49,
        "offset coverage: smallest {smallest:e}, largest {largest:e}"
    );
}

#[test]
fn start_at_solution_is_accepted_without_an_update() {
    let sketch = fixture();
    let mut points = sketch.targets.clone();
    let mut options = options();
    options.max_iterations = 1;
    assert_eq!(solve(&mut points, &sketch.constraints, options), Ok(()));
    for (p, (point, target)) in points.iter().zip(&sketch.targets).enumerate() {
        assert_eq!(point.x.to_bits(), target.x.to_bits(), "point {p} coord x");
        assert_eq!(point.y.to_bits(), target.y.to_bits(), "point {p} coord y");
    }
}

#[test]
fn same_seed_gives_bit_identical_solution() {
    let sketch = fixture();
    for i in [0, 137, 199] {
        let mut first = start(&sketch, i);
        let mut second = start(&sketch, i);
        assert_eq!(bits(&first), bits(&second), "seed {i}: starts differ");
        assert_eq!(
            solve(&mut first, &sketch.constraints, options()),
            Ok(()),
            "seed {i}: first solve"
        );
        assert_eq!(
            solve(&mut second, &sketch.constraints, options()),
            Ok(()),
            "seed {i}: second solve"
        );
        for (p, (a, b)) in first.iter().zip(&second).enumerate() {
            assert_eq!(a.x.to_bits(), b.x.to_bits(), "seed {i}: point {p} coord x");
            assert_eq!(a.y.to_bits(), b.y.to_bits(), "seed {i}: point {p} coord y");
        }
    }
}
