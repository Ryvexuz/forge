//! Canonical USD subset export: `#usda 1.0`, then one empty line before each
//! root or child block. Four-space indentation encloses `def Xform "n<i>"` or
//! `def Mesh "n<i>"`, with braces on separate lines. Every node writes translate,
//! orient (w first), scale, and that exact xformOpOrder. Meshes then write
//! faceVertexCounts (triangles), faceVertexIndices, and points. Lists and tuples
//! use `, ` separators, floats use shortest round-trip Display, and every line
//! ends in a newline without trailing spaces.
//!
//! Validation precedes output: the subset has no layers or instances; invalid
//! hierarchies are refused. Names and order come back from position, requiring
//! pre-order nodes and sequential, uniquely used meshes with none unused.
//! Invalid triangles, indices, and non-finite positions cannot import. Rotations
//! must be bitwise fixed points of Quat::new: about 1.4% of unit f32 quaternions
//! are not, so exporting them would change a value. Nesting beyond the importer's
//! shared MAX_DEPTH is also refused.
//!
//! The d11/d12 tests live in `core/scene/tests/usd_round_trip.rs` because the
//! import test file is at its 450-line stop line.

use crate::hierarchy::HierarchyError;
use crate::math::Quat;
use crate::scene::Scene;
use crate::usd::MAX_DEPTH;

#[derive(Clone, Debug, PartialEq)]
pub enum ExportError {
    /// The scene has layers; the subset has none.
    Layers,
    /// The scene has instances; the subset has none.
    Instances,
    /// World evaluation refused a cycle, bad parent, or non-finite transform.
    Hierarchy(HierarchyError),
    /// The pre-order walk visits this node at a different position.
    NodeOrder { node: usize },
    /// This node's mesh index differs from the number of preceding mesh nodes.
    MeshOrder { node: usize },
    /// Mesh nodes and meshes differ in number.
    MeshCount { used: usize, meshes: usize },
    /// Non-triangle indices, an out-of-range index, or a non-finite position.
    MeshInvalid { mesh: usize },
    /// Renormalizing this node's rotation would change its component bits.
    RotationNotFixed { node: usize },
    /// The first node whose nesting exceeds the importer's shared limit.
    TooDeep { node: usize },
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ExportError {}

/// The canonical `.usda` text of `scene`, or the first reason it cannot round-trip.
pub fn export_usda(scene: &Scene) -> Result<String, ExportError> {
    if !scene.layers.is_empty() {
        return Err(ExportError::Layers);
    }
    if !scene.instances.nodes().is_empty() {
        return Err(ExportError::Instances);
    }
    scene
        .hierarchy
        .world_matrices()
        .map_err(ExportError::Hierarchy)?;

    let nodes = &scene.hierarchy.nodes;
    let mut children = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            // world_matrices has already validated every parent and cycle.
            children[parent].push(i);
        } else {
            roots.push(i);
        }
    }
    let mut stack: Vec<_> = roots.into_iter().rev().map(|i| (i, 0)).collect();
    let mut depths = vec![0; nodes.len()];
    let mut position = 0;
    while let Some((node, depth)) = stack.pop() {
        if node != position {
            return Err(ExportError::NodeOrder { node });
        }
        depths[node] = depth;
        position += 1;
        stack.extend(children[node].iter().rev().map(|&i| (i, depth + 1)));
    }

    let mut used = 0;
    for (node, entry) in nodes.iter().enumerate() {
        if let Some(mesh) = entry.mesh {
            if usize::try_from(mesh).ok() != Some(used) {
                return Err(ExportError::MeshOrder { node });
            }
            used += 1;
        }
    }
    if used != scene.meshes.len() {
        return Err(ExportError::MeshCount {
            used,
            meshes: scene.meshes.len(),
        });
    }
    for (mesh, entry) in scene.meshes.iter().enumerate() {
        if !entry.indices.len().is_multiple_of(3)
            || entry.indices.iter().any(|&i| {
                usize::try_from(i)
                    .ok()
                    .is_none_or(|i| i >= entry.positions.len())
            })
            || entry.positions.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(ExportError::MeshInvalid { mesh });
        }
    }
    for (node, entry) in nodes.iter().enumerate() {
        let c = entry.transform.rotation.components();
        if Quat::new(c[0], c[1], c[2], c[3])
            .is_none_or(|q| q.components().map(f32::to_bits) != c.map(f32::to_bits))
        {
            return Err(ExportError::RotationNotFixed { node });
        }
    }
    for (node, &depth) in depths.iter().enumerate() {
        if depth >= MAX_DEPTH {
            return Err(ExportError::TooDeep { node });
        }
    }

    let mut text = String::from("#usda 1.0\n");
    let mut open = 0;
    for (i, (node, &depth)) in nodes.iter().zip(&depths).enumerate() {
        while open > depth {
            open -= 1;
            line(&mut text, &"    ".repeat(open), "}");
        }
        text.push('\n');
        let indent = "    ".repeat(depth);
        let kind = if node.mesh.is_some() { "Mesh" } else { "Xform" };
        line(&mut text, &indent, &format!("def {kind} \"n{i}\""));
        line(&mut text, &indent, "{");
        let indent = "    ".repeat(depth + 1);
        let t = node.transform.translation;
        let [x, y, z, w] = node.transform.rotation.components();
        let s = node.transform.scale;
        line(
            &mut text,
            &indent,
            &format!("float3 xformOp:translate = ({}, {}, {})", t.x, t.y, t.z),
        );
        line(
            &mut text,
            &indent,
            &format!("quatf xformOp:orient = ({w}, {x}, {y}, {z})"),
        );
        line(
            &mut text,
            &indent,
            &format!("float3 xformOp:scale = ({}, {}, {})", s.x, s.y, s.z),
        );
        line(
            &mut text,
            &indent,
            "uniform token[] xformOpOrder = [\"xformOp:translate\", \"xformOp:orient\", \"xformOp:scale\"]",
        );
        if let Some(mesh) = node.mesh {
            let mesh = &scene.meshes[mesh as usize];
            line(
                &mut text,
                &indent,
                &format!(
                    "int[] faceVertexCounts = {}",
                    list(mesh.indices.chunks_exact(3).map(|_| 3))
                ),
            );
            line(
                &mut text,
                &indent,
                &format!("int[] faceVertexIndices = {}", list(&mesh.indices)),
            );
            let points = list(
                mesh.positions
                    .iter()
                    .map(|[x, y, z]| format!("({x}, {y}, {z})")),
            );
            line(&mut text, &indent, &format!("point3f[] points = {points}"));
        }
        open = depth + 1;
    }
    while open > 0 {
        open -= 1;
        line(&mut text, &"    ".repeat(open), "}");
    }
    Ok(text)
}

fn line(text: &mut String, indent: &str, value: &str) {
    text.push_str(indent);
    text.push_str(value);
    text.push('\n');
}

fn list<T: std::fmt::Display>(values: impl IntoIterator<Item = T>) -> String {
    let mut text = String::from("[");
    for (i, value) in values.into_iter().enumerate() {
        if i != 0 {
            text.push_str(", ");
        }
        text.push_str(&format!("{value}"));
    }
    text.push(']');
    text
}
