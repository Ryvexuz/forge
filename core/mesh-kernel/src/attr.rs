//! Named attributes and equally face-weighted normals.

use crate::halfedge::Mesh;
use crate::vec3::Vec3;
use std::collections::BTreeMap;
use std::fmt;

pub type Normal = Vec3;
pub type Uv = [f64; 2];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Domain {
    Vertex,
    /// Corner indices are half-edge indices.
    Corner,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Attribute {
    Normal(Vec<Normal>),
    Uv(Vec<Uv>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrError {
    NonFinite,
    InvalidTopology,
    DegenerateFace,
    CancelledNormal,
}

impl fmt::Display for AttrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for AttrError {}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct AttrStore {
    attributes: BTreeMap<(Domain, String), Attribute>,
}

impl AttrStore {
    /// Insert or rewrite a named channel. A failed validation preserves the store.
    /// Channel lengths are independent of a mesh; callers choose their domain size.
    pub fn insert(
        &mut self,
        domain: Domain,
        name: impl Into<String>,
        attribute: Attribute,
    ) -> Result<Option<Attribute>, AttrError> {
        let valid = match &attribute {
            Attribute::Normal(values) => values.iter().all(|&v| finite(v)),
            Attribute::Uv(values) => values.iter().flatten().all(|v| v.is_finite()),
        };
        if !valid {
            return Err(AttrError::NonFinite);
        }
        Ok(self.attributes.insert((domain, name.into()), attribute))
    }

    pub fn get(&self, domain: Domain, name: &str) -> Option<&Attribute> {
        self.attributes.get(&(domain, name.to_owned()))
    }

    /// Canonical little-endian stream: u64 channel count, then channels ordered
    /// by domain and UTF-8 name. Each has a domain byte (vertex=0, corner=1),
    /// u64 name byte length, name bytes, type byte (normal=0, UV=1),
    /// u64 element count, and packed f64 components in xyz or uv order.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(self.attributes.len() as u64).to_le_bytes());
        for ((domain, name), attribute) in &self.attributes {
            bytes.push(match domain {
                Domain::Vertex => 0,
                Domain::Corner => 1,
            });
            bytes.extend_from_slice(&(name.len() as u64).to_le_bytes());
            bytes.extend_from_slice(name.as_bytes());
            match attribute {
                Attribute::Normal(values) => {
                    bytes.push(0);
                    bytes.extend_from_slice(&(values.len() as u64).to_le_bytes());
                    for v in values {
                        for component in [v.x, v.y, v.z] {
                            bytes.extend_from_slice(&component.to_le_bytes());
                        }
                    }
                }
                Attribute::Uv(values) => {
                    bytes.push(1);
                    bytes.extend_from_slice(&(values.len() as u64).to_le_bytes());
                    for component in values.iter().flatten() {
                        bytes.extend_from_slice(&component.to_le_bytes());
                    }
                }
            }
        }
        bytes
    }
}

fn triangles(mesh: &Mesh) -> Result<Vec<[usize; 3]>, AttrError> {
    if mesh.vertices.iter().any(|&v| !finite(v)) {
        return Err(AttrError::NonFinite);
    }
    if mesh.faces.len().checked_mul(3) != Some(mesh.halfedges.len()) {
        return Err(AttrError::InvalidTopology);
    }
    let mut seen = vec![false; mesh.halfedges.len()];
    let mut result = Vec::with_capacity(mesh.faces.len());
    for (face, &start) in mesh.faces.iter().enumerate() {
        let mut current = start;
        let mut indices = [0; 3];
        for index in &mut indices {
            let h = mesh
                .halfedges
                .get(current)
                .ok_or(AttrError::InvalidTopology)?;
            if seen[current] || h.face != face || h.origin >= mesh.vertices.len() {
                return Err(AttrError::InvalidTopology);
            }
            seen[current] = true;
            *index = h.origin;
            current = h.next;
        }
        if current != start
            || indices[0] == indices[1]
            || indices[1] == indices[2]
            || indices[2] == indices[0]
        {
            return Err(AttrError::InvalidTopology);
        }
        result.push(indices);
    }
    Ok(result)
}

fn face_edges(a: Vec3, b: Vec3, c: Vec3) -> Result<(Vec3, Vec3), AttrError> {
    // First try differences directly to preserve small translated triangles.
    // If subtraction overflows, scale coordinates before subtracting instead.
    let mut u = b - a;
    let mut v = c - a;
    if !finite(u) || !finite(v) {
        let scale = [a, b, c]
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()])
            .fold(0.0_f64, f64::max);
        // All-zero coordinates make a zero-size triangle, not a division by zero.
        if scale == 0.0 {
            return Err(AttrError::DegenerateFace);
        }
        let scaled = |p: Vec3| Vec3::new(p.x / scale, p.y / scale, p.z / scale);
        u = scaled(b) - scaled(a);
        v = scaled(c) - scaled(a);
    }
    Ok((u, v))
}

/// Unnormalised cross product of two edges: its direction is the face normal
/// and its length is twice the face area (the area weight for vertex normals).
/// A zero-area face has weight zero, so it is the zero vector here.
fn face_area_normal(a: Vec3, b: Vec3, c: Vec3) -> Result<Vec3, AttrError> {
    let (u, v) = face_edges(a, b, c)?;
    let cross = u.cross(v);
    if !finite(cross) {
        return Err(AttrError::NonFinite);
    }
    Ok(cross)
}

fn face_normal(a: Vec3, b: Vec3, c: Vec3) -> Result<Normal, AttrError> {
    let (u, v) = face_edges(a, b, c)?;
    // Unit edges bound the cross product independently of coordinate magnitude.
    let normal = u.normalized().cross(v.normalized()).normalized();
    if !finite(normal) {
        return Err(AttrError::NonFinite);
    }
    if normal == Vec3::default() {
        return Err(AttrError::DegenerateFace);
    }
    Ok(normal)
}

/// Unit normals in face order. Finite coordinates are accepted at every scale.
/// As with ordinary f64 geometry, nearly collinear edges can lose their cross
/// product to rounding; a cross product rounded to zero is a DegenerateFace.
/// No angular accuracy guarantee is made for ill-conditioned triangles.
pub fn compute_face_normals(mesh: &Mesh) -> Result<Vec<Normal>, AttrError> {
    triangles(mesh)?
        .into_iter()
        .map(|[a, b, c]| face_normal(mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]))
        .collect()
}

/// Sum area-weighted (unnormalised cross product) face normals, then normalize. Zero-area faces add nothing.
/// Isolated vertices have zero normals; a nonempty fan whose sum is zero is reported rather than assigned a direction.
pub fn compute_vertex_normals(mesh: &Mesh) -> Result<Vec<Normal>, AttrError> {
    let faces = triangles(mesh)?;
    let mut sums = vec![Vec3::default(); mesh.vertices.len()];
    let mut used = vec![false; mesh.vertices.len()];
    for [a, b, c] in faces {
        let normal = face_area_normal(mesh.vertices[a], mesh.vertices[b], mesh.vertices[c])?;
        for i in [a, b, c] {
            sums[i] = sums[i] + normal;
            used[i] = true;
        }
    }
    for (sum, used) in sums.iter_mut().zip(used) {
        if !finite(*sum) {
            return Err(AttrError::NonFinite);
        }
        *sum = sum.normalized();
        if used && *sum == Vec3::default() {
            return Err(AttrError::CancelledNormal);
        }
    }
    Ok(sums)
}
