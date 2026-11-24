//! Local DOF = unknowns - rank at the given points. Equation counts per kind
//! come from `Constraint::jacobian_rows`. Singular configurations (for example,
//! collinear points) can have more DOF than the generic sketch.
//!
//! QR processes the transpose's columns in equation order without pivoting,
//! after scaling by the largest absolute entry. A diagonal at most
//! `RANK_TOLERANCE * largest_initial_column_norm` is dependent. This matches
//! the solver's scaling and relative cutoff policy and its tests' tolerance.
//! Redundancy takes status priority over remaining DOF, then underconstraint,
//! then full constraint.

use crate::constraint::Constraint;
use crate::format::Sketch;
use crate::geom::Point;

/// Relative QR cutoff after common scaling, matching the solver tests' value.
pub const RANK_TOLERANCE: f64 = 1e-12;

/// Redundancy wins over positive DOF; otherwise positive DOF is underconstrained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    FullyConstrained,
    Underconstrained,
    Overconstrained,
}

impl Status {
    /// Stable status strings for reports.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::FullyConstrained => "fully_constrained",
            Self::Underconstrained => "underconstrained",
            Self::Overconstrained => "overconstrained",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Analysis {
    pub status: Status,
    /// Twice the number of points.
    pub unknowns: usize,
    /// Total number of Jacobian rows.
    pub equations: usize,
    /// Numerical rank at the analysed points.
    pub rank: usize,
    /// Unknowns minus rank.
    pub dof: usize,
    /// Ascending constraint indices with at least one dependent equation.
    pub redundant: Vec<usize>,
    /// Maximum absolute residual, including redundant equations; zero if empty.
    pub residual_inf: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DofError {
    /// Nonfinite or out-of-domain point, or a panicking constraint evaluation.
    InvalidInput,
}

/// Analyse the known solution (`targets`), never the starting guess.
pub fn analyse(sketch: &Sketch) -> Result<Analysis, DofError> {
    analyse_at(&sketch.targets, &sketch.constraints)
}

/// Analyse any points without modifying them. Coordinates must be finite and
/// at most 1e100 in magnitude, matching the constraint evaluation domain.
pub fn analyse_at(points: &[Point], constraints: &[Constraint]) -> Result<Analysis, DofError> {
    if points
        .iter()
        .any(|p| [p.x, p.y].iter().any(|v| !v.is_finite() || v.abs() > 1e100))
    {
        return Err(DofError::InvalidInput);
    }
    let unknowns = points.len().checked_mul(2).ok_or(DofError::InvalidInput)?;
    let (rows, owners, residual_inf) = std::panic::catch_unwind(|| {
        let mut rows = Vec::new();
        let mut owners = Vec::new();
        let mut residual_inf = 0.0_f64;
        for (index, constraint) in constraints.iter().enumerate() {
            let equations = constraint.jacobian_rows(points);
            owners.extend(std::iter::repeat_n(index, equations.len()));
            rows.extend(equations);
            for value in constraint.residual(points) {
                residual_inf = residual_inf.max(value.abs());
            }
        }
        (rows, owners, residual_inf)
    })
    .map_err(|_| DofError::InvalidInput)?;
    let equations = rows.len();
    let (rank, redundant) = rank_rows(rows, &owners, unknowns);
    let dof = unknowns - rank;
    let status = if !redundant.is_empty() {
        Status::Overconstrained
    } else if dof > 0 {
        Status::Underconstrained
    } else {
        Status::FullyConstrained
    };
    Ok(Analysis {
        status,
        unknowns,
        equations,
        rank,
        dof,
        redundant,
        residual_inf,
    })
}

// Each stored row is a column of J^T. Skipping a dependent column does not
// consume a reflector, so later independent equations still contribute rank.
fn rank_rows(mut columns: Vec<Vec<f64>>, owners: &[usize], unknowns: usize) -> (usize, Vec<usize>) {
    let scale = columns.iter().flatten().fold(0.0_f64, |s, v| s.max(v.abs()));
    if scale != 0.0 {
        for column in &mut columns {
            for value in column {
                *value /= scale;
            }
        }
    }
    let largest = columns
        .iter()
        .map(|column| column.iter().fold(0.0_f64, |s, v| s.hypot(*v)))
        .fold(0.0_f64, f64::max);
    let mut rank = 0;
    let mut redundant = Vec::new();
    for j in 0..columns.len() {
        let norm = columns[j][rank..]
            .iter()
            .fold(0.0_f64, |s, v| s.hypot(*v));
        if rank == unknowns || norm <= RANK_TOLERANCE * largest {
            if redundant.last() != Some(&owners[j]) {
                redundant.push(owners[j]);
            }
            continue;
        }
        let alpha = -norm.copysign(columns[j][rank]);
        let mut reflector: Vec<f64> = columns[j][rank..].iter().map(|v| v / norm).collect();
        reflector[0] -= alpha / norm;
        let length = reflector.iter().fold(0.0_f64, |s, v| s.hypot(*v));
        for value in &mut reflector {
            *value /= length;
        }
        for column in &mut columns[j..] {
            let dot: f64 = reflector
                .iter()
                .zip(&column[rank..])
                .map(|(v, x)| v * x)
                .sum();
            for (x, v) in column[rank..].iter_mut().zip(&reflector) {
                *x -= 2.0 * v * dot;
            }
        }
        rank += 1;
    }
    (rank, redundant)
}
