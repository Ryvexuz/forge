//! Catmull--Clark subdivision (Catmull and Clark, Computer-Aided Design 10(6),
//! 1978, pp. 350--355, "Recursively generated B-spline surfaces on arbitrary
//! topological meshes").
//!
//! Face points are the mean of the face corners. An interior edge point is
//! (P0 + P1 + F0 + F1)/4. At an interior vertex of valence n, the new point is
//! (F + 2R + (n-3)P)/n, where F is the mean incident face point and R the mean
//! incident edge midpoint. Each old corner produces a quad. This implementation
//! accepts closed manifold surfaces only; boundary rules are not inferred.
//! Coordinates must be finite. Weighted sums are evaluated after scaling by
//! their largest coordinate, avoiding intermediate overflow; subnormal rounding
//! still follows IEEE 754. Output positions are checked before being returned.

use crate::halfedge::{Mesh, MeshError};
use crate::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyMesh {
    pub vertices: Vec<Vec3>,
    pub faces: Vec<Vec<usize>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubdivError {
    Mesh(MeshError),
    InvalidFace,
    NotClosedManifold,
    NonFinitePosition,
}

impl std::fmt::Display for SubdivError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SubdivError {}

impl PolyMesh {
    /// Convert an unmodified, valid indexed triangle mesh.
    pub fn from_mesh(mesh: &Mesh) -> Self {
        let (vertices, triangles) = mesh.to_triangles();
        Self {
            vertices,
            faces: triangles.into_iter().map(Vec::from).collect(),
        }
    }

    /// Fan triangulation preserves winding; faces should be convex for a
    /// geometric (rather than merely topological) triangulation.
    pub fn to_mesh(&self) -> Result<Mesh, SubdivError> {
        let mut triangles = Vec::new();
        for face in &self.faces {
            if face.len() < 3 || face.iter().copied().collect::<BTreeSet<_>>().len() != face.len() {
                return Err(SubdivError::InvalidFace);
            }
            for i in 1..face.len() - 1 {
                triangles.push([face[0], face[i], face[i + 1]]);
            }
        }
        Mesh::from_triangles(self.vertices.clone(), &triangles).map_err(SubdivError::Mesh)
    }
}

fn edge(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

fn weighted(points: &[(Vec3, f64)]) -> Result<Vec3, SubdivError> {
    let coordinate = |axis: fn(Vec3) -> f64| {
        let scale = points
            .iter()
            .map(|&(p, _)| axis(p).abs())
            .fold(0.0_f64, f64::max);
        if scale == 0.0 {
            return 0.0;
        }
        let sum = points
            .iter()
            .map(|&(p, w)| (axis(p) / scale) * w)
            .sum::<f64>();
        // All callers supply convex weights. Clamp roundoff at the convex hull.
        sum.clamp(-1.0, 1.0) * scale
    };
    let p = Vec3::new(
        coordinate(|p| p.x),
        coordinate(|p| p.y),
        coordinate(|p| p.z),
    );
    if !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite() {
        return Err(SubdivError::NonFinitePosition);
    }
    Ok(p)
}

fn mean(points: &[Vec3]) -> Result<Vec3, SubdivError> {
    weighted(
        &points
            .iter()
            .map(|&p| (p, 1.0 / points.len() as f64))
            .collect::<Vec<_>>(),
    )
}

/// Checked subdivision; rejects invalid indices, non-finite coordinates and
/// non-closed topology, even when `levels` is zero. Input is never modified.
pub fn try_catmull_clark(mesh: &PolyMesh, levels: usize) -> Result<PolyMesh, SubdivError> {
    if !mesh.to_mesh()?.is_watertight() {
        return Err(SubdivError::NotClosedManifold);
    }
    let mut mesh = mesh.clone();
    for _ in 0..levels {
        let mut edges: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        let mut incident = vec![Vec::new(); mesh.vertices.len()];
        let mut face_points = Vec::new();
        for (fi, face) in mesh.faces.iter().enumerate() {
            face_points.push(mean(
                &face.iter().map(|&v| mesh.vertices[v]).collect::<Vec<_>>(),
            )?);
            for i in 0..face.len() {
                incident[face[i]].push(fi);
                edges
                    .entry(edge(face[i], face[(i + 1) % face.len()]))
                    .or_default()
                    .push(fi);
            }
        }
        let mut neighbors = vec![Vec::new(); mesh.vertices.len()];
        for &(a, b) in edges.keys() {
            let midpoint = mean(&[mesh.vertices[a], mesh.vertices[b]])?;
            neighbors[a].push(midpoint);
            neighbors[b].push(midpoint);
        }
        let mut vertices = Vec::new();
        for (i, &p) in mesh.vertices.iter().enumerate() {
            let n = incident[i].len() as f64;
            if n < 3.0 || incident[i].len() != neighbors[i].len() {
                return Err(SubdivError::NotClosedManifold);
            }
            let f = mean(
                &incident[i]
                    .iter()
                    .map(|&j| face_points[j])
                    .collect::<Vec<_>>(),
            )?;
            let r = mean(&neighbors[i])?;
            vertices.push(weighted(&[(f, 1.0 / n), (r, 2.0 / n), (p, (n - 3.0) / n)])?);
        }
        let mut edge_indices = BTreeMap::new();
        for (&(a, b), faces) in &edges {
            if faces.len() != 2 {
                return Err(SubdivError::NotClosedManifold);
            }
            edge_indices.insert((a, b), vertices.len());
            vertices.push(mean(&[
                mesh.vertices[a],
                mesh.vertices[b],
                face_points[faces[0]],
                face_points[faces[1]],
            ])?);
        }
        let face_start = vertices.len();
        vertices.extend(face_points);
        let mut faces = Vec::new();
        for (fi, face) in mesh.faces.iter().enumerate() {
            for i in 0..face.len() {
                faces.push(vec![
                    face[i],
                    edge_indices[&edge(face[i], face[(i + 1) % face.len()])],
                    face_start + fi,
                    edge_indices[&edge(face[(i + face.len() - 1) % face.len()], face[i])],
                ]);
            }
        }
        mesh = PolyMesh { vertices, faces };
    }
    Ok(mesh)
}

/// Subdivide a closed manifold. Panics with a named error for invalid input;
/// use `try_catmull_clark` when handling untrusted geometry.
pub fn catmull_clark(mesh: &PolyMesh, levels: usize) -> PolyMesh {
    try_catmull_clark(mesh, levels).expect("Catmull-Clark input validation failed")
}
