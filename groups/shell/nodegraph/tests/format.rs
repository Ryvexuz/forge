use std::path::Path;

use nodegraph::format::{parse, text_allowed, write, FormatError};
use nodegraph::graph::{Graph, GraphError, NodeId, PortDir, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::kinds::register_all;
use nodegraph::value::{ImageBuf, Value, ValueType};

struct TestKind {
    name: &'static str,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

impl NodeKind for TestKind {
    fn type_name(&self) -> &str {
        self.name
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
    fn eval(&self, _: &[Value], _: &[Value]) -> Result<Vec<Value>, NodeError> {
        Err(NodeError::Evaluation("format-only test kind".into()))
    }
}

fn test_kind(name: &'static str, params: &[(&str, ValueType)]) -> TestKind {
    TestKind {
        name,
        inputs: vec![],
        outputs: vec![],
        params: params
            .iter()
            .map(|(name, ty)| ParamDesc {
                name: (*name).into(),
                value_type: ty.clone(),
            })
            .collect(),
    }
}

fn registry() -> KindRegistry {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).unwrap();
    let mut probe = test_kind(
        "test.probe",
        &[
            ("flag", ValueType::Bool),
            ("count", ValueType::Int),
            ("gain", ValueType::F64),
            ("label", ValueType::Text),
        ],
    );
    probe.inputs.push(PortDesc {
        name: "in".into(),
        value_type: ValueType::F64,
    });
    probe.outputs.push(PortDesc {
        name: "out".into(),
        value_type: ValueType::F64,
    });
    registry.add(Box::new(probe)).unwrap();
    registry
}

fn fixture() -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/graph_v1.bsg"),
    )
    .unwrap()
}

fn replace_line(text: &str, line: usize, replacement: &str) -> String {
    let mut lines: Vec<_> = text.split_terminator('\n').collect();
    lines[line - 1] = replacement;
    format!("{}\n", lines.join("\n"))
}

fn bad_field(line: usize, field: &'static str) -> FormatError {
    FormatError::BadField { line, field }
}

fn graph_error(line: usize, error: GraphError) -> FormatError {
    FormatError::Graph { line, error }
}

fn bad_name(line: usize, text: &str) -> FormatError {
    FormatError::BadName {
        line,
        text: text.into(),
    }
}

fn unsupported(line: usize, param: &str) -> FormatError {
    FormatError::Unsupported {
        line,
        param: param.into(),
    }
}

fn refuses(text: &str, expected: FormatError) {
    let registry = registry();
    assert_eq!(parse(text, &registry), Err(expected), "input: {text:?}");
    assert!(
        parse(&fixture(), &registry).is_ok(),
        "fixture after refusal"
    );
}

#[test]
fn graph_file_round_trip_byte_identical() {
    use Value::{Bool, Int, Text, F64};
    let registry = registry();
    let fixture = fixture();
    let parsed = parse(&fixture, &registry).unwrap();
    assert_eq!(parsed.node_count(), 12);
    assert_eq!(
        parsed.nodes().map(|(id, _)| id.0).collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13]
    );
    assert_eq!(parsed.edges().count(), 10);
    let mut built = Graph::new();
    let nodes = [
        (0, "math.constant", 0.0, 0.0, vec![F64(2.5)]),
        (1, "math.constant", 0.0, 40.0, vec![F64(-1e-7)]),
        (2, "math.add", 120.0, 20.0, vec![]),
        (3, "math.clamp", 240.0, 20.25, vec![F64(0.0), F64(10.0)]),
        (4, "math.negate", 360.0, -12.5, vec![]),
        (5, "image.solid", 0.0, 200.0, vec![Int(4), Int(3), F64(0.1)]),
        (6, "image.gradient", 0.1, 260.0, vec![Int(4), Int(3)]),
        (7, "image.blend", 140.0, 230.0, vec![F64(0.75)]),
        (8, "image.threshold", 280.0, 230.0, vec![F64(0.5)]),
        (9, "image.blur", 420.0, 230.0, vec![]),
        (
            12,
            "test.probe",
            480.0,
            20.0,
            vec![
                Bool(true),
                Int(-42),
                F64(12345678.875),
                Text("naïve = ok".into()),
            ],
        ),
        (
            13,
            "test.probe",
            600.0,
            20.0,
            vec![Bool(false), Int(i64::MAX), F64(0.0), Text("".into())],
        ),
    ];
    for (id, name, x, y, values) in nodes {
        built
            .add_node_with_id(&registry, NodeId(id), name, values, Position { x, y })
            .unwrap();
    }
    for (from, to, port) in [
        (0, 2, "a"),
        (1, 2, "b"),
        (2, 3, "x"),
        (3, 4, "a"),
        (4, 12, "in"),
        (5, 7, "a"),
        (6, 7, "b"),
        (7, 8, "a"),
        (8, 9, "a"),
        (12, 13, "in"),
    ] {
        built
            .connect(NodeId(from), "out", NodeId(to), port)
            .unwrap();
    }
    assert_eq!(parsed, built);
    assert_eq!(write(&parsed).unwrap(), fixture);
    assert_eq!(write(&built).unwrap(), fixture);
    let empty = "schema=1\nkind=graph\nnodes\nedges\n";
    assert_eq!(write(&Graph::new()).unwrap(), empty);
    assert_eq!(parse(empty, &registry).unwrap(), Graph::new());
    // Serialization does no arithmetic: compare extreme floats' bits directly.
    for (x, value) in [
        (f32::from_bits(1), f64::from_bits(1)),
        (f32::MIN_POSITIVE, f64::MIN_POSITIVE),
        (f32::MAX, f64::MAX),
        (f32::MIN, f64::MIN),
        (-0.0, -0.0),
    ] {
        let mut graph = Graph::new();
        let id = graph
            .add_node(
                &registry,
                "math.constant",
                vec![F64(value)],
                Position { x, y: -x },
            )
            .unwrap();
        let decoded = parse(&write(&graph).unwrap(), &registry).unwrap();
        let node = decoded.node(id).unwrap();
        assert_eq!(node.position.x.to_bits(), x.to_bits());
        assert_eq!(node.position.y.to_bits(), (-x).to_bits());
        let Value::F64(actual) = &node.params[0].1 else {
            panic!("wrong value type")
        };
        assert_eq!(actual.to_bits(), value.to_bits());
    }
}

#[test]
fn graph_file_rejects_unknown_version() {
    let fixture = fixture();
    for found in ["2", "0", "01", "1.0", ""] {
        refuses(
            &replace_line(&fixture, 1, &format!("schema={found}")),
            FormatError::UnknownVersion {
                found: found.into(),
            },
        );
    }
    refuses(
        "schema=2\n",
        FormatError::UnknownVersion { found: "2".into() },
    );
    refuses(
        &replace_line(&fixture, 1, "scheme=1"),
        FormatError::BadHeader { line: 1 },
    );
}

#[test]
fn graph_file_rejects_bad_input() {
    let fixture = fixture();
    refuses(
        fixture.trim_end_matches('\n'),
        FormatError::MissingFinalNewline,
    );
    refuses("", FormatError::MissingFinalNewline);
    refuses(
        &fixture.replace('\n', "\r\n"),
        FormatError::CarriageReturn { line: 1 },
    );
    let unknown_type = GraphError::Kind(NodeError::UnknownType("math.pow".into()));
    let unknown_port = GraphError::UnknownPort {
        node: NodeId(3),
        port: "c".into(),
        dir: PortDir::Input,
    };
    let mismatch = GraphError::TypeMismatch {
        from: ValueType::F64,
        to: ValueType::Image,
    };
    let cases = [
        (2, "kind=asset", FormatError::BadHeader { line: 2 }),
        (4, "", FormatError::EmptyLine { line: 4 }),
        (4, "0,math.constant,0,value=2.5", bad_field(4, "fields")),
        (4, "x,math.constant,0,0,value=2.5", bad_field(4, "id")),
        (4, "0,math.constant,abc,0,value=2.5", bad_field(4, "x")),
        (
            4,
            "0,math.pow,0,0,value=2.5",
            graph_error(4, unknown_type.clone()),
        ),
        (4, "0,math.pow,0,0,broken", graph_error(4, unknown_type)),
        (
            4,
            "0,math.constant,0,NaN,value=2.5",
            graph_error(4, GraphError::NonFinite("position".into())),
        ),
        (4, "0,math.constant,0,0,val=2.5", bad_field(4, "param")),
        (7, "3,math.clamp,240,20.25,lo=0", bad_field(7, "param")),
        (
            14,
            "12,test.probe,480,20,flag=yes;count=-42;gain=12345678.875;label=naïve = ok",
            bad_field(14, "value"),
        ),
        (19, "2.out,3.c", graph_error(19, unknown_port)),
        (21, "4.out,9.a", graph_error(21, mismatch)),
        (17, "0out,2.a", bad_field(17, "edge")),
        (
            4,
            "18446744073709551616,math.constant,0,0,value=2.5",
            bad_field(4, "id"),
        ),
        (4, "0,math.constant,0,abc,value=2.5", bad_field(4, "y")),
        (
            4,
            "0,math.constant,0,0,value=1e999",
            graph_error(4, GraphError::NonFinite("param value".into())),
        ),
        (
            4,
            "0,math.constant,0,0,value=2.5;extra=1",
            bad_field(4, "param"),
        ),
        (4, "# comment", bad_field(4, "fields")),
        (
            20,
            "4.out,4.a",
            graph_error(
                20,
                GraphError::Cycle {
                    from: NodeId(4),
                    to: NodeId(4),
                },
            ),
        ),
    ];
    for (line, replacement, expected) in cases {
        refuses(&replace_line(&fixture, line, replacement), expected);
    }
    refuses(
        &fixture.replacen("nodes\n", "", 1),
        FormatError::BadHeader { line: 3 },
    );
    refuses(
        &fixture.replacen("nodes\n", "nodes\n\n", 1),
        FormatError::EmptyLine { line: 4 },
    );
    refuses(
        &fixture.replacen("value=2.5\n", "value=2.5\n0,math.constant,0,0,value=1\n", 1),
        graph_error(5, GraphError::DuplicateNodeId(NodeId(0))),
    );
    refuses(
        &fixture.replacen("0.out,2.a\n", "0.out,2.a\n0.out,2.a\n", 1),
        graph_error(
            18,
            GraphError::InputTaken {
                node: NodeId(2),
                port: "a".into(),
                source: NodeId(0),
            },
        ),
    );
    for text in ["a\\b", "C:x", "a\tb", "a\u{85}b"] {
        refuses(&fixture.replace("naïve = ok", text), bad_name(14, text));
    }
    refuses(
        &format!("{fixture}exports\n"),
        FormatError::BadHeader { line: 27 },
    );
    refuses(
        "schema=1\nkind=graph\nnodes\n",
        FormatError::BadHeader { line: 4 },
    );
    // Large hostile Text is validated, not truncated into an acceptable prefix.
    let hostile = format!("{}\\", "a".repeat(65_536));
    refuses(
        &fixture.replace("naïve = ok", &hostile),
        bad_name(14, &hostile),
    );
}

#[test]
fn graph_file_rejects_non_canonical_numbers() {
    let fixture = fixture();
    for (old, new, line) in [
        ("value=2.5\n", "value=2.50\n", 4),
        ("value=2.5\n", "value=+2.5\n", 4),
        ("value=2.5\n", "value=2.5e0\n", 4),
        ("math.add,120,", "math.add,120.0,", 6),
        ("width=4;height=3;value", "width=04;height=3;value", 9),
        ("count=-42", "count=-0", 14),
        (
            "count=9223372036854775807",
            "count=+9223372036854775807",
            15,
        ),
        ("0,math.constant,0,0,", "00,math.constant,0,0,", 4),
    ] {
        assert!(fixture.contains(old), "mutation must change input");
        refuses(
            &fixture.replacen(old, new, 1),
            FormatError::NotCanonical { line },
        );
    }
    for (a, b) in [(7, 8), (17, 18)] {
        let mut lines: Vec<_> = fixture.split_terminator('\n').collect();
        lines.swap(a - 1, b - 1);
        refuses(
            &format!("{}\n", lines.join("\n")),
            FormatError::NotCanonical { line: a },
        );
    }
}

#[test]
fn subgraph_text_round_trips() {
    let registry = registry();
    let mut graph = Graph::new();
    let first = graph
        .add_node(
            &registry,
            "math.constant",
            vec![Value::F64(1.0)],
            Position { x: 0.0, y: 0.0 },
        )
        .unwrap();
    let second = graph
        .add_node(&registry, "math.add", vec![], Position { x: 50.0, y: 0.0 })
        .unwrap();
    graph.connect(first, "out", second, "a").unwrap();
    graph.export("rhs", second, "b", PortDir::Input).unwrap();
    graph.export("sum", second, "out", PortDir::Output).unwrap();
    let text = "schema=1\nkind=subgraph\nnodes\n0,math.constant,0,0,value=1\n1,math.add,50,0,\nedges\n0.out,1.a\nexports\nrhs,1.b,in\nsum,1.out,out\n";
    assert_eq!(write(&graph).unwrap(), text);
    assert_eq!(parse(text, &registry).unwrap(), graph);
    refuses(
        &text.replace("rhs,1.b,in\nsum,1.out,out\n", ""),
        FormatError::NoExports,
    );
    refuses(
        &text.replace("exports\nrhs,1.b,in\nsum,1.out,out\n", ""),
        FormatError::BadHeader { line: 8 },
    );
    refuses(&text.replace("1.b,in", "1.b,inout"), bad_field(9, "dir"));
    refuses(&text.replace("rhs,", "C:b,"), bad_name(9, "C:b"));
    refuses(
        &text.replace("sum,1.out,out", "rhs,1.out,out"),
        graph_error(10, GraphError::DuplicateExport("rhs".into())),
    );
    refuses(
        &text.replace("rhs,1.b,in\nsum,1.out,out", "sum,1.out,out\nrhs,1.b,in"),
        FormatError::NotCanonical { line: 9 },
    );
}

#[test]
fn write_refuses_unwritable_graphs() {
    let mut registry = registry();
    for kind in [
        test_kind("t.label", &[("label", ValueType::Text)]),
        test_kind("t.img", &[("image", ValueType::Image)]),
        test_kind("t.bad", &[("a=b", ValueType::Text)]),
        test_kind("C:x", &[("label", ValueType::Text)]),
        test_kind("t.named", &[("custom", ValueType::Named("tag".into()))]),
    ] {
        registry.add(Box::new(kind)).unwrap();
    }
    let build = |name: &str, value: Value| {
        let mut graph = Graph::new();
        graph
            .add_node(&registry, name, vec![value], Position { x: 0.0, y: 0.0 })
            .unwrap();
        graph
    };
    for text in ["a,b", "a;b", "a\\b", "C:x", "ratio:2", "x\ny"] {
        assert_eq!(
            write(&build("t.label", Value::Text(text.into()))),
            Err(bad_name(4, text)),
        );
    }
    let graph = build("t.label", Value::Text("10:30".into()));
    assert_eq!(parse(&write(&graph).unwrap(), &registry).unwrap(), graph);
    let image = Value::Image(ImageBuf {
        width: 1,
        height: 1,
        pixels: vec![0.0],
    });
    assert_eq!(write(&build("t.img", image)), Err(unsupported(4, "image")));
    assert_eq!(
        write(&build("t.bad", Value::Text("ok".into()))),
        Err(bad_name(4, "a=b")),
    );
    assert_eq!(
        write(&build("C:x", Value::Text("ok".into()))),
        Err(bad_name(4, "C:x")),
    );
    for (name, param) in [("t.img", "image"), ("t.named", "custom")] {
        let text = format!("schema=1\nkind=graph\nnodes\n0,{name},0,0,{param}=raw\nedges\n");
        assert_eq!(parse(&text, &registry), Err(unsupported(4, param)));
    }
    let mut exported = Graph::new();
    let id = exported
        .add_node(
            &registry,
            "math.constant",
            vec![Value::F64(1.0)],
            Position { x: 0.0, y: 0.0 },
        )
        .unwrap();
    exported.export("C:x", id, "out", PortDir::Output).unwrap();
    assert_eq!(write(&exported), Err(bad_name(7, "C:x")));
    assert!(text_allowed(""));
    assert!(!text_allowed("C:"));
}
