//! Constraint equations with columns ordered `[x0, y0, x1, y1, ...]`.
//!
//! Coordinates and dimensional targets must be finite and at most `1e100`
//! in magnitude, leaving headroom for products. Direction lengths must be at
//! least `1e-100`, keeping inverse lengths finite. Invalid input panics with
//! `invalid constraint input` before evaluation. Angles are in radians.

use crate::geom::Point;

/// The coordinate pinned by a [`Constraint::Fix`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

/// Point indices refer to the supplied point slice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Constraint {
    /// Two equations: `a.x - b.x`, `a.y - b.y`.
    Coincident { a: usize, b: usize },
    /// Euclidean length of `b-a` minus the nonnegative target.
    Distance { a: usize, b: usize, distance: f64 },
    /// Signed angle from `b-a` to `d-c`, minus target, wrapped to [-pi, pi).
    /// The residual is discontinuous at the wrap boundary.
    Angle {
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        angle: f64,
    },
    /// Cross product `(b-a) x (d-c)`.
    Parallel {
        a: usize,
        b: usize,
        c: usize,
        d: usize,
    },
    /// Dot product `(contact-start) . (contact-center)`.
    /// The line ends at the circle contact; the radius must be nonzero.
    Tangent {
        start: usize,
        contact: usize,
        center: usize,
    },
    /// One equation: selected coordinate minus its target value.
    Fix {
        point: usize,
        axis: Axis,
        value: f64,
    },
}

impl Constraint {
    /// Evaluate the equations. See module documentation for the valid domain.
    pub fn residual(&self, pts: &[Point]) -> Vec<f64> {
        self.evaluate(pts).0
    }

    /// Analytic derivatives, one dense row per residual equation.
    /// At an angle wrap boundary these are the one-sided smooth derivatives.
    pub fn jacobian_rows(&self, pts: &[Point]) -> Vec<Vec<f64>> {
        self.evaluate(pts).1
    }

    fn evaluate(&self, pts: &[Point]) -> (Vec<f64>, Vec<Vec<f64>>) {
        let point = |i: usize| {
            let p = *pts.get(i).expect("invalid constraint input: point index");
            assert!(
                [p.x, p.y].iter().all(|v| v.is_finite() && v.abs() <= 1e100),
                "invalid constraint input: coordinate"
            );
            p
        };
        let target = |v: f64| {
            assert!(
                v.is_finite() && v.abs() <= 1e100,
                "invalid constraint input: target"
            );
        };
        let vector = |a, b| {
            let a = point(a);
            let b = point(b);
            let v = [b.x - a.x, b.y - a.y];
            assert!(
                v[0].hypot(v[1]) >= 1e-100,
                "invalid constraint input: degenerate direction"
            );
            v
        };
        let mut rows = Vec::new();
        let mut values = Vec::new();
        let mut equation = |value: f64, entries: &[(usize, [f64; 2])]| {
            let mut row = vec![0.0; pts.len() * 2];
            for &(i, v) in entries {
                row[2 * i] += v[0];
                row[2 * i + 1] += v[1];
            }
            assert!(
                value.is_finite() && row.iter().all(|v| v.is_finite()),
                "invalid constraint input: nonfinite equation"
            );
            values.push(value);
            rows.push(row);
        };
        let neg = |v: [f64; 2]| [-v[0], -v[1]];
        match *self {
            Self::Coincident { a, b } => {
                let p = point(a);
                let q = point(b);
                equation(p.x - q.x, &[(a, [1.0, 0.0]), (b, [-1.0, 0.0])]);
                equation(p.y - q.y, &[(a, [0.0, 1.0]), (b, [0.0, -1.0])]);
            }
            Self::Distance { a, b, distance } => {
                target(distance);
                assert!(
                    distance >= 0.0,
                    "invalid constraint input: negative distance"
                );
                let v = vector(a, b);
                let length = v[0].hypot(v[1]);
                let g = [v[0] / length, v[1] / length];
                equation(length - distance, &[(a, neg(g)), (b, g)]);
            }
            Self::Angle { a, b, c, d, angle } => {
                assert!(
                    angle.is_finite() && angle.abs() <= std::f64::consts::PI,
                    "invalid constraint input: angle"
                );
                let u = vector(a, b);
                let v = vector(c, d);
                let gradient = |w: [f64; 2]| {
                    let length = w[0].hypot(w[1]);
                    [-w[1] / length / length, w[0] / length / length]
                };
                let gu = gradient(u);
                let gv = gradient(v);
                let pi = std::f64::consts::PI;
                let value =
                    (v[1].atan2(v[0]) - u[1].atan2(u[0]) - angle + pi).rem_euclid(2.0 * pi) - pi;
                equation(value, &[(a, gu), (b, neg(gu)), (c, neg(gv)), (d, gv)]);
            }
            Self::Parallel { a, b, c, d } => {
                let u = vector(a, b);
                let v = vector(c, d);
                let gu = [v[1], -v[0]];
                let gv = [-u[1], u[0]];
                equation(
                    u[0] * v[1] - u[1] * v[0],
                    &[(a, neg(gu)), (b, gu), (c, neg(gv)), (d, gv)],
                );
            }
            Self::Tangent {
                start,
                contact,
                center,
            } => {
                let u = vector(start, contact);
                let v = vector(center, contact);
                equation(
                    u[0] * v[0] + u[1] * v[1],
                    &[
                        (start, neg(v)),
                        (contact, [u[0] + v[0], u[1] + v[1]]),
                        (center, neg(u)),
                    ],
                );
            }
            Self::Fix {
                point: i,
                axis,
                value,
            } => {
                target(value);
                let p = point(i);
                let (coordinate, g) = match axis {
                    Axis::X => (p.x, [1.0, 0.0]),
                    Axis::Y => (p.y, [0.0, 1.0]),
                };
                equation(coordinate - value, &[(i, g)]);
            }
        }
        (values, rows)
    }
}
