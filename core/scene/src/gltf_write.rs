//! Deterministic glTF subset export, checked by the importer and scene comparator.

use crate::gltf::{import_gltf, GltfError};
use crate::hierarchy::HierarchyError;
use crate::json::{self, Value};
use crate::math::Quat;
use crate::scene::Scene;
use crate::usd::{compare_scenes, UsdError};

#[derive(Clone, Debug, PartialEq)]
pub enum GltfExportError {
    Layers,
    Instances,
    Hierarchy(HierarchyError),
    RotationNotFixed { node: usize },
    Json(json::Error),
    Reimport(GltfError),
    Mismatch(String),
}

impl std::fmt::Display for GltfExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for GltfExportError {}

fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    Value::Object(entries.into_iter().map(|(k, v)| (k.into(), v)).collect())
}

fn number(n: usize) -> Value {
    Value::Number(n as f64)
}

fn string(s: &str) -> Value {
    Value::String(s.into())
}

fn floats<const N: usize>(values: [f32; N]) -> Value {
    Value::Array(
        values
            .into_iter()
            .map(|v| Value::Number(f64::from(v)))
            .collect(),
    )
}

fn indices(values: impl IntoIterator<Item = usize>) -> Value {
    Value::Array(values.into_iter().map(number).collect())
}

fn view(offset: usize, length: usize, target: usize) -> Value {
    object([
        ("buffer", number(0)),
        ("byteOffset", number(offset)),
        ("byteLength", number(length)),
        ("target", number(target)),
    ])
}

/// Return compact JSON naming `bin_uri` and its binary buffer, or a named refusal.
/// Every successful export re-imports with exactly the source scene's bits.
pub fn export_gltf(scene: &Scene, bin_uri: &str) -> Result<(String, Vec<u8>), GltfExportError> {
    if !scene.layers.is_empty() {
        return Err(GltfExportError::Layers);
    }
    if !scene.instances.nodes().is_empty() {
        return Err(GltfExportError::Instances);
    }
    scene
        .hierarchy
        .world_matrices()
        .map_err(GltfExportError::Hierarchy)?;
    let nodes = &scene.hierarchy.nodes;
    for (node, entry) in nodes.iter().enumerate() {
        let c = entry.transform.rotation.components();
        if Quat::new(c[0], c[1], c[2], c[3])
            .is_none_or(|q| q.components().map(f32::to_bits) != c.map(f32::to_bits))
        {
            return Err(GltfExportError::RotationNotFixed { node });
        }
    }

    let mut children = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        if let Some(parent) = node.parent {
            // world_matrices has validated all parent indices.
            children[parent].push(i);
        } else {
            roots.push(i);
        }
    }
    let mut json_nodes = Vec::new();
    for (node, children) in nodes.iter().zip(children) {
        let mut entries = Vec::new();
        if !children.is_empty() {
            entries.push(("children", indices(children)));
        }
        if let Some(mesh) = node.mesh {
            entries.push(("mesh", Value::Number(f64::from(mesh))));
        }
        let t = node.transform.translation;
        let s = node.transform.scale;
        entries.extend([
            ("translation", floats([t.x, t.y, t.z])),
            ("rotation", floats(node.transform.rotation.components())),
            ("scale", floats([s.x, s.y, s.z])),
        ]);
        json_nodes.push(object(entries));
    }
    let mut doc = vec![
        (
            "asset",
            object([
                ("version", string("2.0")),
                ("generator", string("broadsword forge scene")),
            ]),
        ),
        ("scene", number(0)),
        (
            "scenes",
            Value::Array(vec![object([("nodes", indices(roots))])]),
        ),
        ("nodes", Value::Array(json_nodes)),
    ];
    let mut bin = Vec::new();
    let mut meshes = Vec::new();
    let mut accessors = Vec::new();
    let mut views = Vec::new();
    for mesh in &scene.meshes {
        let position_accessor = accessors.len();
        let offset = bin.len();
        // Empty geometry still builds JSON; the importer owns its refusal.
        let mut min = mesh.positions.first().copied().unwrap_or([0.0; 3]);
        let mut max = min;
        for position in &mesh.positions {
            for k in 0..3 {
                if position[k] < min[k] {
                    min[k] = position[k];
                }
                if position[k] > max[k] {
                    max[k] = position[k];
                }
                bin.extend_from_slice(&position[k].to_le_bytes());
            }
        }
        views.push(view(offset, bin.len() - offset, 34962));
        accessors.push(object([
            ("bufferView", number(position_accessor)),
            ("componentType", number(5126)),
            ("count", number(mesh.positions.len())),
            ("type", string("VEC3")),
            ("min", floats(min)),
            ("max", floats(max)),
        ]));
        let index_accessor = accessors.len();
        let offset = bin.len();
        for index in &mesh.indices {
            bin.extend_from_slice(&index.to_le_bytes());
        }
        views.push(view(offset, bin.len() - offset, 34963));
        accessors.push(object([
            ("bufferView", number(index_accessor)),
            ("componentType", number(5125)),
            ("count", number(mesh.indices.len())),
            ("type", string("SCALAR")),
        ]));
        meshes.push(object([(
            "primitives",
            Value::Array(vec![object([
                (
                    "attributes",
                    object([("POSITION", number(position_accessor))]),
                ),
                ("indices", number(index_accessor)),
                ("mode", number(4)),
            ])]),
        )]));
    }
    if !scene.meshes.is_empty() {
        doc.extend([
            ("meshes", Value::Array(meshes)),
            ("accessors", Value::Array(accessors)),
            ("bufferViews", Value::Array(views)),
            (
                "buffers",
                Value::Array(vec![object([
                    ("uri", string(bin_uri)),
                    ("byteLength", number(bin.len())),
                ])]),
            ),
        ]);
    }
    let text = json::write(&object(doc)).map_err(GltfExportError::Json)?;
    let back = import_gltf(text.as_bytes(), bin_uri, &bin).map_err(GltfExportError::Reimport)?;
    compare_scenes(scene, &back).map_err(|error| {
        GltfExportError::Mismatch(match error {
            UsdError::Mismatch { what } => what,
            other => format!("{other:?}"),
        })
    })?;
    Ok((text, bin))
}
