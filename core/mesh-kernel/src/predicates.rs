//! Adaptive exact signs of determinants of finite binary64 coordinates.
//!
//! The first-stage error bounds use unit roundoff, including the error in
//! coordinate subtraction. Uncertain results use error-free sums and products,
//! assembled into nonoverlapping floating-point expansions. Each component has
//! a separate binary exponent: unlike an ordinary binary64 expansion, this
//! preserves terms below the subnormal range and above the overflow threshold.
//! No coordinate magnitude restriction is imposed on finite inputs.

/// A coordinate was NaN or infinite.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PredicateError {
    NonFiniteCoordinate,
}

impl std::fmt::Display for PredicateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("orientation requires finite coordinates")
    }
}

impl std::error::Error for PredicateError {}

/// Returns +1 for counterclockwise, -1 for clockwise, and 0 for collinear.
///
/// The sign is exact for the supplied binary64 values, not for the real
/// coordinates that may have been rounded to obtain them.
pub fn orient2d(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Result<i8, PredicateError> {
    validate(&[a, b, c])?;
    let ac = [a[0] - c[0], a[1] - c[1]];
    let bc = [b[0] - c[0], b[1] - c[1]];
    if filter_range(&[ac, bc]) {
        let left = ac[0] * bc[1];
        let right = ac[1] * bc[0];
        let determinant = left - right;
        let u = f64::EPSILON / 2.0;
        // A two-product determinant, with rounded coordinate differences:
        // the first-order bound is 3u; 16u^2 covers the higher-order terms.
        let bound = (3.0 + 16.0 * u) * u * (left.abs() + right.abs());
        if determinant.abs() > bound {
            return Ok(sign(determinant));
        }
    }
    let acx = Expansion::difference(a[0], c[0]);
    let acy = Expansion::difference(a[1], c[1]);
    let bcx = Expansion::difference(b[0], c[0]);
    let bcy = Expansion::difference(b[1], c[1]);
    Ok(acx.mul(&bcy).sub(acy.mul(&bcx)).sign())
}

/// Returns the exact sign of det(a-d, b-d, c-d), with vectors as rows.
///
/// Zero means coplanar. In particular, the xy triangle (0,0,0), (1,0,0),
/// (0,1,0) has negative orientation relative to (0,0,1).
pub fn orient3d(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Result<i8, PredicateError> {
    validate(&[a, b, c, d])?;
    let ad: [f64; 3] = std::array::from_fn(|i| a[i] - d[i]);
    let bd: [f64; 3] = std::array::from_fn(|i| b[i] - d[i]);
    let cd: [f64; 3] = std::array::from_fn(|i| c[i] - d[i]);
    if filter_range(&[ad, bd, cd]) {
        let p = [bd[0] * cd[1], cd[0] * ad[1], ad[0] * bd[1]];
        let q = [cd[0] * bd[1], ad[0] * cd[1], bd[0] * ad[1]];
        let determinant = ad[2] * (p[0] - q[0]) + bd[2] * (p[1] - q[1]) + cd[2] * (p[2] - q[2]);
        let permanent = (p[0].abs() + q[0].abs()) * ad[2].abs()
            + (p[1].abs() + q[1].abs()) * bd[2].abs()
            + (p[2].abs() + q[2].abs()) * cd[2].abs();
        let u = f64::EPSILON / 2.0;
        // Six triple products and their subtractions give 7u at first
        // order; the remaining terms are bounded by 56u^2.
        let bound = (7.0 + 56.0 * u) * u * permanent;
        if determinant.abs() > bound {
            return Ok(sign(determinant));
        }
    }
    let ad: [Expansion; 3] = std::array::from_fn(|i| Expansion::difference(a[i], d[i]));
    let bd: [Expansion; 3] = std::array::from_fn(|i| Expansion::difference(b[i], d[i]));
    let cd: [Expansion; 3] = std::array::from_fn(|i| Expansion::difference(c[i], d[i]));
    let minor = |i: usize, j: usize| bd[i].mul(&cd[j]).sub(bd[j].mul(&cd[i]));
    Ok(ad[0]
        .mul(&minor(1, 2))
        .sub(ad[1].mul(&minor(0, 2)))
        .add(ad[2].mul(&minor(0, 1)))
        .sign())
}

fn validate<const N: usize>(points: &[[f64; N]]) -> Result<(), PredicateError> {
    if points.iter().flatten().all(|value| value.is_finite()) {
        Ok(())
    } else {
        Err(PredicateError::NonFiniteCoordinate)
    }
}

fn filter_range<const N: usize>(vectors: &[[f64; N]]) -> bool {
    // A conservative arithmetic window, not an input limit. Degree <= 3
    // leaves room for cancellation, rounding residuals and the error bound
    // within the normal range. Outside it we use exponent-tagged expansions.
    const EXPONENT: i32 = f64::MAX_EXP / 4;
    const MIN: f64 = f64::from_bits(((1023 - EXPONENT) as u64) << 52);
    const MAX: f64 = f64::from_bits(((1023 + EXPONENT) as u64) << 52);
    vectors
        .iter()
        .flatten()
        .all(|value| *value == 0.0 || (MIN..=MAX).contains(&value.abs()))
}

fn sign(value: f64) -> i8 {
    if value > 0.0 {
        1
    } else if value < 0.0 {
        -1
    } else {
        0
    }
}

/// The exact value is `mantissa * 2^exponent`; nonzero mantissas have
/// magnitude in [1,2). Exponents cover products of three finite coordinates
/// and their residuals, comfortably within i32.
#[derive(Clone, Copy)]
struct Term {
    mantissa: f64,
    exponent: i32,
}

impl Term {
    fn new(mut value: f64, mut exponent: i32) -> Self {
        if value == 0.0 {
            return Self {
                mantissa: 0.0,
                exponent: 0,
            };
        }
        if value.abs() < f64::MIN_POSITIVE {
            // Exact rescaling before decoding a subnormal's significand.
            value *= (1_u64 << 52) as f64;
            exponent -= 52;
        }
        let bits = value.to_bits();
        let biased = ((bits >> 52) & 0x7ff) as i32;
        Self {
            mantissa: f64::from_bits((bits & !(0x7ff_u64 << 52)) | (1023_u64 << 52)),
            exponent: exponent + biased - 1023,
        }
    }

    fn neg(self) -> Self {
        Self {
            mantissa: -self.mantissa,
            ..self
        }
    }

    /// Error-free sum: high + low is exactly a + b.
    fn sum(mut a: Self, mut b: Self) -> (Self, Self) {
        if a.mantissa == 0.0 {
            return (b, a);
        }
        if b.mantissa == 0.0 {
            return (a, b);
        }
        if a.exponent < b.exponent {
            std::mem::swap(&mut a, &mut b);
        }
        let gap = a.exponent - b.exponent;
        if gap > f64::MANTISSA_DIGITS as i32 + 1 {
            // b is strictly below half an ulp of a, even at a binade edge.
            return (a, b);
        }
        let x = a.mantissa;
        let y = b.mantissa * f64::from_bits(((1023 - gap) as u64) << 52);
        let high = x + y;
        // Recover each rounded-away portion without assuming |x| >= |y|.
        let virtual_y = high - x;
        let low = (x - (high - virtual_y)) + (y - virtual_y);
        (Self::new(high, a.exponent), Self::new(low, a.exponent))
    }
}

/// Increasing-magnitude, nonoverlapping components; zeros are eliminated.
#[derive(Default)]
struct Expansion(Vec<Term>);

impl Expansion {
    fn difference(a: f64, b: f64) -> Self {
        let mut result = Self::default();
        result.grow(Term::new(a, 0));
        result.grow(Term::new(-b, 0));
        result
    }

    fn grow(&mut self, mut carry: Term) {
        let mut written = 0;
        for index in 0..self.0.len() {
            let (high, low) = Term::sum(carry, self.0[index]);
            if low.mantissa != 0.0 {
                self.0[written] = low;
                written += 1;
            }
            carry = high;
        }
        self.0.truncate(written);
        if carry.mantissa != 0.0 {
            self.0.push(carry);
        }
    }

    fn add(mut self, other: Self) -> Self {
        for term in other.0 {
            self.grow(term);
        }
        self
    }

    fn sub(mut self, other: Self) -> Self {
        for term in other.0 {
            self.grow(term.neg());
        }
        self
    }

    fn mul(&self, other: &Self) -> Self {
        let mut result = Self::default();
        for a in &self.0 {
            for b in &other.0 {
                // Normalized factors keep both the product and its exact
                // FMA residual representable, regardless of their exponents.
                let high = a.mantissa * b.mantissa;
                let low = a.mantissa.mul_add(b.mantissa, -high);
                let exponent = a.exponent + b.exponent;
                result.grow(Term::new(low, exponent));
                result.grow(Term::new(high, exponent));
            }
        }
        result
    }

    fn sign(&self) -> i8 {
        self.0.last().map_or(0, |term| sign(term.mantissa))
    }
}
