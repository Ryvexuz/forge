//! Commands are the only writer of the owned graph; callers receive read access.
//! Removal checks identity, that the editor's registry knows the node's type (so
//! undo can rebuild it), and exports before disconnecting outgoing edges, then
//! removes the node and records incoming edges. Undo restores the node, incoming
//! edges, then outgoing edges. Removal and its undo commit only on full success.
//! Grouping copies selected nodes and internal edges, exports every open or
//! externally fed input and every externally read output, and changes no history.
//! Copies retain ids, types, parameters and positions; the original is unchanged.
//! Boundary exports are named `in_<id>_<port>` and `out_<id>_<port>`.
//! Saving a group as an asset belongs to N7.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use crate::graph::{Edge, Graph, GraphError, Node, NodeId, PortDir, Position};
use crate::history::{Command, History};
use crate::kind::KindRegistry;
use crate::value::Value;

pub const NODE_WIDTH: f32 = 120.0;
pub const NODE_HEIGHT: f32 = 60.0;

// All finite stored coordinates fit in the wider range. Combine the largest
// magnitudes first so cancellation precedes addition of a tiny residual.
fn drag_coordinate(start: f32, pointer: f32, anchor: f32) -> f32 {
    let mut terms = [f64::from(start), f64::from(pointer), -f64::from(anchor)];
    terms.sort_by(|a, b| b.abs().total_cmp(&a.abs()));
    (terms[0] + terms[1] + terms[2]) as f32
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    Graph(GraphError),
    EmptySelection,
}

impl From<GraphError> for EditError {
    fn from(error: GraphError) -> Self {
        Self::Graph(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Key {
    Delete,
    Undo,
    Redo,
    ClearSelection,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EditorInput {
    Pointer { x: f32, y: f32 },
    ButtonDown,
    ButtonUp,
    Key(Key),
}

struct AddNode<'r> {
    registry: &'r KindRegistry,
    type_name: String,
    params: Vec<Value>,
    position: Position,
    id: Rc<Cell<Option<NodeId>>>,
}

impl Command<Graph, EditError> for AddNode<'_> {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        if let Some(id) = self.id.get() {
            graph.add_node_with_id(
                self.registry,
                id,
                &self.type_name,
                self.params.clone(),
                self.position,
            )?;
        } else {
            let id = graph.add_node(
                self.registry,
                &self.type_name,
                self.params.clone(),
                self.position,
            )?;
            self.id.set(Some(id));
        }
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let id = self.id.get().ok_or(GraphError::IdsExhausted)?;
        graph.remove_node(id)?;
        Ok(())
    }
}

struct RemoveNode<'r> {
    registry: &'r KindRegistry,
    id: NodeId,
    saved: Option<Node>,
    incoming: Vec<Edge>,
    outgoing: Vec<Edge>,
}

impl Command<Graph, EditError> for RemoveNode<'_> {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let node = graph.node(self.id).ok_or(GraphError::UnknownNode(self.id))?;
        self.registry.lookup(&node.type_name).map_err(GraphError::Kind)?;
        if let Some((name, _)) = graph.exports().iter().find(|(_, e)| e.node == self.id) {
            return Err(GraphError::NodeExported {
                node: self.id,
                name: name.clone(),
            }
            .into());
        }
        let outgoing: Vec<_> = graph
            .edges()
            .filter(|e| e.from == self.id)
            .cloned()
            .collect();
        let mut next = graph.clone();
        for edge in &outgoing {
            next.disconnect(edge.to, &edge.to_port)?;
        }
        let (node, incoming) = next.remove_node(self.id)?;
        self.saved = Some(node);
        self.incoming = incoming;
        self.outgoing = outgoing;
        *graph = next;
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let node = self.saved.as_ref().ok_or(GraphError::UnknownNode(self.id))?;
        let mut next = graph.clone();
        next.add_node_with_id(
            self.registry,
            self.id,
            &node.type_name,
            node.params.iter().map(|(_, v)| v.clone()).collect(),
            node.position,
        )?;
        for edge in self.incoming.iter().chain(&self.outgoing) {
            next.connect(edge.from, &edge.from_port, edge.to, &edge.to_port)?;
        }
        *graph = next;
        Ok(())
    }
}

struct Connect(Edge);

impl Command<Graph, EditError> for Connect {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        graph.connect(self.0.from, &self.0.from_port, self.0.to, &self.0.to_port)?;
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        graph.disconnect(self.0.to, &self.0.to_port)?;
        Ok(())
    }
}

struct Disconnect {
    to: NodeId,
    port: String,
    edge: Option<Edge>,
}

impl Command<Graph, EditError> for Disconnect {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        self.edge = Some(graph.disconnect(self.to, &self.port)?);
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let edge = self.edge.as_ref().ok_or_else(|| GraphError::NotConnected {
            node: self.to,
            port: self.port.clone(),
        })?;
        graph.connect(edge.from, &edge.from_port, edge.to, &edge.to_port)?;
        Ok(())
    }
}

struct SetParam {
    node: NodeId,
    name: String,
    value: Value,
    old: Option<Value>,
}

impl Command<Graph, EditError> for SetParam {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        self.old = Some(graph.set_param(self.node, &self.name, self.value.clone())?);
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let old = self.old.as_ref().ok_or_else(|| GraphError::UnknownParam {
            node: self.node,
            name: self.name.clone(),
        })?;
        graph.set_param(self.node, &self.name, old.clone())?;
        Ok(())
    }
}

struct MoveNode {
    node: NodeId,
    position: Position,
    old: Option<Position>,
}

impl Command<Graph, EditError> for MoveNode {
    fn apply(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        self.old = Some(graph.set_position(self.node, self.position)?);
        Ok(())
    }

    fn undo(&mut self, graph: &mut Graph) -> Result<(), EditError> {
        let old = self.old.ok_or(GraphError::UnknownNode(self.node))?;
        graph.set_position(self.node, old)?;
        Ok(())
    }
}

pub struct Editor<'r> {
    graph: Graph,
    registry: &'r KindRegistry,
    history: History<'r, Graph, EditError>,
    selection: BTreeSet<NodeId>,
    pointer: Position,
    drag: Option<(NodeId, Position, Position)>,
}

impl<'r> Editor<'r> {
    pub fn new(registry: &'r KindRegistry) -> Self {
        Self::with_graph(registry, Graph::new())
    }

    pub fn with_graph(registry: &'r KindRegistry, graph: Graph) -> Self {
        Self {
            graph,
            registry,
            history: History::new(),
            selection: BTreeSet::new(),
            pointer: Position { x: 0.0, y: 0.0 },
            drag: None,
        }
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn registry(&self) -> &'r KindRegistry {
        self.registry
    }

    fn prune(&mut self) {
        self.selection.retain(|id| self.graph.node(*id).is_some());
        if self
            .drag
            .is_some_and(|(id, _, _)| self.graph.node(id).is_none())
        {
            self.drag = None;
        }
    }

    fn run(&mut self, command: Box<dyn Command<Graph, EditError> + 'r>) -> Result<(), EditError> {
        let result = self.history.run(&mut self.graph, command);
        self.prune();
        result
    }

    pub fn add_node(
        &mut self,
        type_name: &str,
        params: Vec<Value>,
        position: Position,
    ) -> Result<NodeId, EditError> {
        let id = Rc::new(Cell::new(None));
        self.run(Box::new(AddNode {
            registry: self.registry,
            type_name: type_name.into(),
            params,
            position,
            id: id.clone(),
        }))?;
        // A successful first apply always fills the result cell.
        id.get().ok_or(EditError::Graph(GraphError::IdsExhausted))
    }

    pub fn remove_node(&mut self, node: NodeId) -> Result<(), EditError> {
        self.run(Box::new(RemoveNode {
            registry: self.registry,
            id: node,
            saved: None,
            incoming: Vec::new(),
            outgoing: Vec::new(),
        }))
    }

    pub fn connect(
        &mut self,
        from: NodeId,
        from_port: &str,
        to: NodeId,
        to_port: &str,
    ) -> Result<(), EditError> {
        self.run(Box::new(Connect(Edge {
            from,
            from_port: from_port.into(),
            to,
            to_port: to_port.into(),
        })))
    }

    pub fn disconnect(&mut self, to: NodeId, to_port: &str) -> Result<(), EditError> {
        self.run(Box::new(Disconnect {
            to,
            port: to_port.into(),
            edge: None,
        }))
    }

    pub fn set_param(&mut self, node: NodeId, name: &str, value: Value) -> Result<(), EditError> {
        self.run(Box::new(SetParam {
            node,
            name: name.into(),
            value,
            old: None,
        }))
    }

    pub fn move_node(&mut self, node: NodeId, position: Position) -> Result<(), EditError> {
        self.run(Box::new(MoveNode {
            node,
            position,
            old: None,
        }))
    }

    pub fn group_selection(&self) -> Result<Graph, EditError> {
        if self.selection.is_empty() {
            return Err(EditError::EmptySelection);
        }
        let mut group = Graph::new();
        for &id in &self.selection {
            let node = self.graph.node(id).ok_or(GraphError::UnknownNode(id))?;
            group.add_node_with_id(
                self.registry,
                id,
                &node.type_name,
                node.params.iter().map(|(_, v)| v.clone()).collect(),
                node.position,
            )?;
            for port in &node.inputs {
                if !self
                    .graph
                    .source_of(id, &port.name)
                    .is_some_and(|e| self.selection.contains(&e.from))
                {
                    group.export(
                        &format!("in_{}_{}", id.0, port.name),
                        id,
                        &port.name,
                        PortDir::Input,
                    )?;
                }
            }
            for port in &node.outputs {
                if self.graph.edges().any(|e| {
                    e.from == id && e.from_port == port.name && !self.selection.contains(&e.to)
                }) {
                    group.export(
                        &format!("out_{}_{}", id.0, port.name),
                        id,
                        &port.name,
                        PortDir::Output,
                    )?;
                }
            }
        }
        for edge in self
            .graph
            .edges()
            .filter(|e| self.selection.contains(&e.from) && self.selection.contains(&e.to))
        {
            group.connect(edge.from, &edge.from_port, edge.to, &edge.to_port)?;
        }
        Ok(group)
    }

    pub fn select(&mut self, node: NodeId, additive: bool) -> Result<(), EditError> {
        self.graph.node(node).ok_or(GraphError::UnknownNode(node))?;
        if !additive {
            self.selection.clear();
        }
        self.selection.insert(node);
        Ok(())
    }

    pub fn clear_selection(&mut self) {
        self.selection.clear();
    }

    pub fn selection(&self) -> &BTreeSet<NodeId> {
        &self.selection
    }

    pub fn undo(&mut self) -> Result<bool, EditError> {
        let result = self.history.undo(&mut self.graph);
        self.prune();
        result
    }

    pub fn redo(&mut self) -> Result<bool, EditError> {
        let result = self.history.redo(&mut self.graph);
        self.prune();
        result
    }

    pub fn undo_len(&self) -> usize {
        self.history.undo_len()
    }

    pub fn redo_len(&self) -> usize {
        self.history.redo_len()
    }

    pub fn handle(&mut self, input: EditorInput) -> Result<(), EditError> {
        match input {
            EditorInput::Pointer { x, y } => {
                if !x.is_finite() || !y.is_finite() {
                    return Err(GraphError::NonFinite("pointer".into()).into());
                }
                self.pointer = Position { x, y };
            }
            EditorInput::ButtonDown => {
                // Widen before subtracting: every finite coordinate is supported,
                // even when subtraction would overflow at the stored precision.
                let hit = self
                    .graph
                    .nodes()
                    .filter(|(_, node)| {
                        let dx = f64::from(self.pointer.x) - f64::from(node.position.x);
                        let dy = f64::from(self.pointer.y) - f64::from(node.position.y);
                        (0.0..f64::from(NODE_WIDTH)).contains(&dx)
                            && (0.0..f64::from(NODE_HEIGHT)).contains(&dy)
                    })
                    .last()
                    .map(|(id, node)| (id, node.position));
                self.drag = None;
                self.clear_selection();
                if let Some((id, start)) = hit {
                    self.select(id, false)?;
                    self.drag = Some((id, start, self.pointer));
                }
            }
            EditorInput::ButtonUp => {
                if let Some((id, start, anchor)) = self.drag.take() {
                    if self.pointer != anchor {
                        let position = Position {
                            x: drag_coordinate(start.x, self.pointer.x, anchor.x),
                            y: drag_coordinate(start.y, self.pointer.y, anchor.y),
                        };
                        self.move_node(id, position)?;
                    }
                }
            }
            EditorInput::Key(key) => match key {
                Key::Delete => {
                    let selected: Vec<_> = self.selection.iter().copied().collect();
                    for id in selected {
                        self.remove_node(id)?;
                    }
                }
                Key::Undo => {
                    self.undo()?;
                }
                Key::Redo => {
                    self.redo()?;
                }
                Key::ClearSelection => self.clear_selection(),
            },
        }
        Ok(())
    }
}
