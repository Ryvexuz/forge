//! In-memory glTF 2.0 subset: one scene, one caller-supplied plain-name buffer,
//! triangle meshes with tightly packed FLOAT positions and unsigned indices.
//! Nodes use TRS or a positive-scale, orthogonal affine matrix. Names are checked
//! but not stored; accessor min/max are checked as numbers, not compared to data.
//! All unlisted keys and duplicate keys are refused, including on unused objects.
//! Validation order: JSON, document, asset, buffers, views, accessor fields,
//! meshes/data, nodes, scenes, then the shared hierarchy validator.

use crate::hierarchy::{Hierarchy, HierarchyError, Node};
use crate::json::{self, Value};
use crate::math::{Quat, Vec3};
use crate::scene::{Mesh, Scene};
use crate::transform::Transform;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum GltfError {
    Utf8,
    Json(json::Error),
    Unsupported(String),
    Invalid(String),
    BadIndex {
        kind: &'static str,
        index: usize,
    },
    BadAccessor {
        accessor: usize,
        reason: &'static str,
    },
    OutOfRange(String),
    BufferUri(String),
    BinLength {
        declared: usize,
        actual: usize,
    },
    Skew {
        node: usize,
    },
    Hierarchy(HierarchyError),
}

impl std::fmt::Display for GltfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for GltfError {}

fn invalid(path: &str) -> GltfError {
    GltfError::Invalid(path.into())
}

fn member(path: &str, key: &str) -> String {
    if path.is_empty() {
        key.into()
    } else {
        format!("{path}.{key}")
    }
}

// The only object/field gateway: validate keys before exposing any field lookup.
fn object<'a>(
    value: Option<&'a Value>,
    path: &str,
    allowed: &str,
) -> Result<impl Fn(&str) -> Option<&'a Value>, GltfError> {
    let Some(Value::Object(entries)) = value else {
        return Err(invalid(if path.is_empty() { "document" } else { path }));
    };
    let mut fields = BTreeMap::new();
    for (key, value) in entries {
        if fields.insert(key.as_str(), value).is_some() {
            return Err(invalid(&format!("{} duplicate", member(path, key))));
        }
    }
    for (key, _) in entries {
        if !allowed
            .split_whitespace()
            .any(|allowed| allowed == key.as_str())
        {
            return Err(GltfError::Unsupported(member(path, key)));
        }
    }
    if let Some(name) = fields.get("name") {
        string(Some(name), &member(path, "name"))?;
    }
    Ok(move |key: &str| fields.get(key).copied())
}

fn array<'a>(v: Option<&'a Value>, path: &str) -> Result<&'a [Value], GltfError> {
    match v {
        None => Ok(&[]),
        Some(Value::Array(a)) => Ok(a),
        _ => Err(invalid(path)),
    }
}

fn string<'a>(v: Option<&'a Value>, path: &str) -> Result<&'a str, GltfError> {
    match v {
        Some(Value::String(s)) => Ok(s),
        _ => Err(invalid(path)),
    }
}

// All integer-valued fields, including optional ones, pass through this rule.
fn number(v: Option<&Value>, path: &str) -> Result<usize, GltfError> {
    match v {
        Some(Value::Number(n))
            if n.is_finite() && (0.0..=f64::from(u32::MAX)).contains(n) && n.fract() == 0.0 =>
        {
            Ok(*n as usize)
        }
        _ => Err(invalid(path)),
    }
}

fn optional_number(v: Option<&Value>, path: &str) -> Result<Option<usize>, GltfError> {
    v.map(|v| number(Some(v), path)).transpose()
}

fn floats<const N: usize>(v: Option<&Value>, path: &str) -> Result<[f32; N], GltfError> {
    let a = array(v, path)?;
    if a.len() != N {
        return Err(invalid(path));
    }
    let mut result = [0.0; N];
    for (out, value) in result.iter_mut().zip(a) {
        let Value::Number(n) = value else {
            return Err(invalid(path));
        };
        *out = *n as f32;
        if !out.is_finite() {
            return Err(invalid(path));
        }
    }
    Ok(result)
}

fn index(i: usize, len: usize, kind: &'static str) -> Result<usize, GltfError> {
    if i < len {
        Ok(i)
    } else {
        Err(GltfError::BadIndex { kind, index: i })
    }
}

fn buffers(v: Option<&Value>, uri: &str, bin: &[u8]) -> Result<(), GltfError> {
    if uri.is_empty()
        || uri.len() > 255
        || uri.starts_with('.')
        || uri.contains("..")
        || !uri.ends_with(".bin")
        || !uri
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(GltfError::BufferUri(uri.into()));
    }
    let a = array(v, "buffers")?;
    if a.len() > 1 {
        return Err(GltfError::Unsupported(format!("buffers.len={}", a.len())));
    }
    let mut declared = 0;
    if let Some(v) = a.first() {
        let b = object(Some(v), "buffers[0]", "uri byteLength name")?;
        if b("uri").is_none() {
            return Err(GltfError::Unsupported("buffers[0] without uri".into()));
        }
        let named = string(b("uri"), "buffers[0].uri")?;
        if named != uri {
            return Err(GltfError::BufferUri(named.into()));
        }
        declared = number(b("byteLength"), "buffers[0].byteLength")?;
    }
    if declared != bin.len() {
        return Err(GltfError::BinLength {
            declared,
            actual: bin.len(),
        });
    }
    Ok(())
}

struct View {
    offset: usize,
    length: usize,
    stride: Option<usize>,
}

fn views(v: Option<&Value>, buffer_count: usize, bin: &[u8]) -> Result<Vec<View>, GltfError> {
    let mut result = Vec::new();
    for (i, v) in array(v, "bufferViews")?.iter().enumerate() {
        let p = format!("bufferViews[{i}]");
        let o = object(
            Some(v),
            &p,
            "buffer byteOffset byteLength byteStride target name",
        )?;
        index(
            number(o("buffer"), &member(&p, "buffer"))?,
            buffer_count,
            "buffer",
        )?;
        let offset = optional_number(o("byteOffset"), &member(&p, "byteOffset"))?.unwrap_or(0);
        let length = number(o("byteLength"), &member(&p, "byteLength"))?;
        let stride = optional_number(o("byteStride"), &member(&p, "byteStride"))?;
        if let Some(target) = optional_number(o("target"), &member(&p, "target"))? {
            if !matches!(target, 34962 | 34963) {
                return Err(invalid(&member(&p, "target")));
            }
        }
        if offset.checked_add(length).is_none_or(|end| end > bin.len()) {
            return Err(GltfError::OutOfRange(p));
        }
        result.push(View {
            offset,
            length,
            stride,
        });
    }
    Ok(result)
}

struct Accessor<'a> {
    view: usize,
    offset: usize,
    component: usize,
    count: usize,
    kind: &'a str,
}

fn accessors(v: Option<&Value>) -> Result<Vec<Accessor<'_>>, GltfError> {
    let mut result = Vec::new();
    for (i, v) in array(v, "accessors")?.iter().enumerate() {
        let p = format!("accessors[{i}]");
        let o = object(
            Some(v),
            &p,
            "bufferView byteOffset componentType count type normalized min max name",
        )?;
        if o("bufferView").is_none() {
            return Err(GltfError::Unsupported(format!("{p} without bufferView")));
        }
        let view = number(o("bufferView"), &member(&p, "bufferView"))?;
        let offset = optional_number(o("byteOffset"), &member(&p, "byteOffset"))?.unwrap_or(0);
        let component = number(o("componentType"), &member(&p, "componentType"))?;
        let count = number(o("count"), &member(&p, "count"))?;
        if count == 0 {
            return Err(bad_accessor(i, "count is zero"));
        }
        let kind = string(o("type"), &member(&p, "type"))?;
        match o("normalized") {
            None | Some(Value::Bool(false)) => {}
            Some(Value::Bool(true)) => {
                return Err(GltfError::Unsupported(format!("{p}.normalized=true")));
            }
            _ => return Err(invalid(&member(&p, "normalized"))),
        }
        for key in ["min", "max"] {
            if let Some(v) = o(key) {
                let path = member(&p, key);
                if !array(Some(v), &path)?
                    .iter()
                    .all(|n| matches!(n, Value::Number(n) if n.is_finite()))
                {
                    return Err(invalid(&path));
                }
            }
        }
        result.push(Accessor {
            view,
            offset,
            component,
            count,
            kind,
        });
    }
    Ok(result)
}

fn bad_accessor(accessor: usize, reason: &'static str) -> GltfError {
    GltfError::BadAccessor { accessor, reason }
}

// Check all arithmetic and slice before decoding or allocating from a count.
fn data<'a>(
    i: usize,
    a: &Accessor<'_>,
    views: &[View],
    bin: &'a [u8],
    size: usize,
    component: usize,
) -> Result<&'a [u8], GltfError> {
    let v = &views[index(a.view, views.len(), "bufferView")?];
    let range_error = || GltfError::OutOfRange(format!("accessors[{i}]"));
    let start = v.offset.checked_add(a.offset).ok_or_else(range_error)?;
    if !start.is_multiple_of(component) {
        return Err(bad_accessor(i, "misaligned"));
    }
    if let Some(stride) = v.stride {
        if stride != size {
            return Err(GltfError::Unsupported(format!(
                "bufferViews[{}].byteStride={stride}",
                a.view
            )));
        }
    }
    let length = a.count.checked_mul(size).ok_or_else(range_error)?;
    if a.offset
        .checked_add(length)
        .is_none_or(|end| end > v.length)
    {
        return Err(range_error());
    }
    let end = start.checked_add(length).ok_or_else(range_error)?;
    bin.get(start..end).ok_or_else(range_error)
}

fn meshes(
    v: Option<&Value>,
    accessors: &[Accessor<'_>],
    views: &[View],
    bin: &[u8],
) -> Result<Vec<Mesh>, GltfError> {
    let mut meshes = Vec::new();
    for (i, v) in array(v, "meshes")?.iter().enumerate() {
        let p = format!("meshes[{i}]");
        let m = object(Some(v), &p, "name primitives")?;
        let primitives = array(m("primitives"), &member(&p, "primitives"))?;
        if primitives.len() != 1 {
            return Err(GltfError::Unsupported(format!(
                "{p}.primitives.len={}",
                primitives.len()
            )));
        }
        let p = format!("{p}.primitives[0]");
        let o = object(primitives.first(), &p, "attributes indices mode")?;
        let mode = optional_number(o("mode"), &member(&p, "mode"))?.unwrap_or(4);
        if mode != 4 {
            return Err(GltfError::Unsupported(format!("{p}.mode={mode}")));
        }
        if o("indices").is_none() {
            return Err(GltfError::Unsupported(format!("{p} without indices")));
        }
        if o("attributes").is_none() {
            return Err(invalid(&member(&p, "attributes.POSITION")));
        }
        let attrs = object(o("attributes"), &member(&p, "attributes"), "POSITION")?;
        let pos = number(attrs("POSITION"), &member(&p, "attributes.POSITION"))?;
        let a = &accessors[index(pos, accessors.len(), "accessor")?];
        if a.kind != "VEC3" || a.component != 5126 {
            return Err(bad_accessor(pos, "position accessor must be VEC3 FLOAT"));
        }
        let bytes = data(pos, a, views, bin, 12, 4)?;
        let mut positions = Vec::new();
        for chunk in bytes.as_chunks::<12>().0 {
            let p: [f32; 3] = std::array::from_fn(|k| {
                let j = k * 4;
                f32::from_le_bytes([chunk[j], chunk[j + 1], chunk[j + 2], chunk[j + 3]])
            });
            if !p.iter().all(|v| v.is_finite()) {
                return Err(bad_accessor(pos, "non-finite position"));
            }
            positions.push(p);
        }
        let idx = number(o("indices"), &member(&p, "indices"))?;
        let a = &accessors[index(idx, accessors.len(), "accessor")?];
        if a.kind != "SCALAR" || !matches!(a.component, 5121 | 5123 | 5125) {
            return Err(bad_accessor(
                idx,
                "index accessor must be SCALAR UNSIGNED_BYTE, UNSIGNED_SHORT or UNSIGNED_INT",
            ));
        }
        if !a.count.is_multiple_of(3) {
            return Err(bad_accessor(idx, "index count not a multiple of 3"));
        }
        let size = match a.component {
            5121 => 1,
            5123 => 2,
            _ => 4,
        };
        let bytes = data(idx, a, views, bin, size, size)?;
        let mut indices = Vec::new();
        for c in bytes.chunks_exact(size) {
            let n = match size {
                1 => u32::from(c[0]),
                2 => u32::from(u16::from_le_bytes([c[0], c[1]])),
                _ => u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
            };
            if u64::from(n) >= positions.len() as u64 {
                return Err(bad_accessor(idx, "index >= vertex count"));
            }
            indices.push(n);
        }
        meshes.push(Mesh { positions, indices });
    }
    Ok(meshes)
}

// Inputs are finite f32. Products of up to three such values, even subnormals,
// fit in f64; this keeps column lengths, dot products and determinant in range.
fn matrix(a: [f32; 16], node: usize) -> Result<Transform, GltfError> {
    let skew = || GltfError::Skew { node };
    if a[3] != 0.0 || a[7] != 0.0 || a[11] != 0.0 || a[15] != 1.0 {
        return Err(skew());
    }
    let mut c = [[0.0_f64; 3]; 3];
    for (i, col) in c.iter_mut().enumerate() {
        for (j, v) in col.iter_mut().enumerate() {
            *v = f64::from(a[i * 4 + j]);
        }
    }
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let s = c.map(|v| dot(v, v).sqrt());
    if s.contains(&0.0) {
        return Err(skew());
    }
    for i in 0..3 {
        for j in i + 1..3 {
            if dot(c[i], c[j]).abs() > 1e-5 * s[i] * s[j] {
                return Err(skew());
            }
        }
    }
    let det = c[0][0] * (c[1][1] * c[2][2] - c[1][2] * c[2][1])
        - c[1][0] * (c[0][1] * c[2][2] - c[0][2] * c[2][1])
        + c[2][0] * (c[0][1] * c[1][2] - c[0][2] * c[1][1]);
    if det <= 0.0 {
        return Err(skew());
    }
    let scales = s.map(|v| v as f32);
    let path = format!("nodes[{node}].matrix");
    if !scales.iter().all(|v| v.is_finite()) {
        return Err(invalid(&path));
    }
    for (col, length) in c.iter_mut().zip(s) {
        for v in col {
            *v /= length;
        }
    }
    let r = |row: usize, col: usize| c[col][row];
    let candidates = [
        1.0 + r(0, 0) - r(1, 1) - r(2, 2),
        1.0 - r(0, 0) + r(1, 1) - r(2, 2),
        1.0 - r(0, 0) - r(1, 1) + r(2, 2),
        1.0 + r(0, 0) + r(1, 1) + r(2, 2),
    ];
    let mut k = 0;
    for i in 1..4 {
        if candidates[i] > candidates[k] {
            k = i;
        }
    }
    let mut q = [0.0; 4];
    q[k] = candidates[k].sqrt() * 0.5;
    let d = 4.0 * q[k];
    if k == 3 {
        q[0] = (r(2, 1) - r(1, 2)) / d;
        q[1] = (r(0, 2) - r(2, 0)) / d;
        q[2] = (r(1, 0) - r(0, 1)) / d;
    } else {
        let i = (k + 1) % 3;
        let j = (k + 2) % 3;
        q[i] = (r(i, k) + r(k, i)) / d;
        q[j] = (r(j, k) + r(k, j)) / d;
        q[3] = (r(j, i) - r(i, j)) / d;
    }
    let q = q.map(|v| v as f32);
    let rotation = Quat::new(q[0], q[1], q[2], q[3]).ok_or_else(|| invalid(&path))?;
    Ok(Transform {
        translation: Vec3::new(a[12], a[13], a[14]),
        rotation,
        scale: Vec3::new(scales[0], scales[1], scales[2]),
    })
}

fn nodes(v: Option<&Value>, mesh_count: usize) -> Result<Vec<Node>, GltfError> {
    let a = array(v, "nodes")?;
    let mut parents = vec![None; a.len()];
    let mut nodes = Vec::new();
    for (i, v) in a.iter().enumerate() {
        let p = format!("nodes[{i}]");
        let o = object(
            Some(v),
            &p,
            "name children mesh translation rotation scale matrix",
        )?;
        let mesh = optional_number(o("mesh"), &member(&p, "mesh"))?
            .map(|m| index(m, mesh_count, "mesh").map(|m| m as u32))
            .transpose()?;
        let mut transform = Transform::default();
        if o("matrix").is_some() {
            if ["translation", "rotation", "scale"]
                .iter()
                .any(|key| o(key).is_some())
            {
                return Err(invalid(&format!("{p}.matrix with TRS")));
            }
            transform = matrix(floats(o("matrix"), &member(&p, "matrix"))?, i)?;
        } else {
            if let Some(v) = o("translation") {
                let [x, y, z] = floats(Some(v), &member(&p, "translation"))?;
                transform.translation = Vec3::new(x, y, z);
            }
            if let Some(v) = o("rotation") {
                let path = member(&p, "rotation");
                let [x, y, z, w] = floats(Some(v), &path)?;
                transform.rotation = Quat::new(x, y, z, w).ok_or_else(|| invalid(&path))?;
            }
            if let Some(v) = o("scale") {
                let [x, y, z] = floats(Some(v), &member(&p, "scale"))?;
                transform.scale = Vec3::new(x, y, z);
            }
        }
        for (k, child) in array(o("children"), &member(&p, "children"))?
            .iter()
            .enumerate()
        {
            let child = index(
                number(Some(child), &format!("{p}.children[{k}]"))?,
                a.len(),
                "node",
            )?;
            if parents[child].replace(i).is_some() {
                return Err(invalid(&format!("nodes[{child}] has two parents")));
            }
        }
        nodes.push(Node {
            parent: None,
            mesh,
            transform,
        });
    }
    for (node, parent) in nodes.iter_mut().zip(parents) {
        node.parent = parent;
    }
    Ok(nodes)
}

fn scenes(v: Option<&Value>, selected: Option<&Value>, nodes: &[Node]) -> Result<(), GltfError> {
    let a = array(v, "scenes")?;
    if a.len() != 1 {
        return Err(GltfError::Unsupported(format!("scenes.len={}", a.len())));
    }
    index(optional_number(selected, "scene")?.unwrap_or(0), 1, "scene")?;
    let o = object(a.first(), "scenes[0]", "nodes name")?;
    let mut roots = vec![false; nodes.len()];
    for (k, v) in array(o("nodes"), "scenes[0].nodes")?.iter().enumerate() {
        let p = format!("scenes[0].nodes[{k}]");
        let i = index(number(Some(v), &p)?, nodes.len(), "node")?;
        if nodes[i].parent.is_some() {
            return Err(invalid(&format!("{p} has a parent")));
        }
        roots[i] = true;
    }
    for (i, n) in nodes.iter().enumerate() {
        if n.parent.is_none() && !roots[i] {
            return Err(invalid(&format!("nodes[{i}] is a root not in the scene")));
        }
    }
    Ok(())
}

/// `json`: .gltf bytes; `bin_uri`: the plain file name its one buffer must name;
/// `bin`: that buffer. No file or network IO: the caller reads both files.
pub fn import_gltf(json: &[u8], bin_uri: &str, bin: &[u8]) -> Result<Scene, GltfError> {
    let text = std::str::from_utf8(json).map_err(|_| GltfError::Utf8)?;
    let value = json::read(text).map_err(GltfError::Json)?;
    let doc = object(
        Some(&value),
        "",
        "asset scene scenes nodes meshes accessors bufferViews buffers",
    )?;
    let asset = object(
        doc("asset"),
        "asset",
        "version minVersion generator copyright",
    )?;
    for key in ["version", "minVersion"] {
        if key == "version" || asset(key).is_some() {
            let v = string(asset(key), &member("asset", key))?;
            if v != "2.0" {
                return Err(GltfError::Unsupported(format!("asset.{key}={v}")));
            }
        }
    }
    for key in ["generator", "copyright"] {
        if let Some(v) = asset(key) {
            string(Some(v), &member("asset", key))?;
        }
    }
    buffers(doc("buffers"), bin_uri, bin)?;
    let buffer_count = array(doc("buffers"), "buffers")?.len();
    let views = views(doc("bufferViews"), buffer_count, bin)?;
    let accessors = accessors(doc("accessors"))?;
    let meshes = meshes(doc("meshes"), &accessors, &views, bin)?;
    let nodes = nodes(doc("nodes"), meshes.len())?;
    scenes(doc("scenes"), doc("scene"), &nodes)?;
    let scene = Scene {
        hierarchy: Hierarchy { nodes },
        meshes,
        ..Scene::default()
    };
    scene
        .hierarchy
        .world_matrices()
        .map_err(GltfError::Hierarchy)?;
    Ok(scene)
}
