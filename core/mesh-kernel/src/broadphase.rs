//! Uniform-grid broadphase with inclusive (touching counts) face AABBs.

use crate::halfedge::Mesh;
use crate::vec3::Vec3;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn overlaps(&self, other: &Self) -> bool {
        self.min.x <= other.max.x
            && other.min.x <= self.max.x
            && self.min.y <= other.max.y
            && other.min.y <= self.max.y
            && self.min.z <= other.max.z
            && other.min.z <= self.max.z
    }
}

fn components(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn face_bounds(mesh: &Mesh) -> Vec<Aabb> {
    assert!(u32::try_from(mesh.faces.len()).is_ok(), "FaceCountOverflow");
    mesh.faces
        .iter()
        .map(|&start| {
            let mut current = start;
            let mut min = [f64::INFINITY; 3];
            let mut max = [f64::NEG_INFINITY; 3];
            for _ in 0..3 {
                let edge = mesh.halfedges.get(current).expect("InvalidHalfEdge");
                let point = *mesh.vertices.get(edge.origin).expect("InvalidVertex");
                for (axis, value) in components(point).into_iter().enumerate() {
                    assert!(value.is_finite(), "NonFinitePosition");
                    min[axis] = min[axis].min(value);
                    max[axis] = max[axis].max(value);
                }
                current = edge.next;
            }
            assert_eq!(current, start, "NonTriangularFace");
            Aabb {
                min: Vec3::new(min[0], min[1], min[2]),
                max: Vec3::new(max[0], max[1], max[2]),
            }
        })
        .collect()
}

/// Return sorted, unique overlapping face-AABB pairs.
///
/// For N = max(face counts), cell width is the longest combined extent
/// divided by ceil(cuberoot(N));
/// degenerate combined bounds use one cell. Every touched cell is visited,
/// including the upper boundary, so no neighbour stencil is required.
/// Coordinates are divided by their largest absolute value before subtraction:
/// intermediates remain finite even for subnormals and opposite f64 extremes.
///
/// Panics with a named diagnostic for invalid face references or cycles, non-finite
/// referenced positions, or face counts not representable by the u32 API.
/// As with any exhaustive broadphase, dense overlap can require quadratic output.
pub fn candidate_pairs(a: &Mesh, b: &Mesh) -> Vec<(u32, u32)> {
    let a = face_bounds(a);
    let b = face_bounds(b);
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let scale = a
        .iter()
        .chain(&b)
        .flat_map(|bounds| {
            components(bounds.min)
                .into_iter()
                .chain(components(bounds.max))
        })
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    let scale = if scale == 0.0 { 1.0 } else { scale };
    let mut origin = [f64::INFINITY; 3];
    let mut upper = [f64::NEG_INFINITY; 3];
    for bounds in a.iter().chain(&b) {
        for axis in 0..3 {
            origin[axis] = origin[axis].min(components(bounds.min)[axis] / scale);
            upper[axis] = upper[axis].max(components(bounds.max)[axis] / scale);
        }
    }
    let extent = (0..3)
        .map(|axis| upper[axis] - origin[axis])
        .fold(0.0_f64, f64::max);
    let resolution = (a.len().max(b.len()) as f64).cbrt().ceil();
    let cell = |point: Vec3| -> [u32; 3] {
        std::array::from_fn(|axis| {
            if extent == 0.0 {
                0
            } else {
                (((components(point)[axis] / scale - origin[axis]) / extent) * resolution).floor()
                    as u32
            }
        })
    };
    let visit = |bounds: &Aabb, action: &mut dyn FnMut([u32; 3])| {
        let lo = cell(bounds.min);
        let hi = cell(bounds.max);
        for x in lo[0]..=hi[0] {
            for y in lo[1]..=hi[1] {
                for z in lo[2]..=hi[2] {
                    action([x, y, z]);
                }
            }
        }
    };
    let mut grid: HashMap<[u32; 3], Vec<usize>> = HashMap::new();
    for (index, bounds) in b.iter().enumerate() {
        visit(bounds, &mut |key| {
            grid.entry(key).or_default().push(index);
        });
    }
    let mut seen = vec![usize::MAX; b.len()];
    let mut pairs = Vec::new();
    for (i, bounds) in a.iter().enumerate() {
        visit(bounds, &mut |key| {
            if let Some(indices) = grid.get(&key) {
                for &j in indices {
                    if seen[j] != i {
                        seen[j] = i;
                        if bounds.overlaps(&b[j]) {
                            pairs.push((i as u32, j as u32));
                        }
                    }
                }
            }
        });
    }
    pairs.sort_unstable();
    pairs
}
