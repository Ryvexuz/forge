//! Indexed triangle topology. Boundary and non-manifold input is representable;
//! `is_watertight` is the separate, fail-closed topology predicate.

use crate::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HalfEdge {
    pub origin: usize,
    pub next: usize,
    pub twin: Option<usize>,
    pub face: usize,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub halfedges: Vec<HalfEdge>,
    /// One representative half-edge per triangular face.
    pub faces: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshError {
    NonFinitePosition,
    InvalidIndex,
    RepeatedVertex,
    CountOverflow,
}

impl fmt::Display for MeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for MeshError {}

impl Mesh {
    pub fn from_triangles(
        vertices: Vec<Vec3>,
        triangles: &[[usize; 3]],
    ) -> Result<Self, MeshError> {
        if vertices
            .iter()
            .any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite())
        {
            return Err(MeshError::NonFinitePosition);
        }
        triangles
            .len()
            .checked_mul(3)
            .ok_or(MeshError::CountOverflow)?;
        for &[a, b, c] in triangles {
            if [a, b, c].iter().any(|&i| i >= vertices.len()) {
                return Err(MeshError::InvalidIndex);
            }
            if a == b || b == c || c == a {
                return Err(MeshError::RepeatedVertex);
            }
        }
        let mut mesh = Self {
            vertices,
            ..Self::default()
        };
        let mut edges: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for (face, triangle) in triangles.iter().enumerate() {
            let base = mesh.halfedges.len();
            mesh.faces.push(base);
            for offset in 0..3 {
                let a = triangle[offset];
                let b = triangle[(offset + 1) % 3];
                edges
                    .entry((a.min(b), a.max(b)))
                    .or_default()
                    .push(base + offset);
                mesh.halfedges.push(HalfEdge {
                    origin: a,
                    next: base + (offset + 1) % 3,
                    twin: None,
                    face,
                });
            }
        }
        for group in edges.values() {
            if let &[a, b] = group.as_slice() {
                if mesh.halfedges[a].origin != mesh.halfedges[b].origin {
                    mesh.halfedges[a].twin = Some(b);
                    mesh.halfedges[b].twin = Some(a);
                }
            }
        }
        Ok(mesh)
    }

    /// Preserves input vertex and face order for unmodified meshes.
    pub fn to_triangles(&self) -> (Vec<Vec3>, Vec<[usize; 3]>) {
        let triangles = self
            .faces
            .iter()
            .map(|&start| {
                let a = &self.halfedges[start];
                let b = &self.halfedges[a.next];
                let c = &self.halfedges[b.next];
                [a.origin, b.origin, c.origin]
            })
            .collect();
        (self.vertices.clone(), triangles)
    }

    pub fn is_watertight(&self) -> bool {
        if self.faces.is_empty() || self.faces.len().checked_mul(3) != Some(self.halfedges.len()) {
            return false;
        }
        let mut visited = vec![false; self.halfedges.len()];
        let mut outgoing = vec![Vec::new(); self.vertices.len()];
        let mut edges: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for (face, &start) in self.faces.iter().enumerate() {
            let mut current = start;
            let mut corners = BTreeSet::new();
            for _ in 0..3 {
                let Some(h) = self.halfedges.get(current) else {
                    return false;
                };
                if visited[current] || h.face != face || h.origin >= outgoing.len() {
                    return false;
                }
                visited[current] = true;
                corners.insert(h.origin);
                outgoing[h.origin].push(current);
                let Some(next) = self.halfedges.get(h.next) else {
                    return false;
                };
                let Some(twin) = h.twin.and_then(|t| self.halfedges.get(t)) else {
                    return false;
                };
                if twin.twin != Some(current)
                    || twin.origin != next.origin
                    || self.halfedges.get(twin.next).map(|n| n.origin) != Some(h.origin)
                {
                    return false;
                }
                *edges
                    .entry((h.origin.min(next.origin), h.origin.max(next.origin)))
                    .or_default() += 1;
                current = h.next;
            }
            if current != start || corners.len() != 3 {
                return false;
            }
        }
        if edges.values().any(|&count| count != 2) {
            return false;
        }
        for fan in outgoing {
            let Some(&start) = fan.first() else {
                return false;
            };
            let mut current = start;
            for step in 0..fan.len() {
                // All half-edges and twin links have been validated above.
                let Some(twin) = self.halfedges[current].twin else {
                    return false;
                };
                current = self.halfedges[twin].next;
                if current == start && step + 1 != fan.len() {
                    return false;
                }
            }
            if current != start {
                return false;
            }
        }
        true
    }

    /// V - E + F, counting distinct undirected indexed edges.
    pub fn euler_characteristic(&self) -> i128 {
        let edges: BTreeSet<_> = self
            .halfedges
            .iter()
            .map(|h| {
                let end = self.halfedges[h.next].origin;
                (h.origin.min(end), h.origin.max(end))
            })
            .collect();
        self.vertices.len() as i128 - edges.len() as i128 + self.faces.len() as i128
    }

    /// Canonical stream: u64 vertex count, u64 face count, xyz f64 positions,
    /// then three u64 indices per face, all little-endian in input order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let (vertices, triangles) = self.to_triangles();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(vertices.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&(triangles.len() as u64).to_le_bytes());
        for v in vertices {
            for coordinate in [v.x, v.y, v.z] {
                bytes.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
        for triangle in triangles {
            for index in triangle {
                bytes.extend_from_slice(&(index as u64).to_le_bytes());
            }
        }
        bytes
    }
}
