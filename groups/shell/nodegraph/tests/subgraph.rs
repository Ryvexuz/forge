use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use nodegraph::eval::Evaluator;
use nodegraph::format::{self, FormatError};
use nodegraph::graph::{Graph, NodeId, PortDir, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::kinds::register_all;
use nodegraph::subgraph::*;
use nodegraph::value::{Value, ValueType};

const GOLDEN: &str = include_str!("fixtures/asset_blur_v1.bsg");
const ZERO: Position = Position { x: 0.0, y: 0.0 };

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

struct ReadProbe<'a> {
    source: &'a MapSource,
    reads: Cell<usize>,
}

impl AssetSource for ReadProbe<'_> {
    fn read(&self, path: &str) -> Result<String, SubgraphError> {
        self.reads.set(self.reads.get() + 1);
        self.source.read(path)
    }
}

struct AddConstant {
    count: Rc<Cell<u64>>,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

impl NodeKind for AddConstant {
    fn type_name(&self) -> &str {
        "t.add_constant"
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
        if let ([Value::F64(k)], [Value::F64(a)]) = (params, inputs) {
            let value = a + k;
            if a.is_finite() && k.is_finite() && value.is_finite() {
                self.count.set(self.count.get() + 1);
                return Ok(vec![Value::F64(value)]);
            }
        }
        Err(NodeError::Evaluation(
            "invalid counting-kind arguments".into(),
        ))
    }
}

fn base(count: Rc<Cell<u64>>) -> impl Fn(&mut KindRegistry) -> Result<(), NodeError> {
    move |registry| {
        register_all(registry)?;
        let port = |name: &str| PortDesc {
            name: name.into(),
            value_type: ValueType::F64,
        };
        registry.add(Box::new(AddConstant {
            count: count.clone(),
            inputs: vec![port("a")],
            outputs: vec![port("out")],
            params: vec![ParamDesc {
                name: "k".into(),
                value_type: ValueType::F64,
            }],
        }))
    }
}

fn kinds() -> KindRegistry {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).unwrap();
    registry
}

fn directory() -> DirSource {
    DirSource {
        base: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
    }
}

fn add(graph: &mut Graph, registry: &KindRegistry, name: &str, params: Vec<Value>) -> NodeId {
    graph.add_node(registry, name, params, ZERO).unwrap()
}

fn constant(graph: &mut Graph, registry: &KindRegistry, value: f64) -> NodeId {
    add(graph, registry, "math.constant", vec![Value::F64(value)])
}

fn evaluate(
    graph: &Graph,
    registry: &KindRegistry,
    evaluator: &mut Evaluator,
    id: NodeId,
    port: &str,
) -> Value {
    evaluator.evaluate(graph, registry, (id, port)).unwrap()
}

fn image_graph(registry: &KindRegistry) -> Graph {
    let mut graph = Graph::new();
    let invert = add(&mut graph, registry, "image.invert", vec![]);
    let blur = graph
        .add_node(registry, "image.blur", vec![], Position { x: 0.0, y: 80.0 })
        .unwrap();
    let blend = graph
        .add_node(
            registry,
            "image.blend",
            vec![Value::F64(0.5)],
            Position { x: 160.0, y: 40.0 },
        )
        .unwrap();
    graph.connect(invert, "out", blend, "a").unwrap();
    graph.connect(blur, "out", blend, "b").unwrap();
    graph.export("src_a", invert, "a", PortDir::Input).unwrap();
    graph.export("src_b", blur, "a", PortDir::Input).unwrap();
    graph.export("out", blend, "out", PortDir::Output).unwrap();
    graph
}

fn chain_text(count: usize, k: f64, base: BaseKinds<'_>) -> String {
    assert!(count > 0);
    let mut registry = KindRegistry::new();
    base(&mut registry).unwrap();
    let mut graph = Graph::new();
    let first = add(&mut graph, &registry, "t.add_constant", vec![Value::F64(k)]);
    let mut last = first;
    for _ in 1..count {
        let next = add(&mut graph, &registry, "t.add_constant", vec![Value::F64(k)]);
        graph.connect(last, "out", next, "a").unwrap();
        last = next;
    }
    graph.export("x", first, "a", PortDir::Input).unwrap();
    graph.export("y", last, "out", PortDir::Output).unwrap();
    save_asset(&graph).unwrap()
}

fn instance(
    graph: &mut Graph,
    registry: &KindRegistry,
    handle: &AssetHandle,
    source: NodeId,
) -> NodeId {
    let key = next_instance_key(graph, handle);
    let id = add(graph, registry, handle.type_name(), vec![Value::Int(key)]);
    graph.connect(source, "out", id, "x").unwrap();
    id
}

fn outer_text(path: &str) -> String {
    format!("schema=1\nkind=graph\nnodes\n0,asset/{path},0,0,instance=0\nedges\n")
}

fn wrapper(path: &str) -> String {
    format!("schema=1\nkind=subgraph\nnodes\n0,asset/{path},0,0,instance=0\nedges\nexports\nx,0.x,in\ny,0.y,out\n")
}

#[test]
fn subgraph_asset_round_trip_byte_identical() {
    let mut registry = kinds();
    let saved = save_asset(&image_graph(&registry)).unwrap();
    if saved != GOLDEN {
        let line = saved
            .lines()
            .zip(GOLDEN.lines())
            .position(|(a, b)| a != b)
            .map_or_else(
                || saved.lines().count().min(GOLDEN.lines().count()) + 1,
                |n| n + 1,
            );
        panic!("golden mismatch at line {line}");
    }
    assert_eq!(
        save_asset(&format::parse(GOLDEN, &registry).unwrap()).unwrap(),
        GOLDEN
    );
    let handle = load_asset(
        "asset_blur_v1.bsg",
        &directory(),
        &register_all,
        &mut registry,
    )
    .unwrap();
    assert_eq!(handle.type_name(), "asset/asset_blur_v1.bsg");
    assert_eq!(handle.path(), "asset_blur_v1.bsg");
    assert_eq!(
        registry.lookup(handle.type_name()).unwrap().inputs()[0].name,
        "src_a"
    );
    assert_eq!(
        load_asset(
            "asset_blur_v1.bsg",
            &directory(),
            &register_all,
            &mut registry
        )
        .err(),
        Some(SubgraphError::Kind(NodeError::DuplicateType(
            handle.type_name().into()
        )))
    );
}

#[test]
fn subgraph_asset_rejects_unknown_version() {
    let source = MapSource::default();
    source.put("a.bsg", &GOLDEN.replacen("schema=1", "schema=2", 1));
    assert_eq!(
        load_asset("a.bsg", &source, &register_all, &mut kinds()).err(),
        Some(SubgraphError::Format {
            path: "a.bsg".into(),
            error: FormatError::UnknownVersion { found: "2".into() },
        })
    );
    assert_eq!(
        save_asset(&Graph::new()),
        Err(SubgraphError::NotAsset { path: "".into() })
    );
    let registry = kinds();
    let mut graph = image_graph(&registry);
    graph.unexport("out").unwrap();
    assert_eq!(
        save_asset(&graph),
        Err(SubgraphError::NotAsset { path: "".into() })
    );
    graph
        .export("out", NodeId(2), "out", PortDir::Output)
        .unwrap();
    let solid = add(
        &mut graph,
        &registry,
        "image.solid",
        vec![Value::Int(8), Value::Int(8), Value::F64(0.25)],
    );
    graph.connect(solid, "out", NodeId(0), "a").unwrap();
    let error = SubgraphError::ExportedInputConnected {
        path: "".into(),
        export: "src_a".into(),
    };
    assert_eq!(save_asset(&graph), Err(error));
    source.put("a.bsg", &format::write(&graph).unwrap());
    assert!(matches!(
        load_asset("a.bsg", &source, &register_all, &mut KindRegistry::new()),
        Err(SubgraphError::ExportedInputConnected { path, export })
            if path == "a.bsg" && export == "src_a"
    ));
    for text in [
        "schema=1\nkind=graph\nnodes\nedges\n",
        "schema=1\nkind=subgraph\nnodes\nedges\nexports\n",
    ] {
        source.put("a.bsg", text);
        assert!(matches!(
            load_asset("a.bsg", &source, &register_all, &mut KindRegistry::new()),
            Err(SubgraphError::NotAsset { path }) if path == "a.bsg"
        ));
    }
}

#[test]
fn subgraph_instance_equals_inlined() {
    let source = MapSource::default();
    let mut registry = kinds();
    let mut math = Graph::new();
    let plus = add(&mut math, &registry, "math.add", vec![]);
    let two = constant(&mut math, &registry, 2.0);
    let negate = add(&mut math, &registry, "math.negate", vec![]);
    math.connect(two, "out", plus, "b").unwrap();
    math.connect(plus, "out", negate, "a").unwrap();
    math.export("x", plus, "a", PortDir::Input).unwrap();
    math.export("y", negate, "out", PortDir::Output).unwrap();
    source.put("math.bsg", &save_asset(&math).unwrap());
    let handle = load_asset("math.bsg", &source, &register_all, &mut registry).unwrap();
    let mut outer = Graph::new();
    let five = constant(&mut outer, &registry, 5.0);
    let inst = instance(&mut outer, &registry, &handle, five);
    let mut evaluator = Evaluator::new();
    let result = evaluate(&outer, &registry, &mut evaluator, inst, "y");
    assert_eq!(result, Value::F64(-7.0));
    let five = constant(&mut math, &registry, 5.0);
    math.connect(five, "out", plus, "a").unwrap();
    assert_eq!(
        result,
        evaluate(&math, &registry, &mut Evaluator::new(), negate, "out")
    );

    let handle = load_asset(
        "asset_blur_v1.bsg",
        &directory(),
        &register_all,
        &mut registry,
    )
    .unwrap();
    let mut inlined = image_graph(&registry);
    let mut outer = Graph::new();
    let inst = add(
        &mut outer,
        &registry,
        handle.type_name(),
        vec![Value::Int(0)],
    );
    for (kind, params, export) in [
        (
            "image.solid",
            vec![Value::Int(8), Value::Int(8), Value::F64(0.25)],
            "src_a",
        ),
        (
            "image.gradient",
            vec![Value::Int(8), Value::Int(8)],
            "src_b",
        ),
    ] {
        let a = add(&mut outer, &registry, kind, params.clone());
        outer.connect(a, "out", inst, export).unwrap();
        let b = add(&mut inlined, &registry, kind, params);
        let target = inlined.exports()[export].clone();
        inlined
            .connect(b, "out", target.node, &target.port)
            .unwrap();
    }
    let actual = evaluate(&outer, &registry, &mut Evaluator::new(), inst, "out");
    let expected = evaluate(&inlined, &registry, &mut Evaluator::new(), NodeId(2), "out");
    assert_eq!(actual, expected);
    let Value::Image(image) = actual else {
        panic!("expected image")
    };
    assert_eq!((image.width, image.height, image.pixels.len()), (8, 8, 64));
}

#[test]
fn subgraph_instances_have_independent_caches() {
    let count = Rc::new(Cell::new(0));
    let base = base(count.clone());
    let source = MapSource::default();
    source.put("a.bsg", &chain_text(3, 1.0, &base));
    let mut r = kinds();
    let handle = load_asset("a.bsg", &source, &base, &mut r).unwrap();
    let mut g = Graph::new();
    let c1 = constant(&mut g, &r, 2.0);
    let c2 = constant(&mut g, &r, 5.0);
    let i1 = instance(&mut g, &r, &handle, c1);
    let i2 = instance(&mut g, &r, &handle, c2);
    assert_eq!(g.node(i1).unwrap().params[0].1, Value::Int(0));
    assert_eq!(g.node(i2).unwrap().params[0].1, Value::Int(1));
    let mut e = Evaluator::new();
    assert_eq!(evaluate(&g, &r, &mut e, i1, "y"), Value::F64(5.0));
    let unchanged = evaluate(&g, &r, &mut e, i2, "y");
    assert_eq!(unchanged, Value::F64(8.0));
    assert_eq!(count.replace(0), 6);
    // Force an outer call with identical input: neither cache aliasing nor
    // unconditional feeder assignment may run an inner node here.
    e.invalidate(i1);
    assert_eq!(evaluate(&g, &r, &mut e, i1, "y"), Value::F64(5.0));
    assert_eq!(count.replace(0), 0);
    e.reset_stats();
    g.set_param(c1, "value", Value::F64(10.0)).unwrap();
    assert_eq!(evaluate(&g, &r, &mut e, i1, "y"), Value::F64(13.0));
    assert_eq!(evaluate(&g, &r, &mut e, i2, "y"), unchanged);
    assert_eq!(count.replace(0), 3);
    assert_eq!(e.stats().calls, 2);
    let mut registry2 = kinds();
    let handle2 = load_asset("a.bsg", &source, &base, &mut registry2).unwrap();
    let mut graph2 = Graph::new();
    let c = constant(&mut graph2, &registry2, 20.0);
    let i = instance(&mut graph2, &registry2, &handle2, c);
    assert_eq!(
        evaluate(&graph2, &registry2, &mut Evaluator::new(), i, "y"),
        Value::F64(23.0)
    );
    assert_eq!(count.replace(0), 3);
    e.reset_stats();
    evaluate(&g, &r, &mut e, i1, "y");
    evaluate(&g, &r, &mut e, i2, "y");
    assert_eq!(count.get(), 0);
    assert_eq!(e.stats().calls, 0);

    // Direct NodeKind entry validates all inputs before changing private state.
    let kind = r.lookup(handle.type_name()).unwrap();
    for input in [
        Value::F64(f64::NAN),
        Value::F64(f64::INFINITY),
        Value::Int(1),
    ] {
        assert!(matches!(
            kind.eval(&[Value::Int(0)], &[input]),
            Err(NodeError::Evaluation(_))
        ));
    }
    assert!(kind.eval(&[], &[]).is_err());
    assert!(kind.eval(&[Value::Int(0)], &[]).is_err());
    assert_eq!(
        kind.eval(&[Value::Int(0)], &[Value::F64(10.0)]).unwrap(),
        vec![Value::F64(13.0)]
    );
    assert_eq!(count.get(), 0);
}

#[test]
fn subgraph_asset_change_dirties_all_instances() {
    let count = Rc::new(Cell::new(0));
    let base = base(count.clone());
    let source = MapSource::default();
    source.put("a.bsg", &chain_text(3, 1.0, &base));
    let mut r = kinds();
    let handle = load_asset("a.bsg", &source, &base, &mut r).unwrap();
    let mut g = Graph::new();
    let c1 = constant(&mut g, &r, 2.0);
    let c2 = constant(&mut g, &r, 5.0);
    let i1 = instance(&mut g, &r, &handle, c1);
    let i2 = instance(&mut g, &r, &handle, c2);
    let other = constant(&mut g, &r, 99.0);
    let reader = add(&mut g, &r, "math.negate", vec![]);
    g.connect(i1, "y", reader, "a").unwrap();
    let mut e = Evaluator::new();
    for (id, port) in [(reader, "out"), (i2, "y"), (other, "out")] {
        evaluate(&g, &r, &mut e, id, port);
    }
    assert!(e.dirty_nodes(&g).is_empty());
    source.put("a.bsg", &chain_text(3, 10.0, &base));
    reload_asset(&handle, &source, &g, &mut e).unwrap();
    assert_eq!(e.dirty_nodes(&g), BTreeSet::from([i1, i2, reader]));
    assert_eq!(evaluate(&g, &r, &mut e, reader, "out"), Value::F64(-32.0));
    assert_eq!(evaluate(&g, &r, &mut e, i2, "y"), Value::F64(35.0));
    let good = chain_text(3, 10.0, &base);
    source.put("a.bsg", &good.replace("y,2.out,out", "z,2.out,out"));
    assert_eq!(
        reload_asset(&handle, &source, &g, &mut e),
        Err(SubgraphError::PortsChanged {
            path: "a.bsg".into()
        })
    );
    assert!(e.dirty_nodes(&g).is_empty());
    source.0.borrow_mut().remove("a.bsg");
    assert_eq!(
        reload_asset(&handle, &source, &g, &mut e),
        Err(SubgraphError::MissingAsset {
            path: "a.bsg".into()
        })
    );
    assert!(e.dirty_nodes(&g).is_empty());
    source.put("a.bsg", &wrapper("a.bsg"));
    assert!(matches!(
        reload_asset(&handle, &source, &g, &mut e),
        Err(SubgraphError::Recursion { .. })
    ));
    assert!(e.dirty_nodes(&g).is_empty());
    source.put("a.bsg", &good.replace("schema=1", "schema=2"));
    assert!(matches!(
        reload_asset(&handle, &source, &g, &mut e),
        Err(SubgraphError::Format { .. })
    ));
    assert!(e.dirty_nodes(&g).is_empty());
    // A forced demand after failed reloads still uses the last successful asset.
    e.invalidate(i1);
    assert_eq!(evaluate(&g, &r, &mut e, reader, "out"), Value::F64(-32.0));
    // Reload may introduce a previously unused base kind, repeatedly.
    source.put(
        "a.bsg",
        &good
            .replace("t.add_constant", "math.negate")
            .replace("k=10", ""),
    );
    reload_asset(&handle, &source, &g, &mut e).unwrap();
    assert_eq!(evaluate(&g, &r, &mut e, i1, "y"), Value::F64(-2.0));
    source.put("a.bsg", &good);
    reload_asset(&handle, &source, &g, &mut e).unwrap();
    assert_eq!(evaluate(&g, &r, &mut e, i1, "y"), Value::F64(32.0));
}

#[test]
fn subgraph_recursion_and_bad_paths_rejected() {
    let source = MapSource::default();
    for (b, chain) in [
        (false, vec!["a.bsg".to_owned(), "a.bsg".to_owned()]),
        (
            true,
            vec!["a.bsg".to_owned(), "b.bsg".to_owned(), "a.bsg".to_owned()],
        ),
    ] {
        source.put("a.bsg", &wrapper(if b { "b.bsg" } else { "a.bsg" }));
        source.put("b.bsg", &wrapper("a.bsg"));
        let expected = SubgraphError::Recursion { chain };
        assert_eq!(
            load_asset("a.bsg", &source, &register_all, &mut kinds()).err(),
            Some(expected.clone())
        );
        assert_eq!(
            load_graph(&outer_text("a.bsg"), &source, &register_all).err(),
            Some(expected)
        );
    }
    let probe = ReadProbe {
        source: &source,
        reads: Cell::new(0),
    };
    for path in [
        "",
        "/a.bsg",
        "C:/a.bsg",
        "c:a.bsg",
        "dir\\a.bsg",
        "../a.bsg",
        "x/../a.bsg",
        "./a.bsg",
        "x//a.bsg",
        "a.txt",
        "a=b.bsg",
        "a,.bsg",
        "a;.bsg",
        "a\r.bsg",
        "a\n.bsg",
        "a\t.bsg",
        "a\0.bsg",
    ] {
        let expected = SubgraphError::BadPath { path: path.into() };
        assert_eq!(check_asset_path(path), Err(expected.clone()));
        assert_eq!(
            load_asset(path, &probe, &register_all, &mut kinds()).err(),
            Some(expected)
        );
    }
    assert_eq!(probe.reads.get(), 0);
    assert!(check_asset_path("dir/a.bsg").is_ok());
    assert!(matches!(
        directory().read("../a.bsg"),
        Err(SubgraphError::BadPath { .. })
    ));
    assert_eq!(
        load_graph(&outer_text("missing.bsg"), &source, &register_all).err(),
        Some(SubgraphError::MissingAsset {
            path: "missing.bsg".into()
        })
    );
    let base = base(Rc::new(Cell::new(0)));
    source.put("a.bsg", &chain_text(1, 1.0, &base));
    let duplicate = "schema=1\nkind=graph\nnodes\n0,asset/a.bsg,0,0,instance=0\n1,asset/a.bsg,0,0,instance=0\nedges\n";
    let expected = SubgraphError::DuplicateInstanceKey {
        type_name: "asset/a.bsg".into(),
        key: 0,
    };
    assert_eq!(
        load_graph(duplicate, &source, &base).err(),
        Some(expected.clone())
    );
    source.put(
        "b.bsg",
        &duplicate
            .replace("kind=graph", "kind=subgraph")
            .replace("edges\n", "edges\nexports\ny,0.y,out\n"),
    );
    assert_eq!(
        load_asset("b.bsg", &source, &base, &mut kinds()).err(),
        Some(expected)
    );
    // A hostile "comment" is not ignored by the format parser.
    source.put(
        "a.bsg",
        &format!("{}# asset/a.bsg\n", chain_text(1, 1.0, &base)),
    );
    assert!(matches!(
        load_asset("a.bsg", &source, &base, &mut kinds()),
        Err(SubgraphError::Format { .. })
    ));
}

#[test]
fn subgraph_nesting_depth_8_evaluates() {
    let count = Rc::new(Cell::new(0));
    let base = base(count.clone());
    let source = MapSource::default();
    source.put("d8.bsg", &chain_text(20_000, 1.0, &base));
    for depth in (1..8).rev() {
        source.put(
            &format!("d{depth}.bsg"),
            &wrapper(&format!("d{}.bsg", depth + 1)),
        );
    }
    let top = "schema=1\nkind=graph\nnodes\n0,math.constant,0,0,value=5\n1,asset/d1.bsg,0,0,instance=0\nedges\n0.out,1.x\n";
    let loaded = load_graph(top, &source, &base).unwrap();
    assert_eq!(loaded.assets.len(), 1);
    let mut evaluator = Evaluator::new();
    assert_eq!(
        evaluate(
            &loaded.graph,
            &loaded.registry,
            &mut evaluator,
            NodeId(1),
            "y"
        ),
        Value::F64(20_005.0)
    );
    assert_eq!(count.get(), 20_000);
    // A second enclosing instance must not reuse nested instances' key-zero caches.
    let mut graph = loaded.graph.clone();
    let c = constant(&mut graph, &loaded.registry, 7.0);
    let i = instance(&mut graph, &loaded.registry, &loaded.assets[0], c);
    assert_eq!(
        evaluate(&graph, &loaded.registry, &mut Evaluator::new(), i, "y"),
        Value::F64(20_007.0)
    );
    assert_eq!(count.get(), 40_000);
    evaluator.invalidate(NodeId(1));
    assert_eq!(
        evaluate(
            &loaded.graph,
            &loaded.registry,
            &mut evaluator,
            NodeId(1),
            "y"
        ),
        Value::F64(20_005.0)
    );
    assert_eq!(count.get(), 40_000);
    source.put("d8.bsg", &wrapper("d9.bsg"));
    source.put("d9.bsg", &chain_text(1, 1.0, &base));
    let expected = SubgraphError::TooDeep {
        chain: (1..=9).map(|depth| format!("d{depth}.bsg")).collect(),
    };
    assert_eq!(
        load_graph(top, &source, &base).err(),
        Some(expected.clone())
    );
    assert_eq!(
        load_asset("d1.bsg", &source, &base, &mut kinds()).err(),
        Some(expected.clone())
    );
    assert_eq!(
        reload_asset(&loaded.assets[0], &source, &loaded.graph, &mut evaluator),
        Err(expected)
    );
    assert!(evaluator.dirty_nodes(&loaded.graph).is_empty());
}
