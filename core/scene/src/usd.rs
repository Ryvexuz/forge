//! Strict ASCII USD subset import and bit-exact scene comparison, not full USD.
//! In addition to float3 TRS, accepts double3, quatf xformOp:orient, and requires
//! int[] faceVertexCounts (triangles only). Layers and composition are unsupported.

use crate::hierarchy::{HierarchyError, Node};
use crate::math::{Quat, Vec3};
use crate::scene::{Mesh, Scene};
use crate::transform::Transform;
use std::collections::HashSet;

/// Nesting limit for prims, the same value as `json::MAX_DEPTH`.
pub const MAX_DEPTH: usize = crate::json::MAX_DEPTH;
const OPS: [&str; 4] = [
    "xformOp:translate",
    "xformOp:rotateXYZ",
    "xformOp:orient",
    "xformOp:scale",
];

#[derive(Clone, Debug, PartialEq)]
pub enum UsdError {
    Header,
    Syntax { line: usize },
    Unsupported { line: usize, what: String },
    Invalid { line: usize, what: String },
    NonFinite { line: usize },
    TooDeep { line: usize },
    Hierarchy(HierarchyError),
    Mismatch { what: String },
}

impl std::fmt::Display for UsdError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for UsdError {}

fn unsupported(line: usize, what: impl Into<String>) -> UsdError {
    UsdError::Unsupported {
        line,
        what: what.into(),
    }
}
fn invalid(line: usize, what: impl Into<String>) -> UsdError {
    UsdError::Invalid {
        line,
        what: what.into(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Word,
    String,
    Number,
    Symbol,
    End,
}
#[derive(Clone, Copy)]
struct Token<'a> {
    kind: Kind,
    text: &'a str,
    line: usize,
}
struct Parser<'a> {
    text: &'a str,
    pos: usize,
    line: usize,
    look: Option<Token<'a>>,
}

impl<'a> Parser<'a> {
    fn digits(&mut self) -> bool {
        let start = self.pos;
        while self
            .text
            .as_bytes()
            .get(self.pos)
            .is_some_and(u8::is_ascii_digit)
        {
            self.pos += 1;
        }
        self.pos != start
    }
    fn lex(&mut self) -> Result<Token<'a>, UsdError> {
        let bytes = self.text.as_bytes();
        while let Some(&c) = bytes.get(self.pos) {
            if !matches!(c, b' ' | b'\t' | b'\n') {
                break;
            }
            self.line += usize::from(c == b'\n');
            self.pos += 1;
        }
        let start = self.pos;
        let line = self.line;
        let syntax = UsdError::Syntax { line };
        let Some(&c) = bytes.get(self.pos) else {
            return Ok(Token {
                kind: Kind::End,
                text: "",
                line,
            });
        };
        self.pos += 1;
        let kind = match c {
            b'#' => return Err(unsupported(line, "#")),
            b'{' | b'}' | b'(' | b')' | b'[' | b']' | b',' | b'=' => Kind::Symbol,
            b'"' => {
                while let Some(&c) = bytes.get(self.pos) {
                    if c == b'"' {
                        let text = &self.text[start + 1..self.pos];
                        self.pos += 1;
                        return Ok(Token {
                            kind: Kind::String,
                            text,
                            line,
                        });
                    }
                    if matches!(c, b'\\' | b'\n') {
                        return Err(syntax);
                    }
                    self.pos += 1;
                }
                return Err(syntax);
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                while bytes
                    .get(self.pos)
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':'))
                {
                    self.pos += 1;
                }
                if bytes.get(self.pos..self.pos + 2) == Some(b"[]") {
                    self.pos += 2;
                }
                Kind::Word
            }
            b'-' | b'0'..=b'9' => {
                self.pos = start + usize::from(c == b'-');
                if !self.digits() {
                    return Err(syntax);
                }
                if bytes.get(self.pos) == Some(&b'.') {
                    self.pos += 1;
                    if !self.digits() {
                        return Err(syntax);
                    }
                }
                if matches!(bytes.get(self.pos), Some(b'e' | b'E')) {
                    self.pos += 1;
                    if matches!(bytes.get(self.pos), Some(b'+' | b'-')) {
                        self.pos += 1;
                    }
                    if !self.digits() {
                        return Err(syntax);
                    }
                }
                Kind::Number
            }
            _ => return Err(syntax),
        };
        Ok(Token {
            kind,
            text: &self.text[start..self.pos],
            line,
        })
    }
    fn peek(&mut self) -> Result<Token<'a>, UsdError> {
        if self.look.is_none() {
            self.look = Some(self.lex()?);
        }
        self.look.ok_or(UsdError::Syntax { line: self.line })
    }
    fn take(&mut self) -> Result<Token<'a>, UsdError> {
        let token = self.peek()?;
        self.look = None;
        Ok(token)
    }
    fn kind(&mut self, kind: Kind) -> Result<Token<'a>, UsdError> {
        let token = self.take()?;
        if token.kind != kind {
            return Err(UsdError::Syntax { line: token.line });
        }
        Ok(token)
    }
    fn symbol(&mut self, text: &str) -> Result<(), UsdError> {
        let token = self.take()?;
        if token.kind != Kind::Symbol || token.text != text {
            return Err(UsdError::Syntax { line: token.line });
        }
        Ok(())
    }
    fn number(&mut self) -> Result<f32, UsdError> {
        let token = self.kind(Kind::Number)?;
        let value = token
            .text
            .parse::<f32>()
            .map_err(|_| UsdError::Syntax { line: token.line })?;
        if !value.is_finite() {
            return Err(UsdError::NonFinite { line: token.line });
        }
        Ok(value)
    }
    fn tuple<const N: usize>(&mut self) -> Result<[f32; N], UsdError> {
        self.symbol("(")?;
        let mut values = [0.0; N];
        for (i, value) in values.iter_mut().enumerate() {
            if i != 0 {
                self.symbol(",")?;
            }
            *value = self.number()?;
        }
        self.symbol(")")?;
        Ok(values)
    }
    fn at(&mut self, symbol: &str) -> Result<bool, UsdError> {
        let token = self.peek()?;
        Ok(token.kind == Kind::Symbol && token.text == symbol)
    }
    fn list<T>(
        &mut self,
        mut item: impl FnMut(&mut Self) -> Result<T, UsdError>,
    ) -> Result<Vec<T>, UsdError> {
        self.symbol("[")?;
        let mut values = Vec::new();
        while !self.at("]")? {
            values.push(item(self)?);
            if self.at("]")? {
                break;
            }
            self.symbol(",")?;
        }
        self.symbol("]")?;
        Ok(values)
    }
    fn integers(&mut self) -> Result<Vec<u32>, UsdError> {
        self.list(|p| {
            let token = p.kind(Kind::Number)?;
            if !token.text.bytes().all(|c| c.is_ascii_digit()) {
                return Err(invalid(token.line, "bad integer"));
            }
            token
                .text
                .parse()
                .map_err(|_| invalid(token.line, "bad integer"))
        })
    }
    fn attribute(&mut self, prim: &mut Prim<'a>) -> Result<(), UsdError> {
        let first = self.kind(Kind::Word)?;
        let uniform = first.text == "uniform";
        let ty = if uniform {
            self.kind(Kind::Word)?
        } else {
            first
        };
        let name = self.kind(Kind::Word)?;
        let n = name.text;
        let expected = match n {
            "xformOp:translate" | "xformOp:rotateXYZ" | "xformOp:scale" => {
                !uniform && matches!(ty.text, "float3" | "double3")
            }
            "xformOp:orient" => !uniform && ty.text == "quatf",
            "xformOpOrder" => uniform && ty.text == "token[]",
            "points" | "faceVertexCounts" | "faceVertexIndices" => {
                if prim.mesh.is_none() {
                    return Err(unsupported(name.line, format!("{n} on Xform")));
                }
                let expected = if n == "points" { "point3f[]" } else { "int[]" };
                !uniform && ty.text == expected
            }
            _ => return Err(unsupported(name.line, n)),
        };
        if !expected {
            let prefix = if uniform { "uniform " } else { "" };
            return Err(unsupported(name.line, format!("{prefix}{} {n}", ty.text)));
        }
        if !prim.seen.insert(n) {
            return Err(invalid(name.line, format!("duplicate {n}")));
        }
        self.symbol("=")?;
        match n {
            "xformOp:translate" | "xformOp:scale" => {
                let [x, y, z] = self.tuple()?;
                let value = Vec3::new(x, y, z);
                if n == OPS[0] {
                    prim.transform.translation = value;
                } else {
                    prim.transform.scale = value;
                }
            }
            "xformOp:rotateXYZ" => {
                prim.transform.rotation = rotate_xyz_degrees(self.tuple()?)
                    .ok_or(UsdError::NonFinite { line: name.line })?;
            }
            "xformOp:orient" => prim.orient = Some(self.tuple()?),
            "xformOpOrder" => prim.order = self.list(|p| Ok(p.kind(Kind::String)?.text))?,
            "points" => prim.points = Some(self.list(|p| p.tuple())?),
            "faceVertexCounts" => prim.counts = Some(self.integers()?),
            "faceVertexIndices" => prim.indices = Some(self.integers()?),
            _ => return Err(unsupported(name.line, n)),
        }
        Ok(())
    }
}

#[derive(Default)]
struct Prim<'a> {
    line: usize,
    node: usize,
    mesh: Option<u32>,
    seen: HashSet<&'a str>,
    children: HashSet<&'a str>,
    order: Vec<&'a str>,
    transform: Transform,
    orient: Option<[f32; 4]>,
    points: Option<Vec<[f32; 3]>>,
    counts: Option<Vec<u32>>,
    indices: Option<Vec<u32>>,
}

impl Prim<'_> {
    fn finish(mut self, scene: &mut Scene) -> Result<(), UsdError> {
        for op in &self.order {
            if !OPS.contains(op) {
                return Err(unsupported(self.line, *op));
            }
        }
        let mut ordered = HashSet::new();
        for op in &self.order {
            if !ordered.insert(*op) {
                return Err(invalid(self.line, "duplicate op in xformOpOrder"));
            }
        }
        if self.order.iter().any(|op| !self.seen.contains(op)) {
            return Err(invalid(self.line, "xformOpOrder names a missing op"));
        }
        if OPS
            .iter()
            .any(|op| self.seen.contains(op) && !ordered.contains(op))
        {
            return Err(invalid(self.line, "op not in xformOpOrder"));
        }
        if self.seen.contains(OPS[1]) && self.seen.contains(OPS[2]) {
            return Err(unsupported(self.line, "rotateXYZ and orient"));
        }
        let rank = |op| match op {
            "xformOp:translate" => 0,
            "xformOp:scale" => 2,
            _ => 1,
        };
        if self
            .order
            .windows(2)
            .any(|pair| rank(pair[0]) > rank(pair[1]))
        {
            return Err(unsupported(self.line, "xformOpOrder order"));
        }
        if let Some([w, x, y, z]) = self.orient {
            self.transform.rotation =
                Quat::new(x, y, z, w).ok_or_else(|| invalid(self.line, "zero orient"))?;
        }
        if let Some(index) = self.mesh {
            let points = self
                .points
                .ok_or_else(|| invalid(self.line, "missing points"))?;
            let counts = self
                .counts
                .ok_or_else(|| invalid(self.line, "missing faceVertexCounts"))?;
            let indices = self
                .indices
                .ok_or_else(|| invalid(self.line, "missing faceVertexIndices"))?;
            if counts.iter().any(|&count| count != 3) {
                return Err(unsupported(self.line, "faceVertexCounts"));
            }
            if counts.len().checked_mul(3) != Some(indices.len()) {
                return Err(invalid(self.line, "faceVertexIndices length"));
            }
            if indices
                .iter()
                .any(|&i| usize::try_from(i).ok().is_none_or(|i| i >= points.len()))
            {
                return Err(invalid(self.line, "faceVertexIndices range"));
            }
            let mesh = scene
                .meshes
                .get_mut(index as usize)
                .ok_or(UsdError::Syntax { line: self.line })?;
            *mesh = Mesh {
                positions: points,
                indices,
            };
        }
        scene
            .hierarchy
            .nodes
            .get_mut(self.node)
            .ok_or(UsdError::Syntax { line: self.line })?
            .transform = self.transform;
        Ok(())
    }
}

/// Imports only the documented subset, validating world matrices before return.
pub fn import_usda(text: &str) -> Result<Scene, UsdError> {
    let body = if text == "#usda 1.0" {
        ""
    } else {
        text.strip_prefix("#usda 1.0\n").ok_or(UsdError::Header)?
    };
    let mut parser = Parser {
        text: body,
        pos: 0,
        line: 2,
        look: None,
    };
    let mut scene = Scene::default();
    let mut stack: Vec<Prim<'_>> = Vec::new();
    let mut roots = HashSet::new();
    loop {
        let token = parser.peek()?;
        match (token.kind, token.text) {
            (Kind::End, _) if stack.is_empty() => break,
            (Kind::Word, "over" | "class") => return Err(unsupported(token.line, token.text)),
            (Kind::Symbol, "(") if stack.is_empty() => return Err(unsupported(token.line, "(")),
            (Kind::Word, "def") => {
                parser.take()?;
                if stack.len() >= MAX_DEPTH {
                    return Err(UsdError::TooDeep { line: token.line });
                }
                let ty = parser.kind(Kind::Word)?;
                if !matches!(ty.text, "Xform" | "Mesh") {
                    return Err(unsupported(ty.line, ty.text));
                }
                let name = parser.kind(Kind::String)?;
                let mut chars = name.text.bytes();
                if !chars
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
                    || !chars.all(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return Err(invalid(token.line, "bad prim name"));
                }
                let siblings = stack.last_mut().map_or(&mut roots, |p| &mut p.children);
                if !siblings.insert(name.text) {
                    return Err(invalid(token.line, "duplicate prim name"));
                }
                let open = parser.peek()?;
                if open.kind == Kind::Symbol && open.text == "(" {
                    return Err(unsupported(open.line, "("));
                }
                parser.symbol("{")?;
                let mesh = if ty.text == "Mesh" {
                    let index = u32::try_from(scene.meshes.len())
                        .map_err(|_| invalid(token.line, "bad integer"))?;
                    scene.meshes.push(Mesh::default());
                    Some(index)
                } else {
                    None
                };
                let node = scene.hierarchy.nodes.len();
                scene.hierarchy.nodes.push(Node {
                    parent: stack.last().map(|p| p.node),
                    mesh,
                    transform: Transform::default(),
                });
                stack.push(Prim {
                    line: token.line,
                    node,
                    mesh,
                    ..Prim::default()
                });
            }
            (Kind::Symbol, "}") => {
                parser.take()?;
                stack
                    .pop()
                    .ok_or(UsdError::Syntax { line: token.line })?
                    .finish(&mut scene)?;
            }
            (Kind::Word, _) if !stack.is_empty() => {
                let prim = stack
                    .last_mut()
                    .ok_or(UsdError::Syntax { line: token.line })?;
                parser.attribute(prim)?;
            }
            _ => return Err(UsdError::Syntax { line: token.line }),
        }
    }
    scene
        .hierarchy
        .world_matrices()
        .map_err(UsdError::Hierarchy)?;
    Ok(scene)
}

/// USD rotateXYZ: rotate about X first, then Y, then Z (angles in degrees).
/// None only for a non-finite angle. All finite f32 angles fit the f64 calculation.
pub fn rotate_xyz_degrees(degrees: [f32; 3]) -> Option<Quat> {
    if !degrees.iter().all(|d| d.is_finite()) {
        return None;
    }
    let [(sx, cx), (sy, cy), (sz, cz)] =
        degrees.map(|d| (f64::from(d).to_radians() / 2.0).sin_cos());
    let multiply = |[ax, ay, az, aw]: [f64; 4], [bx, by, bz, bw]: [f64; 4]| {
        [
            aw * bx + ax * bw + ay * bz - az * by,
            aw * by - ax * bz + ay * bw + az * bx,
            aw * bz + ax * by - ay * bx + az * bw,
            aw * bw - ax * bx - ay * by - az * bz,
        ]
    };
    let [x, y, z, w] = multiply(
        multiply([0.0, 0.0, sz, cz], [0.0, sy, 0.0, cy]),
        [sx, 0.0, 0.0, cx],
    );
    Quat::new(x as f32, y as f32, z as f32, w as f32)
}

fn same_bits(a: impl IntoIterator<Item = f32>, b: impl IntoIterator<Item = f32>) -> bool {
    a.into_iter()
        .map(f32::to_bits)
        .eq(b.into_iter().map(f32::to_bits))
}
fn xyz(v: Vec3) -> [f32; 3] {
    [v.x, v.y, v.z]
}
fn same_rows<const N: usize>(a: &[[f32; N]], b: &[[f32; N]]) -> bool {
    a.len() == b.len() && same_bits(a.iter().flatten().copied(), b.iter().flatten().copied())
}

/// Returns the first named difference; all floating-point values use bit equality.
pub fn compare_scenes(a: &Scene, b: &Scene) -> Result<(), UsdError> {
    let check = |equal, what: String| {
        if equal {
            Ok(())
        } else {
            Err(UsdError::Mismatch { what })
        }
    };
    check(
        a.hierarchy.nodes.len() == b.hierarchy.nodes.len(),
        "node count".into(),
    )?;
    for (i, (a, b)) in a.hierarchy.nodes.iter().zip(&b.hierarchy.nodes).enumerate() {
        check(a.parent == b.parent, format!("node {i} parent"))?;
        check(a.mesh == b.mesh, format!("node {i} mesh"))?;
        let (a, b) = (&a.transform, &b.transform);
        check(
            same_bits(xyz(a.translation), xyz(b.translation)),
            format!("node {i} translation"),
        )?;
        check(
            same_bits(a.rotation.components(), b.rotation.components()),
            format!("node {i} rotation"),
        )?;
        check(
            same_bits(xyz(a.scale), xyz(b.scale)),
            format!("node {i} scale"),
        )?;
    }
    check(a.meshes.len() == b.meshes.len(), "mesh count".into())?;
    for (i, (a, b)) in a.meshes.iter().zip(&b.meshes).enumerate() {
        check(
            same_rows(&a.positions, &b.positions),
            format!("mesh {i} positions"),
        )?;
        check(a.indices == b.indices, format!("mesh {i} indices"))?;
    }
    check(a.layers == b.layers, "layers".into())?;
    let (a, b) = (&a.instances, &b.instances);
    check(
        same_rows(a.matrices(), b.matrices())
            && same_bits(a.angles().iter().copied(), b.angles().iter().copied())
            && same_bits(a.speeds().iter().copied(), b.speeds().iter().copied())
            && a.layers() == b.layers()
            && a.nodes() == b.nodes(),
        "instances".into(),
    )
}
