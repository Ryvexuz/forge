//! Boolean composition of closed meshes, with exact-coordinate welding.

use crate::broadphase::candidate_pairs;
use crate::classify::{classify_regions, Side};
use crate::halfedge::{Mesh, MeshError};
use crate::intersect::{intersect_triangles_tagged, IntersectionError, TaggedIntersection};
use crate::split::{split_faces_tagged, SplitError};
use crate::vec3::Vec3;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    A,
    B,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BooleanError {
    /// A vertex coordinate is non-finite; checked before topology or geometry.
    NonFiniteInput { operand: Operand },
    /// The operand fails the closed edge and vertex-fan checks.
    InputNotWatertight { operand: Operand },
    /// Triangle intersection refused this broadphase candidate pair.
    Intersect {
        face_a: u32,
        face_b: u32,
        error: IntersectionError,
    },
    /// Splitting refused the operand's cuts; no snapping or retry is performed.
    Split { operand: Operand, error: SplitError },
    /// Mesh construction or representable mesh counts failed.
    Mesh(MeshError),
    /// The result fails the watertight gate.
    NotWatertight,
    /// A union or difference kept no face.
    EmptyResult,
}

impl fmt::Display for BooleanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for BooleanError {}

#[derive(Clone, Copy)]
enum Operation {
    Union,
    Intersection,
    Difference,
}

pub fn union(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    compose(a, b, Operation::Union)
}

pub fn intersection(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    compose(a, b, Operation::Intersection)
}

/// A minus B.
pub fn difference(a: &Mesh, b: &Mesh) -> Result<Mesh, BooleanError> {
    compose(a, b, Operation::Difference)
}

/// The single result gate: return a watertight mesh unchanged, or fail closed.
/// Empty meshes also fail; genuinely empty intersections bypass this gate.
pub fn watertight_gate(mesh: Mesh) -> Result<Mesh, BooleanError> {
    if mesh.is_watertight() {
        Ok(mesh)
    } else {
        Err(BooleanError::NotWatertight)
    }
}

fn validate(mesh: &Mesh, operand: Operand) -> Result<(), BooleanError> {
    if mesh
        .vertices
        .iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
    {
        return Err(BooleanError::NonFiniteInput { operand });
    }
    if !mesh.is_watertight() {
        return Err(BooleanError::InputNotWatertight { operand });
    }
    Ok(())
}

// Some(false) keeps the face, Some(true) keeps it with reversed winding.
// Boundary rows concern coplanar faces, whose hard-pair tests belong to M12.
fn keep(operation: Operation, operand: Operand, side: Side) -> Option<bool> {
    match (operation, operand, side) {
        (Operation::Union, Operand::A, Side::Outside | Side::OnBoundarySame)
        | (Operation::Union, Operand::B, Side::Outside)
        | (Operation::Intersection, Operand::A, Side::Inside | Side::OnBoundarySame)
        | (Operation::Intersection, Operand::B, Side::Inside)
        | (Operation::Difference, Operand::A, Side::Outside | Side::OnBoundaryOpposite) => {
            Some(false)
        }
        (Operation::Difference, Operand::B, Side::Inside) => Some(true),
        _ => None,
    }
}

fn compose(a: &Mesh, b: &Mesh, operation: Operation) -> Result<Mesh, BooleanError> {
    validate(a, Operand::A)?;
    validate(b, Operand::B)?;
    // Broadphase's public face indices are u32; refuse its overflow panic.
    for mesh in [a, b] {
        u32::try_from(mesh.faces.len()).map_err(|_| BooleanError::Mesh(MeshError::CountOverflow))?;
    }
    let (vertices_a, triangles_a) = a.to_triangles();
    let (vertices_b, triangles_b) = b.to_triangles();
    let mut cuts_a = vec![Vec::new(); a.faces.len()];
    let mut cuts_b = vec![Vec::new(); b.faces.len()];
    for (face_a, face_b) in candidate_pairs(a, b) {
        let cut = intersect_triangles_tagged(
            triangles_a[face_a as usize].map(|i| vertices_a[i]),
            triangles_b[face_b as usize].map(|i| vertices_b[i]),
        )
        .map_err(|error| BooleanError::Intersect {
            face_a,
            face_b,
            error,
        })?;
        if cut != TaggedIntersection::None {
            cuts_a[face_a as usize].push(cut.clone());
            cuts_b[face_b as usize].push(cut.swapped());
        }
    }
    let (split_a, seams_a) = split_faces_tagged(a, &cuts_a).map_err(|error| BooleanError::Split {
        operand: Operand::A,
        error,
    })?;
    let (split_b, seams_b) = split_faces_tagged(b, &cuts_b).map_err(|error| BooleanError::Split {
        operand: Operand::B,
        error,
    })?;
    let sides_a = classify_regions(&split_a, &seams_a, b);
    let sides_b = classify_regions(&split_b, &seams_b, a);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut indices = BTreeMap::new();
    for (mesh, sides, operand) in [
        (&split_a, sides_a, Operand::A),
        (&split_b, sides_b, Operand::B),
    ] {
        let (points, faces) = mesh.to_triangles();
        for (mut face, side) in faces.into_iter().zip(sides) {
            let Some(reverse) = keep(operation, operand, side) else {
                continue;
            };
            if reverse {
                face.swap(1, 2);
            }
            triangles.push(face.map(|i| {
                let p = points[i];
                let xyz = [p.x, p.y, p.z].map(|v| if v == 0.0 { 0.0 } else { v });
                *indices.entry(xyz.map(f64::to_bits)).or_insert_with(|| {
                    let index = vertices.len();
                    vertices.push(Vec3::new(xyz[0], xyz[1], xyz[2]));
                    index
                })
            }));
        }
    }
    if triangles.is_empty() {
        return match operation {
            Operation::Intersection => Ok(Mesh::default()),
            Operation::Union | Operation::Difference => Err(BooleanError::EmptyResult),
        };
    }
    let mesh = Mesh::from_triangles(vertices, &triangles).map_err(BooleanError::Mesh)?;
    watertight_gate(mesh)
}
