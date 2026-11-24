//! Triangle intersection with exact topology decisions on finite binary64 input.
//!
//! All geometric signs, including coordinate ordering, use `predicates`.
//! Intersection coordinates are rational numbers rounded to nearest binary64,
//! ties to even. Rounding can make distinct endpoints coincide; it never changes
//! the topology decision. No epsilon, coordinate band, or magnitude cap is used.
//!
//! Exact rational cut points are rounded coordinatewise to nearest, so the same
//! exact point always has the same binary64 coordinates and welds across operands.
//! Rounding can break incidence: exact edge points can fail the splitter's exact
//! on-segment or chart tests. Tags preserve the incidence decided before rounding,
//! giving faces sharing an edge the same placement without a geometric adjustment.
//! Distinct exact points that round together retain their own tags here; the
//! splitter owns their merged vertex and refuses merges that would crack an edge.

use crate::predicates::{orient2d, orient3d};
use crate::vec3::Vec3;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq)]
pub enum Intersection {
    None,
    /// A point contact is represented by equal endpoints.
    Segment(Vec3, Vec3),
    /// Boundary order, starting at the lexicographically smallest rounded point.
    /// Either winding is possible. Collinear contacts use `Segment` instead.
    CoplanarOverlap(Vec<Vec3>),
}

/// Where a point lies, in the argument's original vertex order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Feature {
    /// An input vertex.
    Vertex(usize),
    /// An input edge, with its smaller local index first.
    Edge(usize, usize),
    /// The relative interior of the triangle.
    Face,
}

/// A rounded point with exact incidence on the first and second arguments.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TaggedPoint {
    /// Rounded coordinates.
    pub point: Vec3,
    /// Incidence in argument order.
    pub on: [Feature; 2],
}

/// Intersection topology with each exact point's incidence retained separately.
#[derive(Clone, Debug, PartialEq)]
pub enum TaggedIntersection {
    /// No contact.
    None,
    /// Closed segment, including a point contact.
    Segment(TaggedPoint, TaggedPoint),
    /// Corners in the same boundary order as `Intersection`.
    CoplanarOverlap(Vec<TaggedPoint>),
}

impl TaggedIntersection {
    /// Remove incidence without changing points, duplicates or ordering.
    pub fn untagged(&self) -> Intersection {
        match self {
            Self::None => Intersection::None,
            Self::Segment(p, q) => Intersection::Segment(p.point, q.point),
            Self::CoplanarOverlap(poly) => {
                Intersection::CoplanarOverlap(poly.iter().map(|p| p.point).collect())
            }
        }
    }

    /// Exchange the two arguments' incidence, leaving the geometry unchanged.
    pub fn swapped(&self) -> TaggedIntersection {
        self.map_points(|mut p| {
            p.on.swap(0, 1);
            p
        })
    }

    fn map_points(&self, mut map: impl FnMut(TaggedPoint) -> TaggedPoint) -> Self {
        match self {
            Self::None => Self::None,
            Self::Segment(p, q) => Self::Segment(map(*p), map(*q)),
            Self::CoplanarOverlap(poly) => {
                Self::CoplanarOverlap(poly.iter().copied().map(map).collect())
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntersectionError {
    NonFiniteCoordinate,
    DegenerateTriangle,
    InvalidConstruction,
}

impl std::fmt::Display for IntersectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for IntersectionError {}

type Point = [f64; 3];
type Triangle = [Point; 3];

/// Intersect two closed, nondegenerate triangles.
///
/// Invalid input is a named error, not an empty intersection. Vertex order and
/// triangle order do not affect the result, including its boundary ordering.
/// The only inexact operation is the final rounding of constructed coordinates.
pub fn intersect_triangles(a: [Vec3; 3], b: [Vec3; 3]) -> Result<Intersection, IntersectionError> {
    intersect_triangles_tagged(a, b).map(|t| t.untagged())
}

/// Intersect triangles, retaining exact incidence in caller argument/vertex order.
/// Validation, rounded coordinates and topology match `intersect_triangles`.
pub fn intersect_triangles_tagged(
    a: [Vec3; 3],
    b: [Vec3; 3],
) -> Result<TaggedIntersection, IntersectionError> {
    let mut triangles = [a, b].map(|t| t.map(|v| [v.x, v.y, v.z]));
    let original = triangles;
    if !triangles.iter().flatten().flatten().all(|x| x.is_finite()) {
        return Err(IntersectionError::NonFiniteCoordinate);
    }
    for triangle in &mut triangles {
        for point in triangle.iter_mut() {
            // Canonicalize signed zero without changing its exact value.
            for coordinate in point {
                if *coordinate == 0.0 {
                    *coordinate = 0.0;
                }
            }
        }
        triangle.sort_by(compare);
        projection(triangle).ok_or(IntersectionError::DegenerateTriangle)?;
    }
    let swapped = compare_triangles(&triangles[0], &triangles[1]) == Ordering::Greater;
    if swapped {
        triangles.swap(0, 1);
    }
    let orders: [[usize; 3]; 2] = std::array::from_fn(|i| {
        triangles[i].map(|p| {
            original[i ^ usize::from(swapped)]
                .iter()
                .position(|&q| p == q)
                .expect("sorting preserves validated input vertices")
        })
    });
    let [a, b] = triangles;
    let sides = b.map(|p| side(a, p));
    let result = if sides == [0; 3] {
        coplanar(a, b)
    } else {
        let mut points = Vec::new();
        for (argument, (triangle, other)) in [(a, b), (b, a)].into_iter().enumerate() {
            let axes = projection(&other).ok_or(IntersectionError::DegenerateTriangle)?;
            let distances = triangle.map(|p| side(other, p));
            for i in 0..3 {
                let j = (i + 1) % 3;
                if distances[i] == 0 && inside(other, triangle[i], axes) {
                    let mut on = [Feature::Face; 2];
                    on[argument] = Feature::Vertex(i);
                    on[1 - argument] = feature(std::array::from_fn(|k| {
                        on_edge(triangle[i], other[k], other[(k + 1) % 3], axes)
                    }));
                    points.push(TaggedPoint {
                        point: vector(triangle[i]),
                        on,
                    });
                }
                if distances[i] * distances[j] == -1 {
                    let signs = std::array::from_fn::<_, 3, _>(|k| {
                        orientation3(triangle[i], triangle[j], other[k], other[(k + 1) % 3])
                    });
                    if signs.iter().all(|s| *s >= 0) || signs.iter().all(|s| *s <= 0) {
                        let weights = [
                            determinant3(other, triangle[j]).absolute(),
                            determinant3(other, triangle[i]).absolute(),
                        ];
                        let mut on = [Feature::Face; 2];
                        on[argument] = Feature::Edge(i.min(j), i.max(j));
                        on[1 - argument] = feature(signs.map(|s| s == 0));
                        points.push(TaggedPoint {
                            point: vector(construct(triangle[i], triangle[j], weights)?),
                            on,
                        });
                    }
                }
            }
        }
        points.sort_by(|p, q| {
            compare(
                &[p.point.x, p.point.y, p.point.z],
                &[q.point.x, q.point.y, q.point.z],
            )
        });
        Ok(match (points.first(), points.last()) {
            (Some(&start), Some(&end)) => TaggedIntersection::Segment(start, end),
            _ => TaggedIntersection::None,
        })
    }?;
    Ok(result.map_points(|mut p| {
        for (i, order) in orders.iter().enumerate() {
            p.on[i] = match p.on[i] {
                Feature::Vertex(k) => Feature::Vertex(order[k]),
                Feature::Edge(k, l) => {
                    Feature::Edge(order[k].min(order[l]), order[k].max(order[l]))
                }
                Feature::Face => Feature::Face,
            };
        }
        if swapped {
            p.on.swap(0, 1);
        }
        p
    }))
}

fn feature(edges: [bool; 3]) -> Feature {
    for (k, &incident) in edges.iter().enumerate() {
        if incident && edges[(k + 2) % 3] {
            return Feature::Vertex(k);
        }
    }
    match edges.iter().position(|&e| e) {
        Some(k) => Feature::Edge(k.min((k + 1) % 3), k.max((k + 1) % 3)),
        None => Feature::Face,
    }
}

fn vector(p: Point) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn orientation2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> i8 {
    orient2d(a, b, c).expect("private coordinates have been validated as finite")
}

fn orientation3(a: Point, b: Point, c: Point, d: Point) -> i8 {
    orient3d(a, b, c, d).expect("private coordinates have been validated as finite")
}

fn side(t: Triangle, p: Point) -> i8 {
    orientation3(t[0], t[1], t[2], p)
}

fn compare(a: &Point, b: &Point) -> Ordering {
    for (&x, &y) in a.iter().zip(b) {
        // This determinant has sign(x-y), without a rounded subtraction.
        let sign = orientation2([y, 0.0], [x, 0.0], [0.0, 1.0]);
        if sign != 0 {
            return sign.cmp(&0);
        }
    }
    Ordering::Equal
}

fn compare_triangles(a: &Triangle, b: &Triangle) -> Ordering {
    for (p, q) in a.iter().zip(b) {
        let order = compare(p, q);
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

fn project(p: Point, axes: [usize; 2]) -> [f64; 2] {
    axes.map(|i| p[i])
}

fn turn(a: Point, b: Point, c: Point, axes: [usize; 2]) -> i8 {
    orientation2(project(a, axes), project(b, axes), project(c, axes))
}

fn projection(t: &Triangle) -> Option<[usize; 2]> {
    [[0, 1], [0, 2], [1, 2]]
        .into_iter()
        .find(|&axes| turn(t[0], t[1], t[2], axes) != 0)
}

fn inside(t: Triangle, p: Point, axes: [usize; 2]) -> bool {
    let winding = turn(t[0], t[1], t[2], axes);
    (0..3).all(|i| turn(t[i], t[(i + 1) % 3], p, axes) * winding >= 0)
}

fn on_edge(p: Point, a: Point, b: Point, axes: [usize; 2]) -> bool {
    if turn(a, b, p, axes) != 0 {
        return false;
    }
    let (lo, hi) = if compare(&a, &b) == Ordering::Greater {
        (b, a)
    } else {
        (a, b)
    };
    compare(&lo, &p) != Ordering::Greater && compare(&p, &hi) != Ordering::Greater
}

struct Corner {
    point: Point,
    // Incidence on the original six edges, not on rounded constructed edges.
    edges: [bool; 6],
}

fn coplanar(a: Triangle, b: Triangle) -> Result<TaggedIntersection, IntersectionError> {
    let axes = projection(&a).ok_or(IntersectionError::DegenerateTriangle)?;
    let edges: [(Point, Point); 6] = std::array::from_fn(|i| {
        let t = if i < 3 { a } else { b };
        (t[i % 3], t[(i + 1) % 3])
    });
    let mut corners: Vec<Corner> = Vec::new();
    for p in a.into_iter().chain(b) {
        if inside(a, p, axes)
            && inside(b, p, axes)
            && !corners
                .iter()
                .any(|c| compare(&c.point, &p) == Ordering::Equal)
        {
            corners.push(Corner {
                point: p,
                edges: edges.map(|(u, v)| on_edge(p, u, v, axes)),
            });
        }
    }
    for (i, &(u, v)) in edges.iter().enumerate().take(3) {
        for (j, &(s, t)) in edges.iter().enumerate().skip(3) {
            if turn(u, v, s, axes) * turn(u, v, t, axes) == -1
                && turn(s, t, u, axes) * turn(s, t, v, axes) == -1
            {
                let weights = [
                    determinant2(s, t, v, axes).absolute(),
                    determinant2(s, t, u, axes).absolute(),
                ];
                let mut incidence = [false; 6];
                incidence[i] = true;
                incidence[j] = true;
                corners.push(Corner {
                    point: construct(u, v, weights)?,
                    edges: incidence,
                });
            }
        }
    }
    corners.sort_by(|a, b| compare(&a.point, &b.point));
    let tagged = |c: &Corner| TaggedPoint {
        point: vector(c.point),
        on: [
            feature([c.edges[0], c.edges[1], c.edges[2]]),
            feature([c.edges[3], c.edges[4], c.edges[5]]),
        ],
    };
    match corners.len() {
        0 => return Ok(TaggedIntersection::None),
        1 => {
            let p = tagged(&corners[0]);
            return Ok(TaggedIntersection::Segment(p, p));
        }
        2 => {
            return Ok(TaggedIntersection::Segment(
                tagged(&corners[0]),
                tagged(&corners[1]),
            ));
        }
        _ => {}
    }

    // Each contributing original edge contains exactly two boundary corners.
    // This graph orders the polygon without testing rounded coordinates.
    let mut neighbours = vec![Vec::new(); corners.len()];
    for edge in 0..6 {
        let incident: Vec<_> = corners
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.edges[edge].then_some(i))
            .collect();
        if let [u, v] = incident[..] {
            for (from, to) in [(u, v), (v, u)] {
                if !neighbours[from].contains(&to) {
                    neighbours[from].push(to);
                }
            }
        } else if incident.len() > 2 {
            return Err(IntersectionError::InvalidConstruction);
        }
    }
    if neighbours.iter().any(|n| n.len() != 2) {
        return Err(IntersectionError::InvalidConstruction);
    }
    let mut polygon = Vec::new();
    let mut previous = usize::MAX;
    let mut current = 0;
    for _ in 0..corners.len() {
        polygon.push(tagged(&corners[current]));
        let next = neighbours[current]
            .iter()
            .copied()
            .filter(|&n| n != previous)
            .min()
            .ok_or(IntersectionError::InvalidConstruction)?;
        previous = current;
        current = next;
        if current == 0 && polygon.len() != corners.len() {
            return Err(IntersectionError::InvalidConstruction);
        }
    }
    if current != 0 {
        return Err(IntersectionError::InvalidConstruction);
    }
    Ok(TaggedIntersection::CoplanarOverlap(polygon))
}

// Construction arithmetic only: signed base-2^32 integers in units of the
// smallest subnormal. Their signs are never used for geometric classification.
// Finite binary64 inputs and determinants of degree <= 3 bound every allocation.
#[derive(Clone, Default)]
struct Integer {
    negative: bool,
    limbs: Vec<u32>,
}

impl Integer {
    fn normalized(mut self) -> Self {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
        self.negative &= !self.limbs.is_empty();
        self
    }

    fn coordinate(value: f64) -> Self {
        let bits = value.to_bits();
        let exponent = ((bits >> 52) & 0x7ff) as usize;
        let fraction = bits & ((1_u64 << 52) - 1);
        let significand = if exponent == 0 {
            fraction
        } else {
            fraction | (1_u64 << 52)
        };
        let shift = exponent.saturating_sub(1);
        let shifted = u128::from(significand) << (shift % 32);
        let mut limbs = vec![0; shift / 32];
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

    fn absolute(mut self) -> Self {
        self.negative = false;
        self
    }

    fn magnitude_cmp(&self, other: &Self) -> Ordering {
        self.limbs
            .len()
            .cmp(&other.limbs.len())
            .then_with(|| self.limbs.iter().rev().cmp(other.limbs.iter().rev()))
    }

    fn add(&self, other: &Self) -> Self {
        let (large, small) = if self.magnitude_cmp(other) == Ordering::Less {
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

    fn sub(&self, other: &Self) -> Self {
        let mut negated = other.clone();
        negated.negative = !negated.negative;
        self.add(&negated)
    }

    fn mul(&self, other: &Self) -> Self {
        let mut limbs = vec![0; self.limbs.len() + other.limbs.len()];
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

    fn bits(&self) -> usize {
        self.limbs.last().map_or(0, |last| {
            self.limbs.len() * 32 - last.leading_zeros() as usize
        })
    }

    fn shifted(&self, shift: usize) -> Self {
        let mut limbs = vec![0; shift / 32];
        let mut carry = 0_u64;
        for &limb in &self.limbs {
            let value = (u64::from(limb) << (shift % 32)) | carry;
            limbs.push(value as u32);
            carry = value >> 32;
        }
        limbs.push(carry as u32);
        Self {
            negative: self.negative,
            limbs,
        }
        .normalized()
    }

    // Round (self / denominator) * 2^-1074 directly, including subnormals.
    fn rounded(&self, denominator: &Self) -> Result<f64, IntersectionError> {
        if denominator.limbs.is_empty() || denominator.negative {
            return Err(IntersectionError::InvalidConstruction);
        }
        if self.limbs.is_empty() {
            return Ok(0.0);
        }
        let mut remainder = self.clone().absolute();
        let mut exponent = remainder.bits() as i32 - denominator.bits() as i32;
        let order = if exponent >= 0 {
            remainder.magnitude_cmp(&denominator.shifted(exponent as usize))
        } else {
            remainder
                .shifted((-exponent) as usize)
                .magnitude_cmp(denominator)
        };
        if order == Ordering::Less {
            exponent -= 1;
        }
        let shift = (exponent - (f64::MANTISSA_DIGITS as i32 - 1)).max(0) as usize;
        let divisor = denominator.shifted(shift);
        let mut quotient = 0_u64;
        if remainder.magnitude_cmp(&divisor) != Ordering::Less {
            let bits = remainder.bits() - divisor.bits();
            if bits >= u64::BITS as usize {
                return Err(IntersectionError::InvalidConstruction);
            }
            for bit in (0..=bits).rev() {
                let term = divisor.shifted(bit);
                if remainder.magnitude_cmp(&term) != Ordering::Less {
                    remainder = remainder.sub(&term);
                    quotient |= 1_u64 << bit;
                }
            }
        }
        match remainder.shifted(1).magnitude_cmp(&divisor) {
            Ordering::Greater => quotient += 1,
            Ordering::Equal if quotient & 1 != 0 => quotient += 1,
            _ => {}
        }
        // The result is a convex combination of finite input coordinates.
        // These checks also keep exponent encoding below fail-closed.
        let power = shift as i32 - 1074;
        if power > 1023 {
            return Err(IntersectionError::InvalidConstruction);
        }
        let scale = if power < -1022 {
            f64::from_bits(1_u64 << (power + 1074))
        } else {
            f64::from_bits(((power + 1023) as u64) << 52)
        };
        let magnitude = quotient as f64 * scale;
        if !magnitude.is_finite() {
            return Err(IntersectionError::InvalidConstruction);
        }
        Ok(if self.negative && magnitude != 0.0 {
            -magnitude
        } else {
            magnitude
        })
    }
}

fn determinant2(a: Point, b: Point, c: Point, axes: [usize; 2]) -> Integer {
    let difference =
        |p: Point, i: usize| Integer::coordinate(p[axes[i]]).sub(&Integer::coordinate(c[axes[i]]));
    difference(a, 0)
        .mul(&difference(b, 1))
        .sub(&difference(a, 1).mul(&difference(b, 0)))
}

fn determinant3(t: Triangle, p: Point) -> Integer {
    let rows = t.map(|v| {
        std::array::from_fn::<_, 3, _>(|i| {
            Integer::coordinate(v[i]).sub(&Integer::coordinate(p[i]))
        })
    });
    let minor = |i: usize, j: usize| {
        rows[1][i]
            .mul(&rows[2][j])
            .sub(&rows[1][j].mul(&rows[2][i]))
    };
    rows[0][0]
        .mul(&minor(1, 2))
        .sub(&rows[0][1].mul(&minor(0, 2)))
        .add(&rows[0][2].mul(&minor(0, 1)))
}

fn construct(start: Point, end: Point, weights: [Integer; 2]) -> Result<Point, IntersectionError> {
    let denominator = weights[0].add(&weights[1]);
    let mut result = [0.0; 3];
    for (axis, coordinate) in result.iter_mut().enumerate() {
        let numerator = Integer::coordinate(start[axis])
            .mul(&weights[0])
            .add(&Integer::coordinate(end[axis]).mul(&weights[1]));
        *coordinate = numerator.rounded(&denominator)?;
    }
    Ok(result)
}
