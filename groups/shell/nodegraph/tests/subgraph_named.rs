use std::cell::RefCell;
use std::collections::BTreeMap;

use nodegraph::format;
use nodegraph::graph::{Graph, PortDir, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::kinds::register_all;
use nodegraph::subgraph::{load_asset, save_asset, AssetSource, SubgraphError};
use nodegraph::value::{Value, ValueType};

#[derive(Default)]
struct MapSource(RefCell<BTreeMap<String, String>>);

impl MapSource {
    fn put(&self, path: &str, text: &str) {
        self.0.borrow_mut().insert(path.into(), text.into());
    }
}

impl AssetSource for MapSource {
    fn read(&self, path: &str) -> Result<String, SubgraphError> {
        self.0
            .borrow()
            .get(path)
            .cloned()
            .ok_or_else(|| SubgraphError::MissingAsset { path: path.into() })
    }
}

struct NamedSource {
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
}

impl NodeKind for NamedSource {
    fn type_name(&self) -> &str {
        "t.named_source"
    }
    fn inputs(&self) -> &[PortDesc] {
        &self.inputs
    }
    fn outputs(&self) -> &[PortDesc] {
        &self.outputs
    }
    fn params(&self) -> &[ParamDesc] {
        &[]
    }
    fn eval(&self, _params: &[Value], _inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        Err(NodeError::Evaluation("not evaluated".into()))
    }
}

struct NamedSink {
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
}

impl NodeKind for NamedSink {
    fn type_name(&self) -> &str {
        "t.named_sink"
    }
    fn inputs(&self) -> &[PortDesc] {
        &self.inputs
    }
    fn outputs(&self) -> &[PortDesc] {
        &self.outputs
    }
    fn params(&self) -> &[ParamDesc] {
        &[]
    }
    fn eval(&self, _params: &[Value], _inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        Err(NodeError::Evaluation("not evaluated".into()))
    }
}

fn port(name: &str, value_type: ValueType) -> PortDesc {
    PortDesc {
        name: name.into(),
        value_type,
    }
}

fn base(registry: &mut KindRegistry) -> Result<(), NodeError> {
    register_all(registry)?;
    registry.add(Box::new(NamedSource {
        inputs: vec![port("a", ValueType::F64)],
        outputs: vec![port("out", ValueType::Named("t.custom".into()))],
    }))?;
    registry.add(Box::new(NamedSink {
        inputs: vec![port("a", ValueType::Named("t.custom".into()))],
        outputs: vec![port("out", ValueType::F64)],
    }))
}

fn graph(type_name: &str) -> Graph {
    let mut registry = KindRegistry::new();
    base(&mut registry).unwrap();
    let mut graph = Graph::new();
    let node = graph
        .add_node(&registry, type_name, vec![], Position { x: 0.0, y: 0.0 })
        .unwrap();
    graph.export("x", node, "a", PortDir::Input).unwrap();
    graph.export("y", node, "out", PortDir::Output).unwrap();
    graph
}

#[test]
fn subgraph_named_output_export_saves_and_loads() {
    let graph = graph("t.named_source");
    let text = save_asset(&graph).unwrap();
    let source = MapSource::default();
    source.put("named_out.bsg", &text);
    let mut outer = KindRegistry::new();
    let handle = load_asset("named_out.bsg", &source, &base, &mut outer).unwrap();
    let kind = outer.lookup(handle.type_name()).unwrap();
    assert_eq!(
        kind.outputs(),
        &[port("y", ValueType::Named("t.custom".into()))]
    );
    assert_eq!(kind.inputs(), &[port("x", ValueType::F64)]);
}

#[test]
fn subgraph_named_input_export_refused_by_save_and_load() {
    let input_graph = graph("t.named_sink");
    let want = SubgraphError::Kind(NodeError::Evaluation(
        "unsupported exported type: t.custom".into(),
    ));
    assert_eq!(save_asset(&input_graph), Err(want.clone()));
    let text = format::write(&input_graph).unwrap();
    let source = MapSource::default();
    source.put("named_in.bsg", &text);
    assert_eq!(
        load_asset("named_in.bsg", &source, &base, &mut KindRegistry::new()).err(),
        Some(want)
    );
    assert!(save_asset(&graph("t.named_source")).is_ok());
}
