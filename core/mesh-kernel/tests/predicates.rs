use mesh_kernel::predicates::{orient2d, orient3d, PredicateError};
use std::cmp::Ordering;

// Independent signed base-2^32 integer oracle. Coordinates are integers in
// units of 2^-1074, so even the smallest subnormal is represented exactly.
// The oracle uses homogeneous determinants, not the production translation,
// floating-point filters, FMA residuals, or expansion arithmetic.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct Big {
    negative: bool,
    limbs: Vec<u32>,
}

impl Big {
    fn normalized(mut self) -> Self {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        self.negative &= !self.limbs.is_empty();
        self
    }

    fn coordinate(value: f64) -> Self {
        assert!(value.is_finite(), "oracle requires finite coordinates");
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as usize;
        let fraction = bits & ((1_u64 << 52) - 1);
        let significand = if exponent == 0 {
            fraction
        } else {
            fraction | (1_u64 << 52)
        };
        let shift = exponent.saturating_sub(1);
        let mut limbs = vec![0; shift / 32];
        let shifted = u128::from(significand) << (shift % 32);
        limbs.extend([
            shifted as u32,
            (shifted >> 32) as u32,
            (shifted >> 64) as u32,
        ]);
        Self {
            negative: bits >> 63 != 0,
            limbs,
        }
        .normalized()
    }

    fn one() -> Self {
        Self {
            negative: false,
            limbs: vec![1],
        }
    }

    fn neg(mut self) -> Self {
        if !self.limbs.is_empty() {
            self.negative = !self.negative;
        }
        self
    }

    fn add(&self, other: &Self) -> Self {
        let order = self
            .limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()));
        let (large, small) = if order == Ordering::Less {
            (other, self)
        } else {
            (self, other)
        };
        let mut limbs = Vec::with_capacity(large.limbs.len() + 1);
        let mut carry = 0_u64;
        for (i, &left) in large.limbs.iter().enumerate() {
            let right = u64::from(*small.limbs.get(i).unwrap_or(&0));
            if self.negative == other.negative {
                let sum = u64::from(left) + right + carry;
                limbs.push(sum as u32);
                carry = sum >> 32;
            } else {
                let subtrahend = right + carry;
                limbs.push(u64::from(left).wrapping_sub(subtrahend) as u32);
                carry = u64::from(u64::from(left) < subtrahend);
            }
        }
        if self.negative == other.negative && carry != 0 {
            limbs.push(carry as u32);
        }
        Self {
            negative: large.negative,
            limbs,
        }
        .normalized()
    }

    fn mul(&self, other: &Self) -> Self {
        let mut limbs = vec![0_u32; self.limbs.len() + other.limbs.len()];
        for (i, &a) in self.limbs.iter().enumerate() {
            let mut carry = 0_u64;
            for (j, &b) in other.limbs.iter().enumerate() {
                let product = u64::from(a) * u64::from(b) + u64::from(limbs[i + j]) + carry;
                limbs[i + j] = product as u32;
                carry = product >> 32;
            }
            limbs[i + other.limbs.len()] = carry as u32;
        }
        Self {
            negative: self.negative != other.negative,
            limbs,
        }
        .normalized()
    }

    fn sign(&self) -> i8 {
        if self.limbs.is_empty() {
            0
        } else if self.negative {
            -1
        } else {
            1
        }
    }
}

fn determinant(matrix: &[Vec<Big>]) -> Big {
    if matrix.len() == 1 {
        return matrix[0][0].clone();
    }
    let mut result = Big::default();
    for (column, entry) in matrix[0].iter().enumerate() {
        let minor: Vec<Vec<Big>> = matrix[1..]
            .iter()
            .map(|row| {
                row.iter()
                    .enumerate()
                    .filter(|(i, _)| *i != column)
                    .map(|(_, value)| value.clone())
                    .collect()
            })
            .collect();
        let product = entry.mul(&determinant(&minor));
        result = result.add(&if column % 2 == 0 {
            product
        } else {
            product.neg()
        });
    }
    result
}

fn oracle<const N: usize>(points: &[[f64; N]]) -> i8 {
    assert_eq!(points.len(), N + 1);
    let matrix: Vec<Vec<Big>> = points
        .iter()
        .map(|point| {
            let mut row: Vec<Big> = point.iter().copied().map(Big::coordinate).collect();
            row.push(Big::one());
            row
        })
        .collect();
    determinant(&matrix).sign()
}

fn check2(label: &str, points: [[f64; 2]; 3]) {
    let [a, b, c] = points;
    let expected = oracle(&points);
    let actual = orient2d(a, b, c).expect("finite fixture");
    eprintln!("{label}: dimension=2 expected={expected} actual={actual}");
    assert_eq!(actual, expected, "{label}: {points:?}");
    assert_eq!(orient2d(b, a, c), Ok(-expected), "{label}: swap");
    assert_eq!(orient2d(b, c, a), Ok(expected), "{label}: cycle");
}

fn check3(label: &str, points: [[f64; 3]; 4]) {
    let [a, b, c, d] = points;
    let expected = oracle(&points);
    let actual = orient3d(a, b, c, d).expect("finite fixture");
    eprintln!("{label}: dimension=3 expected={expected} actual={actual}");
    assert_eq!(actual, expected, "{label}: {points:?}");
    assert_eq!(orient3d(b, a, c, d), Ok(-expected), "{label}: swap");
    assert_eq!(orient3d(b, c, a, d), Ok(expected), "{label}: cycle");
    assert_eq!(orient3d(d, b, c, a), Ok(-expected), "{label}: swap origin");
}

#[test]
fn orientation_predicate_exact_on_near_degenerate() {
    let up = f64::from_bits(1.0_f64.to_bits() + 1);
    let down = f64::from_bits(1.0_f64.to_bits() - 1);
    // (1+u)(1-u)-1 = -u^2, where u = 2^-52. Both rounded
    // products are 1, so a plain float determinant returns zero.
    let below = 1.0 - f64::EPSILON;
    let cancellation = [[up, 1.0], [1.0, below], [0.0, 0.0]];
    assert_eq!(oracle(&cancellation), -1);
    let [a, b, c] = cancellation;
    let plain = (a[0] - c[0]) * (b[1] - c[1]) - (a[1] - c[1]) * (b[0] - c[0]);
    assert_eq!(plain, 0.0, "mutation witness must cancel in binary64");
    check2("one-ulp line cancellation", cancellation);
    for y in [down, 1.0, up] {
        check2("on/one-ulp off line", [[0.0, 0.0], [1.0, 1.0], [1.0, y]]);
    }

    // Lift the cancellation into z=0, with the fourth point one ulp
    // (the smallest subnormal) above or below the plane. Replacing the
    // exact fallback by a float determinant loses this nonzero sign.
    let tiny = f64::from_bits(1);
    for height in [0.0, tiny, -tiny] {
        check3(
            "on/one-ulp off z=0",
            [
                [up, 1.0, 0.0],
                [1.0, below, 0.0],
                [0.0, 0.0, 0.0],
                [0.0, 0.0, height],
            ],
        );
    }
    // The same cancellation at ordinary z scale, without underflow.
    let lifted = [
        [up, 1.0, 1.0],
        [1.0, below, 1.0],
        [0.0, 0.0, 1.0],
        [0.0, 0.0, 0.0],
    ];
    assert_eq!(oracle(&lifted), -1);
    check3("ordinary-scale product cancellation", lifted);
    for height in [down, 1.0, up] {
        let points = [
            [up, 1.0, 1.0],
            [1.0, below, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, height],
        ];
        // All xy products in the plain translated determinant cancel,
        // although the oracle finds a nonzero sign off z=1. No underflow
        // is involved here, even for the cubic residual.
        let dz = 1.0 - height;
        let plain = dz * (up * below - 1.0);
        assert_eq!(plain, 0.0, "3d mutation witness must cancel");
        if height != 1.0 {
            assert_ne!(oracle(&points), 0, "off-plane oracle must be nonzero");
        }
        check3("on/one-ulp off z=1 with cancellation", points);
    }
    for height in [down, 1.0, up] {
        check3(
            "on/one-ulp off oblique plane",
            [
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 1.0],
                [0.0, 1.0, 1.0],
                [0.5, 0.5, height],
            ],
        );
    }
}

#[test]
fn exact_signs_across_the_finite_binary64_range() {
    let tiny = f64::from_bits(1);
    let largest_subnormal = f64::from_bits(f64::MIN_POSITIVE.to_bits() - 1);
    let maximum = f64::MAX;
    let previous = f64::from_bits(maximum.to_bits() - 1);
    for scale in [tiny, largest_subnormal, f64::MIN_POSITIVE, 1.0, maximum] {
        check2("scaled triangle", [[scale, 0.0], [0.0, scale], [0.0, 0.0]]);
        check3(
            "scaled tetrahedron",
            [
                [scale, 0.0, 0.0],
                [0.0, scale, 0.0],
                [0.0, 0.0, scale],
                [0.0, 0.0, 0.0],
            ],
        );
    }
    check2(
        "overflowing differences",
        [[maximum, maximum], [-maximum, maximum], [maximum, -maximum]],
    );
    check3(
        "overflowing differences",
        [
            [maximum, maximum, maximum],
            [-maximum, maximum, maximum],
            [maximum, -maximum, maximum],
            [maximum, maximum, -maximum],
        ],
    );
    check2(
        "one-ulp cancellation at maximum",
        [
            [maximum, previous],
            [previous, f64::from_bits(previous.to_bits() - 1)],
            [0.0, 0.0],
        ],
    );
    check2(
        "mixed exponents",
        [[maximum, tiny], [previous, tiny], [-maximum, -tiny]],
    );
    check3(
        "mixed exponents",
        [
            [maximum, tiny, 0.0],
            [previous, tiny, 0.0],
            [-maximum, -tiny, 0.0],
            [0.0, 0.0, tiny],
        ],
    );
    // Exact subtraction tails matter: rounding 1+tiny to 1 loses this sign.
    check2("subtraction tail", [[1.0, 1.0], [tiny, 0.0], [0.0, -tiny]]);
    check3(
        "subtraction tail",
        [
            [1.0, 1.0, 1.0],
            [tiny, 0.0, 1.0],
            [0.0, -tiny, 1.0],
            [0.0, 0.0, 0.0],
        ],
    );
    // Product residuals near both ends of the exponent range, including
    // both boundaries of the filter's conservative arithmetic window.
    for exponent in [-1000, -256, 256, 1000] {
        let scale = f64::from_bits(((1023 + exponent) as u64) << 52);
        let up = f64::from_bits(scale.to_bits() + 1);
        let below = f64::from_bits(scale.to_bits() - 2);
        check2(
            "scaled product cancellation",
            [[up, scale], [scale, below], [0.0, 0.0]],
        );
        check3(
            "scaled product cancellation",
            [
                [up, scale, scale],
                [scale, below, scale],
                [0.0, 0.0, scale],
                [0.0, 0.0, 0.0],
            ],
        );
    }
    check2("signed zeros", [[-0.0, 0.0]; 3]);
    check3("signed zeros", [[0.0, -0.0, 0.0]; 4]);
    check2("duplicate points", [[maximum, tiny]; 3]);
    check3("duplicate points", [[maximum, tiny, -maximum]; 4]);
}

#[test]
fn nonfinite_coordinates_are_named_errors_at_both_entry_points() {
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for point in 0..3 {
            for axis in 0..2 {
                let mut points = [[0.0; 2]; 3];
                points[point][axis] = invalid;
                let [a, b, c] = points;
                assert_eq!(orient2d(a, b, c), Err(PredicateError::NonFiniteCoordinate));
            }
        }
        for point in 0..4 {
            for axis in 0..3 {
                let mut points = [[0.0; 3]; 4];
                points[point][axis] = invalid;
                let [a, b, c, d] = points;
                assert_eq!(
                    orient3d(a, b, c, d),
                    Err(PredicateError::NonFiniteCoordinate)
                );
            }
        }
    }
}

#[test]
fn big_integer_oracle_carries_borrows_and_signs() {
    let integer = |value: i64| {
        Big {
            negative: value < 0,
            limbs: vec![
                value.unsigned_abs() as u32,
                (value.unsigned_abs() >> 32) as u32,
            ],
        }
        .normalized()
    };
    for a in [-4_294_967_296_i64, -1, 0, 1, 4_294_967_295] {
        for b in [-4_294_967_296_i64, -1, 0, 1, 4_294_967_295] {
            assert_eq!(integer(a).add(&integer(b)), integer(a + b));
            let product = i128::from(a) * i128::from(b);
            let magnitude = product.unsigned_abs();
            let expected = Big {
                negative: product < 0,
                limbs: (0..4).map(|i| (magnitude >> (32 * i)) as u32).collect(),
            }
            .normalized();
            assert_eq!(integer(a).mul(&integer(b)), expected);
        }
    }
    assert_eq!(Big::coordinate(f64::from_bits(1)), Big::one());
    assert_eq!(Big::coordinate(-f64::from_bits(1)), Big::one().neg());
    assert_eq!(Big::coordinate(-0.0), Big::default());
    assert_eq!(oracle(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]), 1);
    assert_eq!(
        oracle(&[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
        ]),
        -1
    );
}
