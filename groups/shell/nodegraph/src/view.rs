//! Kind-independent draw lists and character grids.
//!
//! Stored positions become cells by flooring each coordinate divided by its
//! cell size in f64, then saturating to i64. If every position is exactly zero,
//! iterative Kahn ordering assigns longest-path layers, spaced by LAYER_STEP_X;
//! ids within each layer are spaced vertically by LAYER_STEP_Y. Otherwise all
//! stored positions are retained, including zero positions.

use std::collections::{BTreeMap, BTreeSet};

use crate::editor::{Editor, NODE_HEIGHT, NODE_WIDTH};
use crate::graph::{Graph, NodeId};

pub const BOX_COLS: i64 = 16;
pub const CELL_W: f64 = NODE_WIDTH as f64 / 16.0;
pub const CELL_H: f64 = NODE_HEIGHT as f64 / 5.0;
pub const TITLE_MAX: usize = 13;
pub const LABEL_MAX: usize = 6;
pub const LAYER_STEP_X: f32 = 150.0;
pub const LAYER_STEP_Y: f32 = 72.0;
pub const MAX_GRID_SIDE: i64 = 2_000;

#[derive(Clone, Debug, PartialEq)]
pub struct NodeBox {
    pub node: NodeId,
    pub col: i64,
    pub row: i64,
    pub rows: i64,
    pub title: String,
    pub dirty: bool,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
    pub start: (i64, i64),
    pub end: (i64, i64),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DrawList {
    pub boxes: Vec<NodeBox>,
    pub wires: Vec<Wire>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ViewError {
    TooLarge { width: u64, height: u64 },
}

fn cell(position: f64, size: f64) -> i64 {
    (position / size).floor() as i64
}

fn clean(text: &str, limit: usize) -> String {
    text.chars()
        .take(limit)
        .map(|c| {
            if c.is_ascii_graphic() || c == ' ' {
                c
            } else {
                '?'
            }
        })
        .collect()
}

fn layers(graph: &Graph) -> BTreeMap<NodeId, usize> {
    let mut incoming: BTreeMap<_, usize> = graph.nodes().map(|(id, _)| (id, 0)).collect();
    let mut outgoing: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for edge in graph.edges() {
        if let Some(count) = incoming.get_mut(&edge.to) {
            *count += 1;
        }
        outgoing.entry(edge.from).or_default().push(edge.to);
    }
    let mut ready: BTreeSet<_> = incoming
        .iter()
        .filter_map(|(&id, &count)| (count == 0).then_some(id))
        .collect();
    let mut result = BTreeMap::new();
    while let Some(id) = ready.pop_first() {
        let layer = *result.entry(id).or_insert(0);
        if let Some(targets) = outgoing.get(&id) {
            for to in targets {
                let next = result.entry(*to).or_insert(0);
                *next = (*next).max(layer + 1);
                if let Some(count) = incoming.get_mut(to) {
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(*to);
                    }
                }
            }
        }
    }
    result
}

impl DrawList {
    pub fn from_editor(editor: &Editor<'_>, dirty: &BTreeSet<NodeId>) -> DrawList {
        Self::from_graph(editor.graph(), dirty)
    }

    pub fn from_graph(graph: &Graph, dirty: &BTreeSet<NodeId>) -> DrawList {
        let automatic = graph
            .nodes()
            .all(|(_, n)| n.position.x == 0.0 && n.position.y == 0.0);
        let layers = if automatic {
            layers(graph)
        } else {
            BTreeMap::new()
        };
        let mut indices: BTreeMap<usize, usize> = BTreeMap::new();
        let mut boxes = Vec::new();
        let mut lookup = BTreeMap::new();
        for (id, node) in graph.nodes() {
            let (x, y) = if automatic {
                let layer = layers.get(&id).copied().unwrap_or(0);
                let index = indices.entry(layer).or_default();
                let position = (
                    layer as f64 * f64::from(LAYER_STEP_X),
                    *index as f64 * f64::from(LAYER_STEP_Y),
                );
                *index += 1;
                position
            } else {
                (f64::from(node.position.x), f64::from(node.position.y))
            };
            lookup.insert(id, boxes.len());
            boxes.push(NodeBox {
                node: id,
                col: cell(x, CELL_W),
                row: cell(y, CELL_H),
                rows: 3 + node.inputs.len().max(node.outputs.len()).max(1) as i64,
                title: clean(
                    &format!(
                        "{}:{}",
                        id.0,
                        node.type_name.rsplit('.').next().unwrap_or("")
                    ),
                    TITLE_MAX,
                ),
                dirty: dirty.contains(&id),
                inputs: node
                    .inputs
                    .iter()
                    .map(|p| clean(&p.name, LABEL_MAX))
                    .collect(),
                outputs: node
                    .outputs
                    .iter()
                    .map(|p| clean(&p.name, LABEL_MAX))
                    .collect(),
            });
        }
        let mut wires = Vec::new();
        for edge in graph.edges() {
            let (Some(&from), Some(&to)) = (lookup.get(&edge.from), lookup.get(&edge.to)) else {
                continue;
            };
            let (Some(source), Some(target)) = (graph.node(edge.from), graph.node(edge.to)) else {
                continue;
            };
            let (Some(i), Some(j)) = (
                source.outputs.iter().position(|p| p.name == edge.from_port),
                target.inputs.iter().position(|p| p.name == edge.to_port),
            ) else {
                continue;
            };
            wires.push(Wire {
                from: edge.from,
                from_port: edge.from_port.clone(),
                to: edge.to,
                to_port: edge.to_port.clone(),
                start: (
                    boxes[from].col.saturating_add(BOX_COLS),
                    boxes[from].row.saturating_add(2).saturating_add(i as i64),
                ),
                end: (
                    boxes[to].col.saturating_sub(1),
                    boxes[to].row.saturating_add(2).saturating_add(j as i64),
                ),
            });
        }
        Self { boxes, wires }
    }

    pub fn render(&self) -> Result<String, ViewError> {
        // Compute extents without traversing any potentially enormous range.
        // Widen before adding offsets, including at saturated stored cells.
        let mut bounds: Option<(i128, i128, i128, i128)> = None;
        let mut include = |x: i128, y: i128| {
            bounds = Some(match bounds {
                None => (x, y, x, y),
                Some((l, t, r, b)) => (l.min(x), t.min(y), r.max(x), b.max(y)),
            });
        };
        for wire in &self.wires {
            let (sx, sy) = (i128::from(wire.start.0), i128::from(wire.start.1));
            let (ex, ey) = (i128::from(wire.end.0), i128::from(wire.end.1));
            include(sx, sy);
            include(ex, ey);
            if sy != ey {
                include(ex - 1, sy);
                include(ex - 1, ey);
            }
        }
        for node in &self.boxes {
            let (x, y) = (i128::from(node.col), i128::from(node.row));
            include(x, y);
            include(
                x + i128::from(BOX_COLS) - 1,
                y + i128::from(node.rows.max(2)) - 1,
            );
            include(x, y + 1);
            let ports = node.inputs.len().max(node.outputs.len());
            if ports > 0 {
                include(x, y + 1 + ports as i128);
            }
        }
        let Some((left, top, right, bottom)) = bounds else {
            return Ok(String::new());
        };
        let (width, height) = (right - left + 1, bottom - top + 1);
        if width > i128::from(MAX_GRID_SIDE) || height > i128::from(MAX_GRID_SIDE) {
            return Err(ViewError::TooLarge {
                width: width.min(i128::from(u64::MAX)) as u64,
                height: height.min(i128::from(u64::MAX)) as u64,
            });
        }
        let mut grid = vec![vec![b' '; width as usize]; height as usize];
        let mut put = |x: i128, y: i128, c: u8| {
            grid[(y - top) as usize][(x - left) as usize] = c;
        };
        for wire in &self.wires {
            let (sx, sy) = (i128::from(wire.start.0), i128::from(wire.start.1));
            let (ex, ey) = (i128::from(wire.end.0), i128::from(wire.end.1));
            if sy == ey {
                for x in sx.min(ex)..=sx.max(ex) {
                    put(x, sy, b'-');
                }
            } else {
                let v = ex - 1;
                for x in sx.min(v)..=sx.max(v) {
                    put(x, sy, b'-');
                }
                for y in sy.min(ey)..=sy.max(ey) {
                    put(v, y, b'|');
                }
                for x in v..=ex {
                    put(x, ey, b'-');
                }
                put(v, sy, b'+');
                put(v, ey, b'+');
            }
        }
        for node in &self.boxes {
            let (x, y) = (i128::from(node.col), i128::from(node.row));
            let rows = i128::from(node.rows.max(2));
            for dy in 0..rows {
                for dx in 0..i128::from(BOX_COLS) {
                    let c = match (dy == 0 || dy == rows - 1, dx == 0 || dx == 15) {
                        (true, true) => b'+',
                        (true, false) => b'-',
                        (false, true) => b'|',
                        (false, false) => b' ',
                    };
                    put(x + dx, y + dy, c);
                }
            }
            put(x + 1, y + 1, if node.dirty { b'*' } else { b' ' });
            for (i, c) in clean(&node.title, TITLE_MAX).bytes().enumerate() {
                put(x + 2 + i as i128, y + 1, c);
            }
            for (j, label) in node.inputs.iter().enumerate() {
                let row = y + 2 + j as i128;
                put(x, row, b'>');
                for (i, c) in clean(label, LABEL_MAX).bytes().enumerate() {
                    put(x + 1 + i as i128, row, c);
                }
            }
            for (j, label) in node.outputs.iter().enumerate() {
                let row = y + 2 + j as i128;
                let label = clean(label, LABEL_MAX);
                put(x + 15, row, b'>');
                for (i, c) in label.bytes().enumerate() {
                    put(x + 15 - label.len() as i128 + i as i128, row, c);
                }
            }
        }
        let mut text = String::new();
        for line in grid {
            let end = line.iter().rposition(|&c| c != b' ').map_or(0, |i| i + 1);
            text.extend(line[..end].iter().map(|&c| char::from(c)));
            text.push('\n');
        }
        Ok(text)
    }
}
