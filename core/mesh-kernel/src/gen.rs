//! Deterministic, dependency-free indexed triangle fixtures.
//!
//! All input parameters are integers. Coordinates stay within 17/16 of the
//! origin on each axis for boxes; icosphere coordinates are normalized.
//! Refinements check geometry before inserting it. Resource exhaustion is an
//! error, not an implicit count cap.

use std::error::Error;
use std::fmt;

/// Indexed fixture geometry; triangles are outward-oriented.
#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<[f64; 3]>,
    pub triangles: Vec<[usize; 3]>,
}

/// Failure to construct a requested fixture without truncation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GenerateError {
    InvalidTriangleCount,
    CountOverflow,
    AllocationFailed,
    InvalidGeometry,
}

impl fmt::Display for GenerateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidTriangleCount => "triangle count must be even and at least twelve",
            Self::CountOverflow => "mesh size is not representable",
            Self::AllocationFailed => "mesh allocation failed",
            Self::InvalidGeometry => "refinement geometry is not representable",
        })
    }
}

impl Error for GenerateError {}

/// SplitMix64 with explicitly wrapping arithmetic and no external entropy.
#[derive(Clone, Debug)]
pub struct SeededRng {
    state: u64,
}

impl SeededRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.state;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }

    fn corner_offset(&mut self) -> f64 {
        // The high 53 bits convert exactly; all intermediates are bounded.
        let unit = (self.next_u64() >> 11) as f64 / ((1_u64 << 53) as f64);
        (unit * 2.0 - 1.0) / 16.0
    }
}

const BOX_VERTICES: [[f64; 3]; 8] = [
    [-1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0],
    [1.0, 1.0, -1.0],
    [-1.0, 1.0, -1.0],
    [-1.0, -1.0, 1.0],
    [1.0, -1.0, 1.0],
    [1.0, 1.0, 1.0],
    [-1.0, 1.0, 1.0],
];

const BOX_TRIANGLES: [[usize; 3]; 12] = [
    [0, 2, 1],
    [0, 3, 2],
    [4, 5, 6],
    [4, 6, 7],
    [0, 1, 5],
    [0, 5, 4],
    [3, 7, 6],
    [3, 6, 2],
    [0, 4, 7],
    [0, 7, 3],
    [1, 2, 6],
    [1, 6, 5],
];

fn reserved<T>(capacity: usize) -> Result<Vec<T>, GenerateError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| GenerateError::AllocationFailed)?;
    Ok(values)
}

/// Construct a closed box with exactly the requested even face count.
pub fn closed_box_with_triangles(n: usize) -> Result<Mesh, GenerateError> {
    refined_box(n, BOX_VERTICES)
}

fn refined_box(n: usize, corners: [[f64; 3]; 8]) -> Result<Mesh, GenerateError> {
    if n < BOX_TRIANGLES.len() || n & 1 != 0 {
        return Err(GenerateError::InvalidTriangleCount);
    }
    if !corners.iter().flatten().all(|value| value.is_finite())
        || BOX_TRIANGLES
            .iter()
            .any(|&[a, b, c]| !nondegenerate(corners[a], corners[b], corners[c]))
    {
        return Err(GenerateError::InvalidGeometry);
    }
    let splits = (n - BOX_TRIANGLES.len()) / 2;
    let vertex_count = BOX_VERTICES
        .len()
        .checked_add(splits)
        .ok_or(GenerateError::CountOverflow)?;
    let mut vertices = reserved(vertex_count)?;
    let mut triangles = reserved(n)?;
    vertices.extend_from_slice(&corners);
    triangles.extend_from_slice(&BOX_TRIANGLES);

    for cursor in 0..splits {
        let [a, b, c] = triangles[cursor];
        let center = std::array::from_fn(|axis| {
            (vertices[a][axis] + vertices[b][axis] + vertices[c][axis]) / 3.0
        });
        // Both finiteness and nonzero child areas are checked before mutation.
        if !center.iter().all(|value| value.is_finite())
            || !nondegenerate(vertices[a], vertices[b], center)
            || !nondegenerate(vertices[b], vertices[c], center)
            || !nondegenerate(vertices[c], vertices[a], center)
        {
            return Err(GenerateError::InvalidGeometry);
        }
        let middle = vertices.len();
        vertices.push(center);
        triangles[cursor] = [a, b, middle];
        triangles.push([b, c, middle]);
        triangles.push([c, a, middle]);
    }
    Ok(Mesh {
        vertices,
        triangles,
    })
}

fn nondegenerate(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> bool {
    let u: [f64; 3] = std::array::from_fn(|axis| b[axis] - a[axis]);
    let v: [f64; 3] = std::array::from_fn(|axis| c[axis] - a[axis]);
    let cross = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    cross.iter().all(|value| value.is_finite()) && cross.iter().any(|&value| value != 0.0)
}

/// Choose an even count in 12..=2,000, then refine a seeded perturbed box.
pub fn jittered_closed_mesh(seed: u64) -> Result<Mesh, GenerateError> {
    const MIN: usize = 12;
    const MAX: usize = 2_000;
    let mut rng = SeededRng::new(seed);
    let n = MIN + 2 * (rng.next_u64() % ((MAX - MIN) / 2 + 1) as u64) as usize;
    let mut corners = BOX_VERTICES;
    for corner in &mut corners {
        for coordinate in corner {
            *coordinate += rng.corner_offset();
        }
    }
    refined_box(n, corners)
}

fn normalized(point: [f64; 3]) -> Result<[f64; 3], GenerateError> {
    let length = point.iter().map(|value| value * value).sum::<f64>().sqrt();
    if !length.is_finite() || length == 0.0 {
        return Err(GenerateError::InvalidGeometry);
    }
    let result = point.map(|value| value / length);
    if !result.iter().all(|value| value.is_finite()) {
        return Err(GenerateError::InvalidGeometry);
    }
    Ok(result)
}

fn edge(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Construct a unit icosphere with `20 * 4^level` triangles.
pub fn icosphere(level: u32) -> Result<Mesh, GenerateError> {
    let face_count = 4_usize
        .checked_pow(level)
        .and_then(|factor| factor.checked_mul(20))
        .ok_or(GenerateError::CountOverflow)?;
    let vertex_count = (face_count / 2)
        .checked_add(2)
        .ok_or(GenerateError::CountOverflow)?;
    let mut vertices = reserved(vertex_count)?;
    let golden_ratio = (1.0 + 5.0_f64.sqrt()) / 2.0;
    for point in [
        [-1.0, golden_ratio, 0.0],
        [1.0, golden_ratio, 0.0],
        [-1.0, -golden_ratio, 0.0],
        [1.0, -golden_ratio, 0.0],
        [0.0, -1.0, golden_ratio],
        [0.0, 1.0, golden_ratio],
        [0.0, -1.0, -golden_ratio],
        [0.0, 1.0, -golden_ratio],
        [golden_ratio, 0.0, -1.0],
        [golden_ratio, 0.0, 1.0],
        [-golden_ratio, 0.0, -1.0],
        [-golden_ratio, 0.0, 1.0],
    ] {
        vertices.push(normalized(point)?);
    }
    let mut triangles = reserved(20)?;
    triangles.extend_from_slice(&[
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ]);

    for _ in 0..level {
        // Each occurrence carries its output slot; sorting shares an edge
        // without hashing, randomness, or iteration-order dependencies.
        // The checked final face count bounds these per-round products:
        // each round has at most final_count / 4 input faces.
        let mut edges = reserved(triangles.len() * 3)?;
        for (index, &[a, b, c]) in triangles.iter().enumerate() {
            let triangle_edges = [edge(a, b), edge(b, c), edge(c, a)];
            for (offset, (u, v)) in triangle_edges.into_iter().enumerate() {
                edges.push((u, v, index * 3 + offset));
            }
        }
        edges.sort_unstable();
        let mut midpoints = reserved(edges.len())?;
        midpoints.resize(edges.len(), 0);
        let mut previous = None;
        let mut middle = 0;
        for (a, b, slot) in edges {
            if previous != Some((a, b)) {
                let point = std::array::from_fn(|axis| vertices[a][axis] + vertices[b][axis]);
                let point = normalized(point)?;
                middle = vertices.len();
                vertices.push(point);
                previous = Some((a, b));
            }
            midpoints[slot] = middle;
        }
        let mut next = reserved(triangles.len() * 4)?;
        for (index, &[a, b, c]) in triangles.iter().enumerate() {
            let [ab, bc, ca] = [
                midpoints[index * 3],
                midpoints[index * 3 + 1],
                midpoints[index * 3 + 2],
            ];
            next.extend_from_slice(&[[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        triangles = next;
    }
    Ok(Mesh {
        vertices,
        triangles,
    })
}
