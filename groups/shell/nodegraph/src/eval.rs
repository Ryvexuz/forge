//! Lazy evaluation using graph revisions and cached-result stamps.
//!
//! A node is clean only when force-full is off, its cached revision matches,
//! and every input still names the recorded source and port, whose node is
//! clean and whose cached stamp matches the recorded stamp. All other nodes
//! are dirty. The same predicate serves demands and whole-graph dirty queries.
//! There is no early cutoff: even an unchanged result receives a fresh stamp.
//! One evaluator serves one graph; call `clear` before using another graph,
//! including a clone edited independently. Both walks are iterative.
//! Cache pruning happens once per graph version; the one-graph rule and
//! clearing between graphs also keep pruning correct.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::graph::{Graph, NodeId};
use crate::kind::{KindRegistry, NodeError};
use crate::value::Value;

/// Cumulative count of node-function calls made by this evaluator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EvalStats {
    pub calls: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EvalError {
    UnknownNode(NodeId),
    UnknownOutput {
        node: NodeId,
        port: String,
    },
    UnconnectedInput {
        node: NodeId,
        port: String,
    },
    /// The node's type name is not registered.
    Kind {
        node: NodeId,
        error: NodeError,
    },
    /// The node function returned an error.
    Node {
        node: NodeId,
        error: NodeError,
    },
    /// A caught node-function panic.
    Panic {
        node: NodeId,
        message: String,
    },
    /// Output count or types differ from the node's output descriptors.
    BadOutputs {
        node: NodeId,
    },
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EvalError {}

struct Entry {
    revision: u64,
    outputs: Vec<Value>,
    stamp: u64,
    inputs: Vec<(NodeId, String, u64)>,
}

/// Lazy, cached evaluation of one graph. Call `clear` before changing graphs.
#[derive(Default)]
pub struct Evaluator {
    cache: BTreeMap<NodeId, Entry>,
    pruned_at: Option<u64>,
    next_stamp: u64,
    stats: EvalStats,
    force_full: bool,
}

impl Evaluator {
    pub fn new() -> Evaluator {
        Self::default()
    }

    // `clean_sources` contains exactly the already-visited clean/successful nodes.
    // Both callers visit sources first, so no recursive cache checks are needed.
    fn clean(&self, graph: &Graph, id: NodeId, clean_sources: &BTreeSet<NodeId>) -> bool {
        if self.force_full {
            return false;
        }
        let (Some(node), Some(entry)) = (graph.node(id), self.cache.get(&id)) else {
            return false;
        };
        if graph.revision(id) != Some(entry.revision) || node.inputs.len() != entry.inputs.len() {
            return false;
        }
        node.inputs
            .iter()
            .zip(&entry.inputs)
            .all(|(port, (source, source_port, stamp))| {
                graph.source_of(id, &port.name).is_some_and(|edge| {
                    edge.from == *source
                        && edge.from_port == *source_port
                        && clean_sources.contains(source)
                        && self.cache.get(source).is_some_and(|e| e.stamp == *stamp)
                })
            })
    }

    /// Runs each dirty upstream node once, in dependency order, and no other node.
    pub fn evaluate(
        &mut self,
        graph: &Graph,
        registry: &KindRegistry,
        output: (NodeId, &str),
    ) -> Result<Value, EvalError> {
        if self.pruned_at != Some(graph.version()) {
            self.cache.retain(|id, _| graph.node(*id).is_some());
            self.pruned_at = Some(graph.version());
        }
        let node = graph
            .node(output.0)
            .ok_or(EvalError::UnknownNode(output.0))?;
        if !node.outputs.iter().any(|p| p.name == output.1) {
            return Err(EvalError::UnknownOutput {
                node: output.0,
                port: output.1.into(),
            });
        }

        let mut stack = vec![(output.0, false)];
        let mut visited = BTreeSet::new();
        let mut order = Vec::new();
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                order.push(id);
            } else if visited.insert(id) {
                stack.push((id, true));
                let node = graph.node(id).ok_or(EvalError::UnknownNode(id))?;
                for port in node.inputs.iter().rev() {
                    if let Some(edge) = graph.source_of(id, &port.name) {
                        stack.push((edge.from, false));
                    }
                }
            }
        }

        let mut clean_sources = BTreeSet::new();
        for id in order {
            if !self.clean(graph, id, &clean_sources) {
                self.run(graph, registry, id)?;
            }
            clean_sources.insert(id);
        }
        self.cached_value(graph, output.0, output.1).cloned()
    }

    fn cached_value(&self, graph: &Graph, id: NodeId, port: &str) -> Result<&Value, EvalError> {
        let node = graph.node(id).ok_or(EvalError::UnknownNode(id))?;
        node.outputs
            .iter()
            .position(|p| p.name == port)
            .and_then(|index| self.cache.get(&id)?.outputs.get(index))
            .ok_or_else(|| EvalError::UnknownOutput {
                node: id,
                port: port.into(),
            })
    }

    fn run(&mut self, graph: &Graph, registry: &KindRegistry, id: NodeId) -> Result<(), EvalError> {
        // Remove the old result before any fallible operation, including lookup.
        // Thus every failure leaves this node and its readers dirty.
        self.cache.remove(&id);
        let node = graph.node(id).ok_or(EvalError::UnknownNode(id))?;
        let kind = registry
            .lookup(&node.type_name)
            .map_err(|error| EvalError::Kind { node: id, error })?;
        let mut inputs = Vec::with_capacity(node.inputs.len());
        let mut sources = Vec::with_capacity(node.inputs.len());
        for port in &node.inputs {
            let edge = graph
                .source_of(id, &port.name)
                .ok_or_else(|| EvalError::UnconnectedInput {
                    node: id,
                    port: port.name.clone(),
                })?;
            inputs.push(self.cached_value(graph, edge.from, &edge.from_port)?.clone());
            // The explicit post-order walk has already cached every source.
            let stamp = self.cache[&edge.from].stamp;
            sources.push((edge.from, edge.from_port.clone(), stamp));
        }
        let params: Vec<_> = node.params.iter().map(|(_, value)| value.clone()).collect();
        self.stats.calls += 1;
        self.next_stamp += 1;
        let outputs = catch_unwind(AssertUnwindSafe(|| kind.eval(&params, &inputs)))
            .map_err(|payload| {
                let message = if let Some(text) = payload.downcast_ref::<&str>() {
                    (*text).to_owned()
                } else if let Some(text) = payload.downcast_ref::<String>() {
                    text.clone()
                } else {
                    "non-string panic payload".into()
                };
                EvalError::Panic { node: id, message }
            })?
            .map_err(|error| EvalError::Node { node: id, error })?;
        if outputs.len() != node.outputs.len()
            || outputs
                .iter()
                .zip(&node.outputs)
                .any(|(value, port)| value.type_of() != port.value_type)
        {
            return Err(EvalError::BadOutputs { node: id });
        }
        self.cache.insert(
            id,
            Entry {
                revision: graph.revision(id).ok_or(EvalError::UnknownNode(id))?,
                outputs,
                stamp: self.next_stamp,
                inputs: sources,
            },
        );
        Ok(())
    }

    /// All nodes a future demand would run, without calling any node functions.
    pub fn dirty_nodes(&self, graph: &Graph) -> BTreeSet<NodeId> {
        let mut remaining: BTreeMap<_, usize> = graph.nodes().map(|(id, _)| (id, 0)).collect();
        let mut readers: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for edge in graph.edges() {
            *remaining.get_mut(&edge.to).expect("graph owns edge target") += 1;
            readers.entry(edge.from).or_default().push(edge.to);
        }
        let mut ready: VecDeque<_> = remaining
            .iter()
            .filter_map(|(&id, &count)| (count == 0).then_some(id))
            .collect();
        let mut clean_sources = BTreeSet::new();
        while let Some(id) = ready.pop_front() {
            if self.clean(graph, id, &clean_sources) {
                clean_sources.insert(id);
            }
            if let Some(nodes) = readers.get(&id) {
                for reader in nodes {
                    let count = remaining.get_mut(reader).expect("graph owns reader");
                    *count -= 1;
                    if *count == 0 {
                        ready.push_back(*reader);
                    }
                }
            }
        }
        graph
            .nodes()
            .map(|(id, _)| id)
            .filter(|id| !clean_sources.contains(id))
            .collect()
    }

    /// Returns false for nodes not in this graph.
    pub fn is_dirty(&self, graph: &Graph, node: NodeId) -> bool {
        self.dirty_nodes(graph).contains(&node)
    }

    /// Drops this result; the clean predicate also dirties all its readers.
    pub fn invalidate(&mut self, node: NodeId) {
        self.cache.remove(&node);
    }

    /// Drops all results, retaining statistics and the force-full setting.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.pruned_at = None;
        self.next_stamp = 0;
    }

    /// Makes every demanded node run once per demand, regardless of its cache.
    pub fn set_force_full(&mut self, on: bool) {
        self.force_full = on;
    }

    pub fn stats(&self) -> EvalStats {
        self.stats
    }

    pub fn reset_stats(&mut self) {
        self.stats = EvalStats::default();
    }

    pub fn cached_len(&self) -> usize {
        self.cache.len()
    }
}
