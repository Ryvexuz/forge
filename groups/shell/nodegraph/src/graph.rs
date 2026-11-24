//! Owned nodes, port snapshots, parameters, positions, edges and named exports.
//! Edits refuse missing identities, invalid types or non-finite values, duplicate
//! names, occupied inputs, cycles, and removal of read or exported nodes.
//! Equality compares nodes, edges and exports, not allocation or edit counters.
//! Each successful edit advances version; evaluation-input edits stamp the affected
//! node's revision. A cached node is stale when its cached revision differs.

use std::collections::{BTreeMap, BTreeSet};

use crate::kind::{KindRegistry, NodeError, PortDesc};
use crate::value::{Value, ValueType};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(pub u64);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PortDir {
    Input,
    Output,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub type_name: String,
    pub params: Vec<(String, Value)>,
    pub position: Position,
    pub inputs: Vec<PortDesc>,
    pub outputs: Vec<PortDesc>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Edge {
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Export {
    pub node: NodeId,
    pub port: String,
    pub dir: PortDir,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GraphError {
    Kind(NodeError),
    DuplicateName {
        type_name: String,
        name: String,
    },
    ParamCount {
        expected: usize,
        got: usize,
    },
    ParamType {
        name: String,
        expected: ValueType,
        got: ValueType,
    },
    NonFinite(String),
    UnknownNode(NodeId),
    DuplicateNodeId(NodeId),
    IdsExhausted,
    UnknownParam {
        node: NodeId,
        name: String,
    },
    UnknownPort {
        node: NodeId,
        port: String,
        dir: PortDir,
    },
    TypeMismatch {
        from: ValueType,
        to: ValueType,
    },
    Cycle {
        from: NodeId,
        to: NodeId,
    },
    InputTaken {
        node: NodeId,
        port: String,
        source: NodeId,
    },
    NotConnected {
        node: NodeId,
        port: String,
    },
    NodeInUse {
        node: NodeId,
        reader: NodeId,
    },
    NodeExported {
        node: NodeId,
        name: String,
    },
    DuplicateExport(String),
    UnknownExport(String),
    BadExportName(String),
}

impl std::fmt::Display for GraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for GraphError {}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    nodes: BTreeMap<NodeId, Node>,
    edges: BTreeSet<Edge>,
    exports: BTreeMap<String, Export>,
    next_id: u64,
    version: u64,
    revisions: BTreeMap<NodeId, u64>,
    incoming: BTreeMap<(NodeId, String), Edge>,
}

impl PartialEq for Graph {
    fn eq(&self, other: &Self) -> bool {
        self.nodes == other.nodes && self.edges == other.edges && self.exports == other.exports
    }
}

impl Graph {
    pub fn new() -> Graph {
        Self::default()
    }

    fn check_node(&self, id: NodeId) -> Result<&Node, GraphError> {
        self.nodes.get(&id).ok_or(GraphError::UnknownNode(id))
    }

    fn check_position(position: Position) -> Result<(), GraphError> {
        if !position.x.is_finite() || !position.y.is_finite() {
            return Err(GraphError::NonFinite("position".into()));
        }
        Ok(())
    }

    fn check_value(name: &str, expected: &ValueType, value: &Value) -> Result<(), GraphError> {
        let got = value.type_of();
        if &got != expected {
            return Err(GraphError::ParamType {
                name: name.into(),
                expected: expected.clone(),
                got,
            });
        }
        let finite = match value {
            Value::F64(v) => v.is_finite(),
            Value::Image(image) => image.pixels.iter().all(|v| v.is_finite()),
            _ => true,
        };
        if !finite {
            return Err(GraphError::NonFinite(format!("param {name}")));
        }
        Ok(())
    }

    fn check_add(
        &self,
        registry: &KindRegistry,
        fixed: Option<NodeId>,
        type_name: &str,
        params: &[Value],
        position: Position,
    ) -> Result<(NodeId, Node), GraphError> {
        let kind = registry.lookup(type_name).map_err(GraphError::Kind)?;
        for names in [
            kind.inputs()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            kind.outputs().iter().map(|p| p.name.as_str()).collect(),
            kind.params().iter().map(|p| p.name.as_str()).collect(),
        ] {
            let mut seen = BTreeSet::new();
            for name in names {
                if !seen.insert(name) {
                    return Err(GraphError::DuplicateName {
                        type_name: type_name.into(),
                        name: name.into(),
                    });
                }
            }
        }
        if params.len() != kind.params().len() {
            return Err(GraphError::ParamCount {
                expected: kind.params().len(),
                got: params.len(),
            });
        }
        for (desc, value) in kind.params().iter().zip(params) {
            Self::check_value(&desc.name, &desc.value_type, value)?;
        }
        Self::check_position(position)?;
        let id = fixed.unwrap_or(NodeId(self.next_id));
        if fixed.is_some() && self.nodes.contains_key(&id) {
            return Err(GraphError::DuplicateNodeId(id));
        }
        if id.0 == u64::MAX {
            return Err(GraphError::IdsExhausted);
        }
        Ok((
            id,
            Node {
                type_name: type_name.into(),
                params: kind
                    .params()
                    .iter()
                    .zip(params)
                    .map(|(desc, value)| (desc.name.clone(), value.clone()))
                    .collect(),
                position,
                inputs: kind.inputs().to_vec(),
                outputs: kind.outputs().to_vec(),
            },
        ))
    }

    fn insert_node(&mut self, id: NodeId, node: Node) {
        self.nodes.insert(id, node);
        self.next_id = self.next_id.max(id.0 + 1);
        self.version += 1;
        self.revisions.insert(id, self.version);
    }

    pub fn add_node(
        &mut self,
        registry: &KindRegistry,
        type_name: &str,
        params: Vec<Value>,
        position: Position,
    ) -> Result<NodeId, GraphError> {
        let (id, node) = self.check_add(registry, None, type_name, &params, position)?;
        self.insert_node(id, node);
        Ok(id)
    }

    pub fn add_node_with_id(
        &mut self,
        registry: &KindRegistry,
        id: NodeId,
        type_name: &str,
        params: Vec<Value>,
        position: Position,
    ) -> Result<(), GraphError> {
        let (id, node) = self.check_add(registry, Some(id), type_name, &params, position)?;
        self.insert_node(id, node);
        Ok(())
    }

    fn check_port(&self, node: NodeId, port: &str, dir: PortDir) -> Result<&PortDesc, GraphError> {
        let n = self.check_node(node)?;
        let ports = match dir {
            PortDir::Input => &n.inputs,
            PortDir::Output => &n.outputs,
        };
        ports
            .iter()
            .find(|p| p.name == port)
            .ok_or_else(|| GraphError::UnknownPort {
                node,
                port: port.into(),
                dir,
            })
    }

    fn check_connect(
        &self,
        from: NodeId,
        from_port: &str,
        to: NodeId,
        to_port: &str,
    ) -> Result<(), GraphError> {
        self.check_node(from)?;
        self.check_node(to)?;
        let output = self.check_port(from, from_port, PortDir::Output)?;
        let input = self.check_port(to, to_port, PortDir::Input)?;
        if output.value_type != input.value_type {
            return Err(GraphError::TypeMismatch {
                from: output.value_type.clone(),
                to: input.value_type.clone(),
            });
        }
        if let Some(edge) = self.source_of(to, to_port) {
            return Err(GraphError::InputTaken {
                node: to,
                port: to_port.into(),
                source: edge.from,
            });
        }
        // A new leaf cannot already reach another node. This also keeps building
        // long chains cheap, without weakening the general reachability check.
        if from == to || (self.has_outgoing(to) && self.descendants(to)?.contains(&from)) {
            return Err(GraphError::Cycle { from, to });
        }
        Ok(())
    }

    fn outgoing(&self, node: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges
            .range(
                Edge {
                    from: node,
                    from_port: String::new(),
                    to: NodeId(0),
                    to_port: String::new(),
                }..,
            )
            .take_while(move |e| e.from == node)
    }

    fn has_outgoing(&self, node: NodeId) -> bool {
        self.outgoing(node).next().is_some()
    }

    pub fn connect(
        &mut self,
        from: NodeId,
        from_port: &str,
        to: NodeId,
        to_port: &str,
    ) -> Result<(), GraphError> {
        self.check_connect(from, from_port, to, to_port)?;
        let edge = Edge {
            from,
            from_port: from_port.into(),
            to,
            to_port: to_port.into(),
        };
        self.incoming.insert((to, to_port.into()), edge.clone());
        self.edges.insert(edge);
        self.version += 1;
        self.revisions.insert(to, self.version);
        Ok(())
    }

    fn check_remove(&self, node: NodeId) -> Result<(), GraphError> {
        self.check_node(node)?;
        if let Some(reader) = self.outgoing(node).map(|e| e.to).min() {
            return Err(GraphError::NodeInUse { node, reader });
        }
        if let Some((name, _)) = self.exports.iter().find(|(_, e)| e.node == node) {
            return Err(GraphError::NodeExported {
                node,
                name: name.clone(),
            });
        }
        Ok(())
    }

    pub fn remove_node(&mut self, node: NodeId) -> Result<(Node, Vec<Edge>), GraphError> {
        self.check_remove(node)?;
        let incoming: Vec<_> = self
            .edges
            .iter()
            .filter(|e| e.to == node)
            .cloned()
            .collect();
        for edge in &incoming {
            self.edges.remove(edge);
            self.incoming.remove(&(edge.to, edge.to_port.clone()));
        }
        let removed = self.nodes.remove(&node).expect("checked node");
        self.revisions.remove(&node);
        self.version += 1;
        Ok((removed, incoming))
    }

    fn check_disconnect(&self, to: NodeId, port: &str) -> Result<Edge, GraphError> {
        self.check_port(to, port, PortDir::Input)?;
        self.source_of(to, port)
            .cloned()
            .ok_or_else(|| GraphError::NotConnected {
                node: to,
                port: port.into(),
            })
    }

    pub fn disconnect(&mut self, to: NodeId, to_port: &str) -> Result<Edge, GraphError> {
        let edge = self.check_disconnect(to, to_port)?;
        self.edges.remove(&edge);
        self.incoming.remove(&(to, to_port.into()));
        self.version += 1;
        self.revisions.insert(to, self.version);
        Ok(edge)
    }

    fn check_set_param(&self, node: NodeId, name: &str, value: &Value) -> Result<usize, GraphError> {
        let n = self.check_node(node)?;
        let index = n
            .params
            .iter()
            .position(|(key, _)| key == name)
            .ok_or_else(|| GraphError::UnknownParam {
                node,
                name: name.into(),
            })?;
        Self::check_value(name, &n.params[index].1.type_of(), value)?;
        Ok(index)
    }

    pub fn set_param(&mut self, node: NodeId, name: &str, value: Value) -> Result<Value, GraphError> {
        let index = self.check_set_param(node, name, &value)?;
        let old = std::mem::replace(
            &mut self.nodes.get_mut(&node).expect("checked node").params[index].1,
            value,
        );
        self.version += 1;
        self.revisions.insert(node, self.version);
        Ok(old)
    }

    fn check_set_position(&self, node: NodeId, position: Position) -> Result<(), GraphError> {
        self.check_node(node)?;
        Self::check_position(position)
    }

    pub fn set_position(&mut self, node: NodeId, position: Position) -> Result<Position, GraphError> {
        self.check_set_position(node, position)?;
        let old = std::mem::replace(
            &mut self.nodes.get_mut(&node).expect("checked node").position,
            position,
        );
        self.version += 1;
        Ok(old)
    }

    fn check_export(
        &self,
        name: &str,
        node: NodeId,
        port: &str,
        dir: PortDir,
    ) -> Result<(), GraphError> {
        if name.is_empty() {
            return Err(GraphError::BadExportName(name.into()));
        }
        if self.exports.contains_key(name) {
            return Err(GraphError::DuplicateExport(name.into()));
        }
        self.check_port(node, port, dir)?;
        Ok(())
    }

    pub fn export(
        &mut self,
        name: &str,
        node: NodeId,
        port: &str,
        dir: PortDir,
    ) -> Result<(), GraphError> {
        self.check_export(name, node, port, dir)?;
        self.exports.insert(
            name.into(),
            Export {
                node,
                port: port.into(),
                dir,
            },
        );
        self.version += 1;
        Ok(())
    }

    fn check_unexport(&self, name: &str) -> Result<(), GraphError> {
        if !self.exports.contains_key(name) {
            return Err(GraphError::UnknownExport(name.into()));
        }
        Ok(())
    }

    pub fn unexport(&mut self, name: &str) -> Result<Export, GraphError> {
        self.check_unexport(name)?;
        let export = self.exports.remove(name).expect("checked export");
        self.version += 1;
        Ok(export)
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> + '_ {
        self.nodes.iter().map(|(&id, node)| (id, node))
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edges(&self) -> impl Iterator<Item = &Edge> + '_ {
        self.edges.iter()
    }

    pub fn source_of(&self, node: NodeId, input_port: &str) -> Option<&Edge> {
        self.incoming.get(&(node, input_port.into()))
    }

    pub fn exports(&self) -> &BTreeMap<String, Export> {
        &self.exports
    }

    pub fn descendants(&self, node: NodeId) -> Result<BTreeSet<NodeId>, GraphError> {
        self.check_node(node)?;
        let mut found = BTreeSet::new();
        let mut stack = vec![node];
        while let Some(current) = stack.pop() {
            for edge in self.outgoing(current) {
                if edge.to != node && found.insert(edge.to) {
                    stack.push(edge.to);
                }
            }
        }
        Ok(found)
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn revision(&self, node: NodeId) -> Option<u64> {
        self.revisions.get(&node).copied()
    }
}
