use std::time::Instant;

use constraint_sketch::dof::{Status, analyse_at};
use constraint_sketch::format::parse_sketch;
use constraint_sketch::solve::{SolveOptions, solve};

const FIXTURE: &str = include_str!("../tests/fixtures/sketch30_v1.txt");
const POSITION_TOLERANCE: f64 = 1e-9;

struct Args {
    budget_us: u64,
}

fn options() -> SolveOptions {
    SolveOptions {
        max_iterations: 64,
        residual_tolerance: 1e-12,
        rank_tolerance: constraint_sketch::dof::RANK_TOLERANCE,
    }
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut constraints = None;
    let mut budget_us = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--constraints" => &mut constraints,
            "--budget-us" => &mut budget_us,
            _ => return Err(format!("unknown flag {flag:?}")),
        };
        if slot.is_some() {
            return Err(format!("duplicate {flag}"));
        }
        let value = args.next().ok_or_else(|| format!("missing {flag} value"))?;
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("invalid {flag} value"));
        }
        *slot = Some(
            value
                .parse::<u64>()
                .map_err(|_| format!("overflow {flag} value"))?,
        );
    }
    let constraints = constraints.ok_or("missing --constraints")?;
    if constraints != 30 {
        return Err("--constraints must be 30".into());
    }
    Ok(Args {
        budget_us: budget_us.ok_or("missing --budget-us")?,
    })
}

fn run(args: &Args) -> (String, i32) {
    let sketch = match parse_sketch(FIXTURE) {
        Ok(sketch) => sketch,
        Err(error) => return (format!("solve30: FAIL solution parse {error:?}"), 1),
    };
    let mut solved = sketch.start.clone();
    if let Err(error) = solve(&mut solved, &sketch.constraints, options()) {
        return (format!("solve30: FAIL solution solve {error:?}"), 1);
    }
    for (p, (point, target)) in solved.iter().zip(&sketch.targets).enumerate() {
        for (coord, actual, expected) in
            [("x", point.x, target.x), ("y", point.y, target.y)]
        {
            let difference = (actual - expected).abs();
            if !actual.is_finite() || !difference.is_finite() || difference > POSITION_TOLERANCE {
                return (
                    format!("solve30: FAIL solution point={p} coord={coord} off={difference:e}"),
                    1,
                );
            }
        }
    }
    let analysis = match analyse_at(&solved, &sketch.constraints) {
        Ok(analysis) => analysis,
        Err(error) => return (format!("solve30: FAIL status analyse {error:?}"), 1),
    };
    if analysis.status != Status::FullyConstrained || analysis.dof != 0 {
        return (
            format!(
                "solve30: FAIL status status={} dof={}",
                analysis.status.as_str(),
                analysis.dof
            ),
            1,
        );
    }
    let mut durations = Vec::with_capacity(101);
    for k in 1..=101 {
        let mut points = sketch.start.clone();
        let start = Instant::now();
        let result = solve(&mut points, &sketch.constraints, options());
        let elapsed = start.elapsed();
        if let Err(error) = result {
            return (
                format!("solve30: FAIL solution repeat={k} solve {error:?}"),
                1,
            );
        }
        for (p, (point, reference)) in points.iter().zip(&solved).enumerate() {
            for (coord, actual, expected) in
                [("x", point.x, reference.x), ("y", point.y, reference.y)]
            {
                if actual.to_bits() != expected.to_bits() {
                    return (
                        format!("solve30: FAIL solution repeat={k} point={p} coord={coord} bits"),
                        1,
                    );
                }
            }
        }
        durations.push(elapsed);
    }
    durations.sort_unstable();
    let nanos = durations[50].as_nanos();
    let solve_us = nanos.div_ceil(1000);
    if solve_us <= u128::from(args.budget_us) {
        (
            format!("solve30: ok constraints=30 status=fully_constrained dof=0 solve_us={solve_us}"),
            0,
        )
    } else {
        (
            format!(
                "solve30: FAIL budget constraints=30 status=fully_constrained dof=0 \
                 solve_us={solve_us} budget_us={}",
                args.budget_us
            ),
            1,
        )
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (line, code) = match parse_args(&args) {
        Ok(args) => run(&args),
        Err(reason) => (format!("solve30: refused {reason}"), 2),
    };
    println!("{line}");
    std::process::exit(code);
}
