//! Budget mode times the reference solve; add-redundant mode copies the first
//! Distance, since a copy is dependent on its original. Drop mode analyses at
//! targets and reports the solver result without requiring success: an
//! under-constrained Jacobian lacks full column rank, causing RankDeficient.

use std::time::Instant;

use constraint_sketch::constraint::Constraint;
use constraint_sketch::dof::{analyse_at, Status};
use constraint_sketch::format::{parse_sketch, Sketch};
use constraint_sketch::solve::{solve, SolveOptions};

const FIXTURE: &str = include_str!("../tests/fixtures/sketch30_v1.txt");
const POSITION_TOLERANCE: f64 = 1e-9;

#[derive(Debug, PartialEq)]
enum Mode {
    Budget { budget_us: u64 },
    AddRedundant { copies: usize },
    Drop { index: usize },
}

#[derive(Debug, PartialEq)]
struct Args {
    mode: Mode,
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
    let mut copies = None;
    let mut index = None;
    let mut args = args.iter();
    while let Some(flag) = args.next() {
        let slot = match flag.as_str() {
            "--constraints" => &mut constraints,
            "--budget-us" => &mut budget_us,
            "--add-redundant" => &mut copies,
            "--drop" => &mut index,
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
    for (flag, value) in [("--add-redundant", copies), ("--drop", index)] {
        if value.is_some_and(|value| !(1..=30).contains(&value)) {
            return Err(format!("{flag} must be in 1..=30"));
        }
    }
    let mode = match (budget_us, copies, index) {
        (Some(budget_us), None, None) => Mode::Budget { budget_us },
        (None, Some(copies), None) => Mode::AddRedundant {
            copies: usize::try_from(copies).map_err(|_| "overflow copies")?,
        },
        (None, None, Some(index)) => Mode::Drop {
            index: usize::try_from(index).map_err(|_| "overflow index")?,
        },
        _ => return Err("exactly one mode flag is required".into()),
    };
    Ok(Args { mode })
}

fn run(args: &Args) -> (String, i32) {
    match args.mode {
        Mode::Budget { budget_us } => run_budget(budget_us),
        Mode::AddRedundant { copies } => run_report(Some(copies), None),
        Mode::Drop { index } => run_report(None, Some(index)),
    }
}

fn run_budget(budget_us: u64) -> (String, i32) {
    let sketch = match parse_sketch(FIXTURE) {
        Ok(sketch) => sketch,
        Err(error) => return (format!("solve30: FAIL solution parse {error:?}"), 1),
    };
    let mut solved = sketch.start.clone();
    if let Err(error) = solve(&mut solved, &sketch.constraints, options()) {
        return (format!("solve30: FAIL solution solve {error:?}"), 1);
    }
    for (p, (point, target)) in solved.iter().zip(&sketch.targets).enumerate() {
        for (coord, actual, expected) in [("x", point.x, target.x), ("y", point.y, target.y)] {
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
    if solve_us <= u128::from(budget_us) {
        (
            format!(
                "solve30: ok constraints=30 status=fully_constrained dof=0 solve_us={solve_us}"
            ),
            0,
        )
    } else {
        (
            format!(
                "solve30: FAIL budget constraints=30 status=fully_constrained dof=0 \
                 solve_us={solve_us} budget_us={budget_us}"
            ),
            1,
        )
    }
}

fn redundant_constraints(sketch: &Sketch, copies: usize) -> Result<Vec<Constraint>, String> {
    if !(1..=sketch.constraints.len()).contains(&copies) {
        return Err("invalid redundant copies".into());
    }
    let first = sketch
        .constraints
        .iter()
        .find(|constraint| matches!(constraint, Constraint::Distance { .. }))
        .ok_or("fixture has no Distance")?;
    let mut constraints = sketch.constraints.clone();
    constraints.extend(std::iter::repeat_n(*first, copies));
    Ok(constraints)
}

fn dropped_constraints(
    sketch: &Sketch,
    index: usize,
) -> Result<(Vec<Constraint>, Constraint), String> {
    if !(1..=sketch.constraints.len()).contains(&index) {
        return Err("invalid drop index".into());
    }
    let mut constraints = sketch.constraints.clone();
    let removed = constraints.remove(index - 1);
    Ok((constraints, removed))
}

fn run_report(copies: Option<usize>, index: Option<usize>) -> (String, i32) {
    let sketch = match parse_sketch(FIXTURE) {
        Ok(sketch) => sketch,
        Err(error) => return (format!("solve30: FAIL solution parse {error:?}"), 1),
    };
    if let Some(index) = index {
        let (constraints, removed) = match dropped_constraints(&sketch, index) {
            Ok(result) => result,
            Err(error) => return (format!("solve30: FAIL status {error}"), 1),
        };
        let expected_dof = removed.jacobian_rows(&sketch.targets).len();
        let analysis = match analyse_at(&sketch.targets, &constraints) {
            Ok(analysis) => analysis,
            Err(error) => return (format!("solve30: FAIL status analyse {error:?}"), 1),
        };
        let mut points = sketch.start.clone();
        let result = match solve(&mut points, &constraints, options()) {
            Ok(_) => "ok".into(),
            Err(error) => format!("{error:?}"),
        };
        if analysis.status == Status::Underconstrained
            && analysis.redundant.is_empty()
            && analysis.dof == expected_dof
        {
            return (
                format!(
                    "solve30: ok constraints=30 drop={index} status={} dof={} solve={result}",
                    analysis.status.as_str(),
                    analysis.dof
                ),
                0,
            );
        }
        return (
            format!(
                "solve30: FAIL status drop={index} status={} redundant={} dof={} expected_dof={expected_dof}",
                analysis.status.as_str(),
                analysis.redundant.len(),
                analysis.dof
            ),
            1,
        );
    }
    let Some(copies) = copies else {
        return ("solve30: FAIL status missing mode".into(), 1);
    };
    let constraints = match redundant_constraints(&sketch, copies) {
        Ok(constraints) => constraints,
        Err(error) => return (format!("solve30: FAIL status {error}"), 1),
    };
    let mut solved = sketch.start.clone();
    if let Err(error) = solve(&mut solved, &constraints, options()) {
        return (format!("solve30: FAIL solution solve {error:?}"), 1);
    }
    for (p, (point, target)) in solved.iter().zip(&sketch.targets).enumerate() {
        for (coord, actual, expected) in [("x", point.x, target.x), ("y", point.y, target.y)] {
            let difference = (actual - expected).abs();
            if !actual.is_finite() || !difference.is_finite() || difference > POSITION_TOLERANCE {
                return (
                    format!("solve30: FAIL solution point={p} coord={coord} off={difference:e}"),
                    1,
                );
            }
        }
    }
    let analysis = match analyse_at(&solved, &constraints) {
        Ok(analysis) => analysis,
        Err(error) => return (format!("solve30: FAIL status analyse {error:?}"), 1),
    };
    if analysis.status == Status::Overconstrained
        && analysis.dof == 0
        && analysis.redundant == (sketch.constraints.len()..constraints.len()).collect::<Vec<_>>()
    {
        (
            format!(
                "solve30: ok constraints=30 add_redundant={copies} status={} redundant={} dof={}",
                analysis.status.as_str(),
                analysis.redundant.len(),
                analysis.dof
            ),
            0,
        )
    } else {
        (
            format!(
                "solve30: FAIL status status={} redundant={} dof={}",
                analysis.status.as_str(),
                analysis.redundant.len(),
                analysis.dof
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

#[cfg(test)]
mod tests {
    use super::*;
    use constraint_sketch::constraint::Axis;

    #[test]
    fn parse_args_modes_and_refusals() {
        for (text, mode) in [
            (
                "--constraints 30 --budget-us 16700",
                Mode::Budget { budget_us: 16700 },
            ),
            (
                "--constraints 30 --add-redundant 1",
                Mode::AddRedundant { copies: 1 },
            ),
            ("--drop 30 --constraints 30", Mode::Drop { index: 30 }),
        ] {
            let args: Vec<_> = text.split_whitespace().map(String::from).collect();
            assert_eq!(parse_args(&args), Ok(Args { mode }), "{text}");
        }
        for text in [
            "--constraints 30",
            "--constraints 30 --budget-us 5 --drop 1",
            "--constraints 30 --add-redundant 1 --drop 1",
            "--constraints 30 --add-redundant 0",
            "--constraints 30 --add-redundant 31",
            "--constraints 30 --drop 0",
            "--constraints 30 --drop 31",
            "--constraints 30 --drop +1",
            "--constraints 30 --drop -1",
            "--constraints 30 --drop",
            "--constraints 30 --drop 1 --drop 2",
            "--constraints 29 --drop 1",
            "--constraints 30 --add-redundant 18446744073709551616",
            "--constraints 30 --drop 1 --steps 3",
        ] {
            let args: Vec<_> = text.split_whitespace().map(String::from).collect();
            assert!(parse_args(&args).is_err(), "{text}");
        }
        for value in ["", "1\r", "1\n", "C:\\1", "18446744073709551616"] {
            let args = ["--constraints", "30", "--drop", value].map(String::from);
            assert!(parse_args(&args).is_err(), "{value:?}");
        }
    }

    #[test]
    fn add_redundant_reports_overconstrained() {
        let mut fixture = parse_sketch(FIXTURE).unwrap();
        let constraints = redundant_constraints(&fixture, 2).unwrap();
        assert_eq!(constraints.len(), 32);
        let distance = Constraint::Distance {
            a: 0,
            b: 1,
            distance: 5.0,
        };
        assert_eq!(&constraints[30..], &[distance, distance]);
        for copies in [1, 3] {
            assert_eq!(
                run(&Args {
                    mode: Mode::AddRedundant { copies },
                }),
                (format!("solve30: ok constraints=30 add_redundant={copies} status=overconstrained redundant={copies} dof=0"), 0)
            );
        }
        assert!(redundant_constraints(&fixture, 0).is_err());
        assert!(redundant_constraints(&fixture, usize::MAX).is_err());
        fixture
            .constraints
            .retain(|c| !matches!(c, Constraint::Distance { .. }));
        assert!(redundant_constraints(&fixture, 1).is_err());
    }

    #[test]
    fn drop_reports_underconstrained_for_every_constraint() {
        let fixture = parse_sketch(FIXTURE).unwrap();
        let (remaining, removed) = dropped_constraints(&fixture, 1).unwrap();
        assert_eq!(remaining.len(), 29);
        assert_eq!(remaining, fixture.constraints[1..]);
        assert_eq!(
            removed,
            Constraint::Fix {
                point: 0,
                axis: Axis::X,
                value: 0.0,
            }
        );
        assert!(dropped_constraints(&fixture, 0).is_err());
        assert!(dropped_constraints(&fixture, 31).is_err());
        assert!(dropped_constraints(&fixture, usize::MAX).is_err());
        for index in 1..=30 {
            let (line, code) = run(&Args {
                mode: Mode::Drop { index },
            });
            assert_eq!(code, 0, "{line}");
            assert!(
                line.starts_with(&format!(
                    "solve30: ok constraints=30 drop={index} status=underconstrained dof=1 solve="
                )),
                "{line}"
            );
            if index == 1 {
                assert_eq!(line, "solve30: ok constraints=30 drop=1 status=underconstrained dof=1 solve=RankDeficient");
            }
        }
    }

    #[test]
    fn budget_mode_lines_unchanged() {
        for (budget_us, prefix, expected_code) in [
            (
                u64::MAX,
                "solve30: ok constraints=30 status=fully_constrained dof=0 solve_us=",
                0,
            ),
            (
                1,
                "solve30: FAIL budget constraints=30 status=fully_constrained dof=0 solve_us=",
                1,
            ),
        ] {
            let (line, code) = run(&Args {
                mode: Mode::Budget { budget_us },
            });
            assert_eq!(code, expected_code, "{line}");
            assert!(line.starts_with(prefix), "{line}");
        }
    }
}
