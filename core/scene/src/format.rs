//! Canonical schema-1 scene serialization.

use crate::hierarchy::Node;
use crate::layer::Layer;
use crate::math::{Quat, Vec3};
use crate::scene::Scene;
use crate::transform::Transform;

/// The schema this code writes.
pub const CURRENT_SCHEMA: u32 = 1;

/// Converts full text from `from` to `from + 1`.
#[derive(Clone, Copy)]
pub struct Migration {
    pub from: u32,
    pub step: fn(&str) -> Result<String, FormatError>,
}

/// Empty at schema 1; each future version adds one step.
pub const MIGRATIONS: &[Migration] = &[];

/// Names parallel the scene's arena nodes.
#[derive(Debug, Default)]
pub struct SceneFile {
    pub scene: Scene,
    pub node_names: Vec<String>,
}

/// Lines are one-based; comma fields are zero-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    MissingHeader,
    UnknownSchema {
        found: String,
    },
    MissingFinalNewline,
    UnknownKind {
        line: usize,
    },
    FieldCount {
        line: usize,
        expected: usize,
        found: usize,
    },
    BadNumber {
        line: usize,
        field: usize,
    },
    NonFinite {
        line: usize,
        field: usize,
    },
    NotUnitQuat {
        line: usize,
    },
    BadEscape {
        line: usize,
    },
    UnknownNode {
        line: usize,
        id: u32,
    },
    UnknownLayer {
        line: usize,
        id: u32,
    },
    OutOfOrder {
        line: usize,
    },
    MissingTransform {
        node: usize,
    },
    NameCount {
        nodes: usize,
        names: usize,
    },
    MeshNotStored {
        node: Option<usize>,
    },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FormatError {}

fn escape(name: &str) -> String {
    let end = name.trim_end_matches(' ').len();
    let mut result = String::new();
    for (i, c) in name.char_indices() {
        match c {
            '%' => result.push_str("%25"),
            ',' => result.push_str("%2C"),
            '\n' => result.push_str("%0A"),
            '\r' => result.push_str("%0D"),
            ' ' if i >= end => result.push_str("%20"),
            _ => result.push(c),
        }
    }
    result
}

fn unescape(name: &str, line: usize) -> Result<String, FormatError> {
    let bad = FormatError::BadEscape { line };
    if name.ends_with(' ') || name.contains('\r') {
        return Err(bad);
    }
    let mut result = String::new();
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            result.push(c);
            continue;
        }
        let decoded = match (chars.next(), chars.next()) {
            (Some('2'), Some('5')) => '%',
            (Some('2'), Some('C')) => ',',
            (Some('0'), Some('A')) => '\n',
            (Some('0'), Some('D')) => '\r',
            (Some('2'), Some('0')) => {
                if result.ends_with(' ')
                    || !chars.as_str().as_bytes().chunks(3).all(|p| p == b"%20")
                {
                    return Err(bad);
                }
                result.push(' ');
                for _ in chars.as_str().as_bytes().chunks(3) {
                    result.push(' ');
                }
                return Ok(result);
            }
            _ => return Err(bad),
        };
        result.push(decoded);
    }
    Ok(result)
}

fn id(text: &str, line: usize, field: usize) -> Result<u32, FormatError> {
    text.parse::<u32>()
        .ok()
        .filter(|value| value.to_string() == text)
        .ok_or(FormatError::BadNumber { line, field })
}

fn floats<const N: usize>(text: &str, line: usize, field: usize) -> Result<[f32; N], FormatError> {
    let parts: Vec<_> = text.split(' ').collect();
    let bad = FormatError::BadNumber { line, field };
    if parts.len() != N {
        return Err(bad);
    }
    let mut result = [0.0; N];
    for (out, part) in result.iter_mut().zip(parts) {
        if part.len() != 8
            || !part
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(bad);
        }
        *out = f32::from_bits(u32::from_str_radix(part, 16).map_err(|_| bad.clone())?);
        if !out.is_finite() {
            return Err(FormatError::NonFinite { line, field });
        }
    }
    Ok(result)
}

fn hex(values: &[f32]) -> String {
    values
        .iter()
        .map(|v| format!("{:08x}", v.to_bits()))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Writes canonical text, or the first refusal. The reader validates the result.
pub fn write_scene(file: &SceneFile) -> Result<String, FormatError> {
    let scene = &file.scene;
    if file.node_names.len() != scene.hierarchy.nodes.len() {
        return Err(FormatError::NameCount {
            nodes: scene.hierarchy.nodes.len(),
            names: file.node_names.len(),
        });
    }
    if !scene.meshes.is_empty() {
        return Err(FormatError::MeshNotStored { node: None });
    }
    if let Some(i) = scene.hierarchy.nodes.iter().position(|n| n.mesh.is_some()) {
        return Err(FormatError::MeshNotStored { node: Some(i) });
    }
    let mut text = format!("schema={CURRENT_SCHEMA}\n");
    for (i, node) in scene.hierarchy.nodes.iter().enumerate() {
        let parent = node.parent.map_or_else(|| "-".into(), |p| p.to_string());
        text.push_str(&format!(
            "node,{i},{parent},{}\n",
            escape(&file.node_names[i])
        ));
    }
    for (i, node) in scene.hierarchy.nodes.iter().enumerate() {
        let t = node.transform;
        text.push_str(&format!(
            "xform,{i},{},{},{}\n",
            hex(&[t.translation.x, t.translation.y, t.translation.z]),
            hex(&t.rotation.components()),
            hex(&[t.scale.x, t.scale.y, t.scale.z]),
        ));
    }
    for layer in &scene.layers {
        text.push_str(&format!(
            "layer,{},{},{}\n",
            layer.id,
            escape(&layer.name),
            u8::from(layer.visible)
        ));
    }
    let inst = &scene.instances;
    for i in 0..inst.angles().len() {
        text.push_str(&format!(
            "inst,{},{},{},{}\n",
            inst.layers()[i],
            inst.nodes()[i],
            hex(&[inst.angles()[i]]),
            hex(&[inst.speeds()[i]])
        ));
    }
    read_scene(&text)?;
    Ok(text)
}

/// Parses canonical text, migrating registered older schemas first.
pub fn read_scene(text: &str) -> Result<SceneFile, FormatError> {
    let header = text.split('\n').next().unwrap_or("");
    let found = header
        .strip_prefix("schema=")
        .ok_or(FormatError::MissingHeader)?;
    if found != CURRENT_SCHEMA.to_string() {
        let unknown = || FormatError::UnknownSchema {
            found: found.into(),
        };
        let version = found.parse::<u32>().map_err(|_| unknown())?;
        if version.to_string() != found
            || version >= CURRENT_SCHEMA
            || !(version..CURRENT_SCHEMA).all(|v| MIGRATIONS.iter().any(|m| m.from == v))
        {
            return Err(unknown());
        }
        let mut migrated = text.to_owned();
        for v in version..CURRENT_SCHEMA {
            let migration = MIGRATIONS.iter().find(|m| m.from == v).ok_or_else(unknown)?;
            migrated = (migration.step)(&migrated)?;
        }
        return read_scene(&migrated);
    }
    if !text.ends_with('\n') {
        return Err(FormatError::MissingFinalNewline);
    }
    let mut file = SceneFile::default();
    let mut references = Vec::new();
    let mut order = 0;
    let mut transforms = 0;
    for (offset, row) in text[..text.len() - 1].split('\n').skip(1).enumerate() {
        let line = offset + 2;
        let fields: Vec<_> = row.split(',').collect();
        let (kind, expected) = match fields[0] {
            "node" => (0, 4),
            "xform" => (1, 5),
            "layer" => (2, 4),
            "inst" => (3, 5),
            _ => return Err(FormatError::UnknownKind { line }),
        };
        if fields.len() != expected {
            return Err(FormatError::FieldCount {
                line,
                expected,
                found: fields.len(),
            });
        }
        let index = id(fields[1], line, 1)?;
        let mut misplaced = kind < order;
        match kind {
            0 => {
                let parent = if fields[2] == "-" {
                    None
                } else {
                    Some(id(fields[2], line, 2)?)
                };
                let name = unescape(fields[3], line)?;
                misplaced |= index as usize != file.scene.hierarchy.nodes.len();
                if let Some(parent) = parent {
                    references.push((line, parent, None));
                }
                file.node_names.push(name);
                file.scene.hierarchy.nodes.push(Node {
                    parent: parent.map(|p| p as usize),
                    mesh: None,
                    transform: Transform::default(),
                });
            }
            1 => {
                let t = floats::<3>(fields[2], line, 2)?;
                let q = floats::<4>(fields[3], line, 3)?;
                let rotation = Quat::new(q[0], q[1], q[2], q[3])
                    .filter(|r| r.components().map(f32::to_bits) == q.map(f32::to_bits))
                    .ok_or(FormatError::NotUnitQuat { line })?;
                let s = floats::<3>(fields[4], line, 4)?;
                misplaced |= index as usize != transforms;
                references.push((line, index, None));
                if let Some(node) = file.scene.hierarchy.nodes.get_mut(index as usize) {
                    node.transform = Transform {
                        translation: Vec3::new(t[0], t[1], t[2]),
                        rotation,
                        scale: Vec3::new(s[0], s[1], s[2]),
                    };
                }
                transforms += 1;
            }
            2 => {
                let name = unescape(fields[2], line)?;
                let visible = match fields[3] {
                    "0" => false,
                    "1" => true,
                    _ => return Err(FormatError::BadNumber { line, field: 3 }),
                };
                file.scene.layers.push(Layer {
                    id: index,
                    name,
                    visible,
                });
            }
            _ => {
                let node = id(fields[2], line, 2)?;
                let [angle] = floats::<1>(fields[3], line, 3)?;
                let [speed] = floats::<1>(fields[4], line, 4)?;
                file.scene
                    .instances
                    .push(index, node, angle, speed)
                    .map_err(|_| FormatError::NonFinite { line, field: 3 })?;
                references.push((line, node, Some(index)));
            }
        }
        if misplaced {
            return Err(FormatError::OutOfOrder { line });
        }
        order = kind;
    }
    for (line, id, layer) in references {
        if id as usize >= file.scene.hierarchy.nodes.len() {
            return Err(FormatError::UnknownNode { line, id });
        }
        if let Some(id) = layer {
            if !file.scene.layers.iter().any(|layer| layer.id == id) {
                return Err(FormatError::UnknownLayer { line, id });
            }
        }
    }
    if transforms < file.scene.hierarchy.nodes.len() {
        return Err(FormatError::MissingTransform { node: transforms });
    }
    Ok(file)
}
