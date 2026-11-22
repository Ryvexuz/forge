//! Versioned sketch interchange format.

use crate::constraint::{Axis, Constraint};
use crate::geom::Point;
use std::fmt::Write;

#[derive(Clone, Debug, PartialEq)]
pub struct Sketch {
    /// The known solution (the fixture's integer target coordinates), one per point.
    pub targets: Vec<Point>,
    /// The perturbed starting guess the solver starts from, same length as `targets`.
    pub start: Vec<Point>,
    pub constraints: Vec<Constraint>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum FormatError {
    Empty,
    MissingHeader,
    UnknownVersion(u64),
    BadSection { line: usize },
    WrongFieldCount { line: usize },
    BadNumber { line: usize },
    NonFinite { line: usize },
    UnknownKind { line: usize },
    BadIndex { line: usize },
    CountMismatch { line: usize },
}

/// Serialize a sketch. Well-formed sketches have equally long point vectors,
/// finite numbers, and in-range constraint indices. This does not run a solver.
pub fn write_sketch(sketch: &Sketch) -> String {
    let mut out = format!("schema=1\npoints {}\n", sketch.targets.len());
    for (i, (target, start)) in sketch.targets.iter().zip(&sketch.start).enumerate() {
        let _ = writeln!(
            out,
            "p {} {:.12} {:.12} {:.12} {:.12}",
            i, target.x, target.y, start.x, start.y
        );
    }
    let _ = writeln!(out, "constraints {}", sketch.constraints.len());
    for constraint in &sketch.constraints {
        match *constraint {
            Constraint::Coincident { a, b } => {
                let _ = writeln!(out, "coincident {a} {b}");
            }
            Constraint::Distance { a, b, distance } => {
                let _ = writeln!(out, "distance {a} {b} {distance:.12}");
            }
            Constraint::Angle { a, b, c, d, angle } => {
                let _ = writeln!(out, "angle {a} {b} {c} {d} {angle:.12}");
            }
            Constraint::Parallel { a, b, c, d } => {
                let _ = writeln!(out, "parallel {a} {b} {c} {d}");
            }
            Constraint::Tangent {
                start,
                contact,
                center,
            } => {
                let _ = writeln!(out, "tangent {start} {contact} {center}");
            }
            Constraint::Fix { point, axis, value } => {
                let axis = match axis {
                    Axis::X => "x",
                    Axis::Y => "y",
                };
                let _ = writeln!(out, "fix {point} {axis} {value:.12}");
            }
        }
    }
    out
}

fn unsigned(text: &str, line: usize) -> Result<usize, FormatError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FormatError::BadNumber { line });
    }
    text.parse().map_err(|_| FormatError::BadNumber { line })
}

fn real(text: &str, line: usize) -> Result<f64, FormatError> {
    if text.is_empty() || text.chars().any(char::is_whitespace) {
        return Err(FormatError::BadNumber { line });
    }
    let value: f64 = text.parse().map_err(|_| FormatError::BadNumber { line })?;
    if !value.is_finite() {
        return Err(FormatError::NonFinite { line });
    }
    Ok(value)
}

fn section(text: Option<&str>, kind: &str, line: usize) -> Result<usize, FormatError> {
    let fields: Vec<_> = text.unwrap_or("").split(' ').collect();
    if fields.len() != 2 || fields[0] != kind {
        return Err(FormatError::BadSection { line });
    }
    unsigned(fields[1], line).map_err(|_| FormatError::BadSection { line })
}

/// Parse syntax and finite values only; geometric validity belongs to the model.
/// Missing rows are reported at the expected row (including one past EOF).
pub fn parse_sketch(text: &str) -> Result<Sketch, FormatError> {
    if text.is_empty() || text.starts_with('\n') {
        return Err(FormatError::Empty);
    }
    // split_terminator preserves internal blank rows without inventing a final row.
    let lines: Vec<_> = text.split_terminator('\n').collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains('\r') {
            return Err(FormatError::BadNumber { line: i + 1 });
        }
    }
    let header = lines.first().ok_or(FormatError::Empty)?;
    let version = header
        .strip_prefix("schema=")
        .ok_or(FormatError::MissingHeader)?;
    if version.is_empty() || !version.bytes().all(|b| b.is_ascii_digit()) {
        return Err(FormatError::MissingHeader);
    }
    let version: u64 = version
        .parse()
        .map_err(|_| FormatError::BadNumber { line: 1 })?;
    if version != 1 {
        return Err(FormatError::UnknownVersion(version));
    }
    if !text.ends_with('\n') {
        return Err(FormatError::BadNumber { line: lines.len() });
    }
    let count = section(lines.get(1).copied(), "points", 2)?;
    let mut targets = Vec::new();
    let mut start = Vec::new();
    let mut cursor = 2;
    // Do not reserve or iterate based on an untrusted count before checking rows.
    while targets.len() < count {
        let line = cursor + 1;
        let row = lines
            .get(cursor)
            .ok_or(FormatError::CountMismatch { line })?;
        if row.starts_with("constraints ") {
            return Err(FormatError::CountMismatch { line });
        }
        let fields: Vec<_> = row.split(' ').collect();
        if fields[0] != "p" {
            return Err(FormatError::UnknownKind { line });
        }
        if fields.len() != 6 {
            return Err(FormatError::WrongFieldCount { line });
        }
        if unsigned(fields[1], line)? != targets.len() {
            return Err(FormatError::BadIndex { line });
        }
        let target = Point {
            x: real(fields[2], line)?,
            y: real(fields[3], line)?,
        };
        let guess = Point {
            x: real(fields[4], line)?,
            y: real(fields[5], line)?,
        };
        targets.push(target);
        start.push(guess);
        cursor += 1;
    }
    if lines.get(cursor).is_some_and(|row| row.starts_with("p ")) {
        return Err(FormatError::CountMismatch { line: cursor + 1 });
    }
    let count = section(lines.get(cursor).copied(), "constraints", cursor + 1)?;
    cursor += 1;
    let mut constraints = Vec::new();
    while constraints.len() < count {
        let line = cursor + 1;
        let row = lines
            .get(cursor)
            .ok_or(FormatError::CountMismatch { line })?;
        let fields: Vec<_> = row.split(' ').collect();
        let expected = match fields[0] {
            "fix" | "distance" | "tangent" => 4,
            "angle" => 6,
            "parallel" => 5,
            "coincident" => 3,
            _ => return Err(FormatError::UnknownKind { line }),
        };
        if fields.len() != expected {
            return Err(FormatError::WrongFieldCount { line });
        }
        let index = |position: usize| -> Result<usize, FormatError> {
            let value = unsigned(fields[position], line)?;
            if value >= targets.len() {
                return Err(FormatError::BadIndex { line });
            }
            Ok(value)
        };
        let constraint = match fields[0] {
            "fix" => Constraint::Fix {
                point: index(1)?,
                axis: match fields[2] {
                    "x" => Axis::X,
                    "y" => Axis::Y,
                    _ => return Err(FormatError::BadNumber { line }),
                },
                value: real(fields[3], line)?,
            },
            "distance" => Constraint::Distance {
                a: index(1)?,
                b: index(2)?,
                distance: real(fields[3], line)?,
            },
            "angle" => Constraint::Angle {
                a: index(1)?,
                b: index(2)?,
                c: index(3)?,
                d: index(4)?,
                angle: real(fields[5], line)?,
            },
            "parallel" => Constraint::Parallel {
                a: index(1)?,
                b: index(2)?,
                c: index(3)?,
                d: index(4)?,
            },
            "tangent" => Constraint::Tangent {
                start: index(1)?,
                contact: index(2)?,
                center: index(3)?,
            },
            "coincident" => Constraint::Coincident {
                a: index(1)?,
                b: index(2)?,
            },
            _ => return Err(FormatError::UnknownKind { line }),
        };
        constraints.push(constraint);
        cursor += 1;
    }
    if cursor != lines.len() {
        return Err(FormatError::CountMismatch { line: cursor + 1 });
    }
    Ok(Sketch {
        targets,
        start,
        constraints,
    })
}
