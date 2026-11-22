//! Dense Householder least squares and Gauss-Newton iteration.
//!
//! Settings are supplied by the caller, rather than hidden global limits.
//! Constraint inputs follow `constraint`'s documented domain. Invalid
//! constraint evaluations are converted to `InvalidInput`; points are updated
//! only when the complete solve succeeds.

use crate::constraint::Constraint;
use crate::geom::Point;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolveError {
    InvalidInput,
    RankDeficient,
    NonFinite,
    DidNotConverge,
}

#[derive(Clone, Copy, Debug)]
pub struct SolveOptions {
    /// Fixed maximum number of Gauss-Newton updates.
    pub max_iterations: usize,
    /// Absolute infinity-norm residual tolerance, in equation units.
    pub residual_tolerance: f64,
    /// Relative QR diagonal cutoff against the largest initial column norm.
    /// Must lie strictly between zero and one.
    pub rank_tolerance: f64,
}

/// Solve `min ||a*x-b||` with full-column-rank Householder QR.
///
/// All entries are scaled by a common matrix magnitude before reflection.
/// A diagonal at most `rank_tolerance * largest_column_norm` is rejected.
/// This is a numerical rank policy, not a symbolic degrees-of-freedom test.
pub fn householder_qr(
    mut a: Vec<Vec<f64>>,
    mut b: Vec<f64>,
    rank_tolerance: f64,
) -> Result<Vec<f64>, SolveError> {
    if !rank_tolerance.is_finite() || rank_tolerance <= 0.0 || rank_tolerance >= 1.0 {
        return Err(SolveError::InvalidInput);
    }
    let n = a.first().map_or(0, Vec::len);
    if a.len() != b.len()
        || a.iter().any(|row| row.len() != n)
        || a.iter().flatten().chain(b.iter()).any(|v| !v.is_finite())
    {
        return Err(SolveError::InvalidInput);
    }
    if a.len() < n {
        return Err(SolveError::RankDeficient);
    }
    if n == 0 {
        return Ok(Vec::new());
    }
    let scale = a.iter().flatten().fold(0.0_f64, |s, v| s.max(v.abs()));
    if scale == 0.0 {
        return Err(SolveError::RankDeficient);
    }
    for row in &mut a {
        for v in row {
            *v /= scale;
        }
    }
    for v in &mut b {
        *v /= scale;
    }
    if b.iter().any(|v| !v.is_finite()) {
        return Err(SolveError::NonFinite);
    }
    let largest = (0..n)
        .map(|j| a.iter().fold(0.0_f64, |s, row| s.hypot(row[j])))
        .fold(0.0_f64, f64::max);
    for k in 0..n {
        let norm = a[k..].iter().fold(0.0_f64, |s, row| s.hypot(row[k]));
        if norm <= rank_tolerance * largest {
            return Err(SolveError::RankDeficient);
        }
        let alpha = -norm.copysign(a[k][k]);
        let mut v: Vec<f64> = a[k..].iter().map(|row| row[k] / norm).collect();
        v[0] -= alpha / norm;
        let vnorm = v.iter().fold(0.0_f64, |s, x| s.hypot(*x));
        for x in &mut v {
            *x /= vnorm;
        }
        for j in k..n {
            let dot: f64 = v.iter().enumerate().map(|(i, v)| v * a[k + i][j]).sum();
            for (i, v) in v.iter().enumerate() {
                a[k + i][j] -= 2.0 * v * dot;
            }
        }
        let dot: f64 = v.iter().enumerate().map(|(i, v)| v * b[k + i]).sum();
        for (i, v) in v.iter().enumerate() {
            b[k + i] -= 2.0 * v * dot;
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let tail: f64 = x[i + 1..]
            .iter()
            .enumerate()
            .map(|(j, x)| a[i][i + 1 + j] * x)
            .sum();
        x[i] = (b[i] - tail) / a[i][i];
    }
    if x.iter().any(|v| !v.is_finite()) {
        return Err(SolveError::NonFinite);
    }
    Ok(x)
}

/// Solve transactionally. Even a zero-residual sketch must have full column
/// rank; unanchored rigid motions are not silently accepted as a solution.
pub fn solve(
    points: &mut [Point],
    constraints: &[Constraint],
    options: SolveOptions,
) -> Result<(), SolveError> {
    if options.max_iterations == 0
        || !options.residual_tolerance.is_finite()
        || options.residual_tolerance <= 0.0
        || !options.rank_tolerance.is_finite()
        || options.rank_tolerance <= 0.0
        || options.rank_tolerance >= 1.0
    {
        return Err(SolveError::InvalidInput);
    }
    let mut current = points.to_vec();
    for iteration in 0..=options.max_iterations {
        if current
            .iter()
            .any(|p| [p.x, p.y].iter().any(|v| !v.is_finite() || v.abs() > 1e100))
        {
            return Err(SolveError::InvalidInput);
        }
        // S2's public evaluation API reports invalid domains by panicking.
        let (rows, residuals) = std::panic::catch_unwind(|| {
            let mut rows = Vec::new();
            let mut residuals = Vec::new();
            for constraint in constraints {
                rows.extend(constraint.jacobian_rows(&current));
                residuals.extend(constraint.residual(&current));
            }
            (rows, residuals)
        })
        .map_err(|_| SolveError::InvalidInput)?;
        if rows.is_empty() && !current.is_empty() {
            return Err(SolveError::RankDeficient);
        }
        let delta = householder_qr(
            rows,
            residuals.iter().map(|r| -r).collect(),
            options.rank_tolerance,
        )?;
        if residuals
            .iter()
            .all(|r| r.abs() <= options.residual_tolerance)
        {
            points.copy_from_slice(&current);
            return Ok(());
        }
        if iteration == options.max_iterations {
            return Err(SolveError::DidNotConverge);
        }
        let next: Vec<Point> = current
            .iter()
            .zip(delta.as_chunks::<2>().0)
            .map(|(p, d)| Point {
                x: p.x + d[0],
                y: p.y + d[1],
            })
            .collect();
        if next.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
            return Err(SolveError::NonFinite);
        }
        current = next;
    }
    Err(SolveError::DidNotConverge)
}
