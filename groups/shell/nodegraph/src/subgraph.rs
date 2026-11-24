//! Assets are graphs with exports, including an output; exported inputs are unconnected.
//! Instances use `asset/<relative path>` and an Int `instance` cache key.
//! Paths are nonempty slash-separated `.bsg` names: no absolute paths, drive
//! letters, backslashes, empty/dot/parent components, `=`, or forbidden format text.
//! An iterative dependency walk checks paths, recursion and depth before building.
//! Each instance owns an inner evaluator, leaving the outer evaluator unchanged (R6).
//! Reload affects direct instances only; reloading nested assets is not supported.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use crate::eval::Evaluator;
use crate::format::{self, FormatError};
use crate::graph::{Graph, NodeId, PortDir, Position};
use crate::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use crate::value::{Value, ValueType};

/// Framework composition setting: eight asset levels, not eight inner nodes.
pub const MAX_DEPTH: usize = 8;
pub const ASSET_PREFIX: &str = "asset/";
pub const INSTANCE_PARAM: &str = "instance";
pub type BaseKinds<'a> = &'a dyn Fn(&mut KindRegistry) -> Result<(), NodeError>;

#[derive(Clone, Debug, PartialEq)]
pub enum SubgraphError {
    BadPath { path: String },
    MissingAsset { path: String },
    Io { path: String, message: String },
    Recursion { chain: Vec<String> },
    TooDeep { chain: Vec<String> },
    Format { path: String, error: FormatError },
    NotAsset { path: String },
    ExportedInputConnected { path: String, export: String },
    PortsChanged { path: String },
    DuplicateInstanceKey { type_name: String, key: i64 },
    Kind(NodeError),
}

impl std::fmt::Display for SubgraphError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SubgraphError {}

pub trait AssetSource {
    fn read(&self, path: &str) -> Result<String, SubgraphError>;
}

pub struct DirSource {
    pub base: PathBuf,
}

impl AssetSource for DirSource {
    fn read(&self, path: &str) -> Result<String, SubgraphError> {
        check_asset_path(path)?;
        std::fs::read_to_string(self.base.join(path)).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                SubgraphError::MissingAsset { path: path.into() }
            } else {
                SubgraphError::Io {
                    path: path.into(),
                    message: error.to_string(),
                }
            }
        })
    }
}

pub fn check_asset_path(path: &str) -> Result<(), SubgraphError> {
    if path.is_empty()
        || !format::text_allowed(path)
        || path.contains('=')
        || path.starts_with('/')
        || !path.ends_with(".bsg")
        || path.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(SubgraphError::BadPath { path: path.into() });
    }
    Ok(())
}

fn asset_rule(graph: &Graph, path: &str) -> Result<(), SubgraphError> {
    if !graph.exports().values().any(|e| e.dir == PortDir::Output) {
        return Err(SubgraphError::NotAsset { path: path.into() });
    }
    for (name, export) in graph.exports() {
        if export.dir == PortDir::Input && graph.source_of(export.node, &export.port).is_some() {
            return Err(SubgraphError::ExportedInputConnected {
                path: path.into(),
                export: name.clone(),
            });
        }
    }
    Ok(())
}

fn unique_keys(graph: &Graph) -> Result<(), SubgraphError> {
    let mut seen = BTreeSet::new();
    for (_, node) in graph.nodes() {
        if node.type_name.starts_with(ASSET_PREFIX) {
            if let Some((_, Value::Int(key))) = node.params.first() {
                if !seen.insert((node.type_name.clone(), *key)) {
                    return Err(SubgraphError::DuplicateInstanceKey {
                        type_name: node.type_name.clone(),
                        key: *key,
                    });
                }
            }
        }
    }
    Ok(())
}

pub fn save_asset(graph: &Graph) -> Result<String, SubgraphError> {
    asset_rule(graph, "")?;
    for (_, node) in graph.nodes() {
        if let Some(path) = node.type_name.strip_prefix(ASSET_PREFIX) {
            check_asset_path(path)?;
        }
    }
    format::write(graph).map_err(|error| SubgraphError::Format {
        path: String::new(),
        error,
    })
}

// Only node-type fields are inspected here. Canonical syntax is the parser's job.
fn node_types(text: &str) -> impl Iterator<Item = &str> {
    text.split_terminator('\n')
        .skip_while(|line| *line != "nodes")
        .skip(1)
        .take_while(|line| *line != "edges")
        .filter_map(|line| line.split(',').nth(1))
}

fn references(text: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut seen = BTreeSet::new();
    for name in node_types(text) {
        if let Some(path) = name.strip_prefix(ASSET_PREFIX) {
            if seen.insert(path.to_owned()) {
                result.push(path.to_owned());
            }
        }
    }
    result
}

struct Frame {
    path: String,
    text: String,
    children: std::vec::IntoIter<String>,
}

// Postorder is collected only after every dependency has passed detection.
// Completed assets are still walked on other branches: depth is chain-dependent.
fn detect(
    roots: &[String],
    source: &dyn AssetSource,
) -> Result<Vec<(String, String)>, SubgraphError> {
    let mut order = Vec::new();
    let mut texts = BTreeMap::new();
    let mut emitted = BTreeSet::new();
    for root in roots {
        let mut pending = Some(root.clone());
        let mut stack: Vec<Frame> = Vec::new();
        loop {
            if let Some(path) = pending.take() {
                check_asset_path(&path)?;
                let mut chain: Vec<_> = stack.iter().map(|f| f.path.clone()).collect();
                chain.push(path.clone());
                if stack.iter().any(|f| f.path == path) {
                    return Err(SubgraphError::Recursion { chain });
                }
                if chain.len() > MAX_DEPTH {
                    return Err(SubgraphError::TooDeep { chain });
                }
                let text = match texts.get(&path) {
                    Some(text) => String::clone(text),
                    None => {
                        let text = source.read(&path)?;
                        texts.insert(path.clone(), text.clone());
                        text
                    }
                };
                let children = references(&text).into_iter();
                stack.push(Frame {
                    path,
                    text,
                    children,
                });
            }
            let Some(frame) = stack.last_mut() else {
                break;
            };
            if let Some(child) = frame.children.next() {
                pending = Some(child);
            } else {
                let frame = stack.pop().expect("frame just inspected");
                if emitted.insert(frame.path.clone()) {
                    order.push((frame.path, frame.text));
                }
            }
        }
    }
    Ok(order)
}

struct Template {
    graph: Graph,
    registry: Rc<KindRegistry>,
    original: Rc<KindRegistry>,
    children: BTreeMap<String, AssetHandle>,
}

struct State {
    graph: Graph,
    registry: Rc<KindRegistry>,
    feeders: Vec<NodeId>,
    evaluator: Evaluator,
}

/// Registered kind; each key owns its graph, feeders and lazy evaluator.
pub struct InstanceKind {
    path: String,
    type_name: String,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
    template: Rc<RefCell<Template>>,
    states: RefCell<BTreeMap<i64, State>>,
}

#[derive(Clone)]
pub struct AssetHandle {
    kind: Rc<InstanceKind>,
}

impl AssetHandle {
    pub fn type_name(&self) -> &str {
        &self.kind.type_name
    }

    pub fn path(&self) -> &str {
        &self.kind.path
    }

    // A nested instance's keys are local to its enclosing private graph.
    fn fork(&self) -> Self {
        Self {
            kind: Rc::new(InstanceKind {
                path: self.path().into(),
                type_name: self.type_name().into(),
                inputs: self.kind.inputs.clone(),
                outputs: self.kind.outputs.clone(),
                params: self.kind.params.clone(),
                template: self.kind.template.clone(),
                states: RefCell::new(BTreeMap::new()),
            }),
        }
    }
}

pub struct Loaded {
    pub graph: Graph,
    pub registry: KindRegistry,
    pub assets: Vec<AssetHandle>,
}

fn evaluation(error: impl std::fmt::Display) -> NodeError {
    NodeError::Evaluation(error.to_string())
}

fn finite(value: &Value) -> bool {
    match value {
        Value::F64(v) => v.is_finite(),
        Value::Image(image) => image.pixels.iter().all(|v| v.is_finite()),
        _ => true,
    }
}

struct Feeder {
    name: String,
    params: Vec<ParamDesc>,
    outputs: Vec<PortDesc>,
}

impl NodeKind for Feeder {
    fn type_name(&self) -> &str {
        &self.name
    }
    fn inputs(&self) -> &[PortDesc] {
        &[]
    }
    fn outputs(&self) -> &[PortDesc] {
        &self.outputs
    }
    fn params(&self) -> &[ParamDesc] {
        &self.params
    }
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        if !inputs.is_empty()
            || params.len() != 1
            || params[0].type_of() != self.params[0].value_type
            || !finite(&params[0])
        {
            return Err(evaluation("invalid feeder arguments"));
        }
        Ok(params.to_vec())
    }
}

fn feeder_name(value_type: &ValueType) -> Result<&'static str, NodeError> {
    match value_type {
        ValueType::Bool => Ok("subgraph/in/bool"),
        ValueType::Int => Ok("subgraph/in/int"),
        ValueType::F64 => Ok("subgraph/in/f64"),
        ValueType::Text => Ok("subgraph/in/text"),
        ValueType::Image => Ok("subgraph/in/image"),
        ValueType::Named(name) => Err(evaluation(format!("unsupported exported type: {name}"))),
    }
}

fn feeders(registry: &mut KindRegistry) -> Result<(), NodeError> {
    for value_type in [
        ValueType::Bool,
        ValueType::Int,
        ValueType::F64,
        ValueType::Text,
        ValueType::Image,
    ] {
        registry.add(Box::new(Feeder {
            name: feeder_name(&value_type)?.into(),
            params: vec![ParamDesc {
                name: "value".into(),
                value_type: value_type.clone(),
            }],
            outputs: vec![PortDesc {
                name: "out".into(),
                value_type,
            }],
        }))?;
    }
    Ok(())
}

// Immutable base implementations can be shared, but nested instance caches cannot.
struct SharedKind {
    registry: Rc<KindRegistry>,
    name: String,
}

impl SharedKind {
    fn kind(&self) -> &dyn NodeKind {
        self.registry
            .lookup(&self.name)
            .expect("registered shared kind")
    }
}

impl NodeKind for SharedKind {
    fn type_name(&self) -> &str {
        &self.name
    }
    fn inputs(&self) -> &[PortDesc] {
        self.kind().inputs()
    }
    fn outputs(&self) -> &[PortDesc] {
        self.kind().outputs()
    }
    fn params(&self) -> &[ParamDesc] {
        self.kind().params()
    }
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        self.kind().eval(params, inputs)
    }
}

impl State {
    fn new(template: &Template, inputs: &[Value]) -> Result<Self, NodeError> {
        let mut registry = KindRegistry::new();
        let names: BTreeSet<_> = template
            .graph
            .nodes()
            .map(|(_, n)| n.type_name.clone())
            .collect();
        for name in names {
            if let Some(child) = template.children.get(&name) {
                registry.add(Box::new(child.fork()))?;
            } else {
                template.registry.lookup(&name)?;
                registry.add(Box::new(SharedKind {
                    registry: template.registry.clone(),
                    name,
                }))?;
            }
        }
        // Register only feeder names not already present in a persisted graph.
        for value_type in [
            ValueType::Bool,
            ValueType::Int,
            ValueType::F64,
            ValueType::Text,
            ValueType::Image,
        ] {
            let name = feeder_name(&value_type)?;
            if registry.lookup(name).is_err() {
                registry.add(Box::new(SharedKind {
                    registry: template.registry.clone(),
                    name: name.into(),
                }))?;
            }
        }
        let mut graph = template.graph.clone();
        let exports: Vec<_> = graph
            .exports()
            .values()
            .filter(|e| e.dir == PortDir::Input)
            .cloned()
            .collect();
        let mut feeders = Vec::new();
        for (export, value) in exports.iter().zip(inputs) {
            let id = graph
                .add_node(
                    &registry,
                    feeder_name(&value.type_of())?,
                    vec![value.clone()],
                    Position { x: 0.0, y: 0.0 },
                )
                .map_err(evaluation)?;
            graph
                .connect(id, "out", export.node, &export.port)
                .map_err(evaluation)?;
            feeders.push(id);
        }
        Ok(Self {
            graph,
            registry: Rc::new(registry),
            feeders,
            evaluator: Evaluator::new(),
        })
    }
}

impl NodeKind for AssetHandle {
    fn type_name(&self) -> &str {
        self.kind.type_name()
    }
    fn inputs(&self) -> &[PortDesc] {
        self.kind.inputs()
    }
    fn outputs(&self) -> &[PortDesc] {
        self.kind.outputs()
    }
    fn params(&self) -> &[ParamDesc] {
        self.kind.params()
    }
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        self.kind.eval(params, inputs)
    }
}

impl NodeKind for InstanceKind {
    fn type_name(&self) -> &str {
        &self.type_name
    }
    fn inputs(&self) -> &[PortDesc] {
        &self.inputs
    }
    fn outputs(&self) -> &[PortDesc] {
        &self.outputs
    }
    fn params(&self) -> &[ParamDesc] {
        &self.params
    }
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        let [Value::Int(key)] = params else {
            return Err(evaluation("expected one Int instance key"));
        };
        let run = || -> Result<Vec<Value>, NodeError> {
            if inputs.len() != self.inputs().len()
                || inputs
                    .iter()
                    .zip(self.inputs())
                    .any(|(v, p)| v.type_of() != p.value_type || !finite(v))
            {
                return Err(evaluation("invalid instance inputs"));
            }
            let template = self.template.borrow();
            let mut states = self.states.borrow_mut();
            if let std::collections::btree_map::Entry::Vacant(entry) = states.entry(*key) {
                entry.insert(State::new(&template, inputs)?);
            }
            let state = states.get_mut(key).expect("state inserted");
            for (id, value) in state.feeders.iter().zip(inputs) {
                if state.graph.node(*id).expect("private feeder").params[0].1 != *value {
                    state
                        .graph
                        .set_param(*id, "value", value.clone())
                        .map_err(evaluation)?;
                }
            }
            template
                .graph
                .exports()
                .values()
                .filter(|e| e.dir == PortDir::Output)
                .map(|e| {
                    state
                        .evaluator
                        .evaluate(&state.graph, &state.registry, (e.node, &e.port))
                        .map_err(evaluation)
                })
                .collect()
        };
        run().map_err(|error| evaluation(format!("{} instance {key}: {error}", self.type_name())))
    }
}

fn interface(graph: &Graph, dir: PortDir) -> Result<Vec<PortDesc>, SubgraphError> {
    graph
        .exports()
        .iter()
        .filter(|(_, e)| e.dir == dir)
        .map(|(name, e)| {
            let node = graph.node(e.node).expect("validated export");
            let ports = if dir == PortDir::Input {
                &node.inputs
            } else {
                &node.outputs
            };
            let port = ports
                .iter()
                .find(|p| p.name == e.port)
                .expect("validated port");
            feeder_name(&port.value_type).map_err(SubgraphError::Kind)?;
            Ok(PortDesc {
                name: name.clone(),
                value_type: port.value_type.clone(),
            })
        })
        .collect()
}

fn build(
    order: Vec<(String, String)>,
    base: &dyn Fn(&mut KindRegistry) -> Result<(), NodeError>,
) -> Result<BTreeMap<String, AssetHandle>, SubgraphError> {
    let mut built: BTreeMap<String, AssetHandle> = BTreeMap::new();
    for (path, text) in order {
        let mut registry = KindRegistry::new();
        base(&mut registry).map_err(SubgraphError::Kind)?;
        feeders(&mut registry).map_err(SubgraphError::Kind)?;
        let mut children = BTreeMap::new();
        for child_path in references(&text) {
            let child = built.get(&child_path).expect("detected postorder").fork();
            registry
                .add(Box::new(child.clone()))
                .map_err(SubgraphError::Kind)?;
            children.insert(child.type_name().into(), child);
        }
        let graph = format::parse(&text, &registry).map_err(|error| match error {
            FormatError::NoExports => SubgraphError::NotAsset { path: path.clone() },
            error => SubgraphError::Format {
                path: path.clone(),
                error,
            },
        })?;
        asset_rule(&graph, &path)?;
        unique_keys(&graph)?;
        let inputs = interface(&graph, PortDir::Input)?;
        let outputs = interface(&graph, PortDir::Output)?;
        let registry = Rc::new(registry);
        let handle = AssetHandle {
            kind: Rc::new(InstanceKind {
                type_name: format!("{ASSET_PREFIX}{path}"),
                path: path.clone(),
                inputs,
                outputs,
                params: vec![ParamDesc {
                    name: INSTANCE_PARAM.into(),
                    value_type: ValueType::Int,
                }],
                template: Rc::new(RefCell::new(Template {
                    graph,
                    original: registry.clone(),
                    registry,
                    children,
                })),
                states: RefCell::new(BTreeMap::new()),
            }),
        };
        built.insert(path, handle);
    }
    Ok(built)
}

pub fn load_asset(
    path: &str,
    source: &dyn AssetSource,
    base: BaseKinds<'_>,
    registry: &mut KindRegistry,
) -> Result<AssetHandle, SubgraphError> {
    let mut built = build(detect(&[path.into()], source)?, base)?;
    let handle = built.remove(path).expect("detected root");
    registry
        .add(Box::new(handle.clone()))
        .map_err(SubgraphError::Kind)?;
    Ok(handle)
}

pub fn load_graph(
    text: &str,
    source: &dyn AssetSource,
    base: BaseKinds<'_>,
) -> Result<Loaded, SubgraphError> {
    let roots = references(text);
    detect(&roots, source)?;
    let mut registry = KindRegistry::new();
    base(&mut registry).map_err(SubgraphError::Kind)?;
    let mut assets = Vec::new();
    for path in roots {
        // Separate loads also isolate a direct asset from a copy nested in another.
        assets.push(load_asset(&path, source, base, &mut registry)?);
    }
    let graph = format::parse(text, &registry).map_err(|error| SubgraphError::Format {
        path: String::new(),
        error,
    })?;
    unique_keys(&graph)?;
    Ok(Loaded {
        graph,
        registry,
        assets,
    })
}

pub fn reload_asset(
    handle: &AssetHandle,
    source: &dyn AssetSource,
    graph: &Graph,
    evaluator: &mut Evaluator,
) -> Result<(), SubgraphError> {
    // Reuse the original base implementations; child kinds are rebuilt from source.
    let template = handle.kind.template.borrow();
    let order = detect(&[handle.path().into()], source)?;
    let mut names = BTreeSet::new();
    for (_, text) in &order {
        for name in node_types(text) {
            if !name.starts_with(ASSET_PREFIX) && !name.starts_with("subgraph/in/") {
                names.insert(name.to_owned());
            }
        }
    }
    let original = template.original.clone();
    let base = |registry: &mut KindRegistry| {
        for name in &names {
            // Unknown types are left to the parser, preserving its path/line error.
            if original.lookup(name).is_ok() {
                registry.add(Box::new(SharedKind {
                    registry: original.clone(),
                    name: name.clone(),
                }))?;
            }
        }
        Ok(())
    };
    let mut built = build(order, &base)?;
    let replacement = built.remove(handle.path()).expect("detected root");
    if handle.inputs() != replacement.inputs() || handle.outputs() != replacement.outputs() {
        return Err(SubgraphError::PortsChanged {
            path: handle.path().into(),
        });
    }
    let mut replacement = replacement.kind.template.borrow_mut();
    replacement.original = original;
    drop(template);
    std::mem::swap(&mut *handle.kind.template.borrow_mut(), &mut *replacement);
    handle.kind.states.borrow_mut().clear();
    for (id, node) in graph.nodes() {
        if node.type_name == handle.type_name() {
            evaluator.invalidate(id);
        }
    }
    Ok(())
}

/// Returns zero when there are no instances. Callers must leave room above the
/// largest key; the Int key space cannot represent a successor of `i64::MAX`.
pub fn next_instance_key(graph: &Graph, handle: &AssetHandle) -> i64 {
    graph
        .nodes()
        .filter(|(_, n)| n.type_name == handle.type_name())
        .filter_map(|(_, n)| match n.params.first() {
            Some((_, Value::Int(key))) => Some(*key),
            _ => None,
        })
        .max()
        .map_or(0, |key| key.saturating_add(1))
}
