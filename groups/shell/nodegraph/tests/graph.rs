use nodegraph::graph::{Export, Graph, GraphError, NodeId, PortDir, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::value::{Value, ValueType};

struct Kind {
    name: String,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

impl NodeKind for Kind {
    fn type_name(&self) -> &str {
        &self.name
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
        Err(NodeError::Evaluation("test kind is not evaluable".into()))
    }
}

fn registry() -> KindRegistry {
    use ValueType::{F64, Int, Named, Text};
    let mut r = KindRegistry::new();
    let specifications = [
        ("t.num", vec![], vec![("v", F64)], vec![("value", F64)]),
        (
            "t.add",
            vec![("a", F64), ("b", F64)],
            vec![("sum", F64)],
            vec![],
        ),
        ("t.pass", vec![("x", F64)], vec![("y", F64)], vec![]),
        ("t.text", vec![], vec![("s", Text)], vec![]),
        ("t.show", vec![("s", Text)], vec![], vec![]),
        ("t.int", vec![], vec![("i", Int)], vec![]),
        ("t.na", vec![], vec![("o", Named("a".into()))], vec![]),
        ("t.nb", vec![("i", Named("b".into()))], vec![], vec![]),
        ("t.dup", vec![("x", F64), ("x", F64)], vec![], vec![]),
        ("t.image", vec![], vec![], vec![("image", ValueType::Image)]),
    ];
    for (name, inputs, outputs, params) in specifications {
        let ports = |items: Vec<(&str, ValueType)>| {
            items
                .into_iter()
                .map(|(name, value_type)| PortDesc {
                    name: name.into(),
                    value_type,
                })
                .collect()
        };
        r.add(Box::new(Kind {
            name: name.into(),
            inputs: ports(inputs),
            outputs: ports(outputs),
            params: params
                .into_iter()
                .map(|(name, value_type)| ParamDesc {
                    name: name.into(),
                    value_type,
                })
                .collect(),
        }))
        .unwrap();
    }
    r
}

const ORIGIN: Position = Position { x: 0.0, y: 0.0 };

fn add(g: &mut Graph, r: &KindRegistry, kind: &str) -> NodeId {
    let params = if kind == "t.num" {
        vec![Value::F64(1.0)]
    } else {
        vec![]
    };
    g.add_node(r, kind, params, ORIGIN).unwrap()
}

fn refuses<T: std::fmt::Debug + PartialEq>(
    g: &mut Graph,
    expected: GraphError,
    edit: impl FnOnce(&mut Graph) -> Result<T, GraphError>,
) {
    let before = g.clone();
    let version = g.version();
    let revisions: Vec<_> = g.nodes().map(|(id, _)| (id, g.revision(id))).collect();
    assert_eq!(edit(g), Err(expected));
    assert_eq!(*g, before);
    assert_eq!(g.version(), version);
    for (id, revision) in revisions {
        assert_eq!(g.revision(id), revision);
    }
}

#[test]
fn graph_rejects_invalid_edits_and_stays_unchanged() {
    use GraphError::*;
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.pass");
    let missing = NodeId(999);
    refuses(
        &mut g,
        Kind(NodeError::UnknownType("missing".into())),
        |g| g.add_node(&r, "missing", vec![], ORIGIN),
    );
    refuses(
        &mut g,
        DuplicateName {
            type_name: "t.dup".into(),
            name: "x".into(),
        },
        |g| g.add_node(&r, "t.dup", vec![], ORIGIN),
    );
    refuses(
        &mut g,
        ParamCount {
            expected: 1,
            got: 0,
        },
        |g| g.add_node(&r, "t.num", vec![], ORIGIN),
    );
    let wrong = ParamType {
        name: "value".into(),
        expected: ValueType::F64,
        got: ValueType::Int,
    };
    refuses(&mut g, wrong.clone(), |g| {
        g.add_node(&r, "t.num", vec![Value::Int(1)], ORIGIN)
    });
    refuses(&mut g, NonFinite("param value".into()), |g| {
        g.add_node(&r, "t.num", vec![Value::F64(f64::NAN)], ORIGIN)
    });
    let infinite = Position {
        x: f32::INFINITY,
        y: 0.0,
    };
    refuses(&mut g, NonFinite("position".into()), |g| {
        g.add_node(&r, "t.pass", vec![], infinite)
    });
    refuses(&mut g, DuplicateNodeId(a), |g| {
        g.add_node_with_id(&r, a, "t.pass", vec![], ORIGIN)
    });
    refuses(&mut g, IdsExhausted, |g| {
        g.add_node_with_id(&r, NodeId(u64::MAX), "t.pass", vec![], ORIGIN)
    });
    refuses(&mut g, UnknownNode(missing), |g| {
        g.connect(missing, "v", b, "x")
    });
    refuses(&mut g, UnknownNode(missing), |g| {
        g.connect(a, "v", missing, "x")
    });
    for (node, dir) in [(a, PortDir::Output), (b, PortDir::Input)] {
        refuses(
            &mut g,
            UnknownPort {
                node,
                port: "bad".into(),
                dir,
            },
            |g| {
                if dir == PortDir::Output {
                    g.connect(a, "bad", b, "x")
                } else {
                    g.connect(a, "v", b, "bad")
                }
            },
        );
    }
    refuses(
        &mut g,
        NotConnected {
            node: b,
            port: "x".into(),
        },
        |g| g.disconnect(b, "x"),
    );
    refuses(&mut g, UnknownNode(missing), |g| {
        g.disconnect(missing, "x")
    });
    refuses(
        &mut g,
        UnknownPort {
            node: b,
            port: "bad".into(),
            dir: PortDir::Input,
        },
        |g| g.disconnect(b, "bad"),
    );
    g.connect(a, "v", b, "x").unwrap();
    refuses(&mut g, NodeInUse { node: a, reader: b }, |g| {
        g.remove_node(a)
    });
    refuses(&mut g, UnknownNode(missing), |g| g.remove_node(missing));
    refuses(
        &mut g,
        UnknownParam {
            node: a,
            name: "bad".into(),
        },
        |g| g.set_param(a, "bad", Value::Int(1)),
    );
    refuses(&mut g, UnknownNode(missing), |g| {
        g.set_param(missing, "value", Value::Int(1))
    });
    refuses(&mut g, wrong, |g| {
        g.set_param(a, "value", Value::Int(1))
    });
    refuses(&mut g, NonFinite("param value".into()), |g| {
        g.set_param(a, "value", Value::F64(f64::NAN))
    });
    refuses(&mut g, NonFinite("position".into()), |g| {
        g.set_position(a, infinite)
    });
    refuses(&mut g, UnknownNode(missing), |g| {
        g.set_position(missing, ORIGIN)
    });
    refuses(&mut g, BadExportName("".into()), |g| {
        g.export("", b, "x", PortDir::Input)
    });
    g.export("in", b, "x", PortDir::Input).unwrap();
    refuses(&mut g, DuplicateExport("in".into()), |g| {
        g.export("in", a, "v", PortDir::Output)
    });
    refuses(
        &mut g,
        NodeExported {
            node: b,
            name: "in".into(),
        },
        |g| g.remove_node(b),
    );
    refuses(&mut g, UnknownExport("bad".into()), |g| g.unexport("bad"));
    refuses(
        &mut g,
        UnknownPort {
            node: b,
            port: "bad".into(),
            dir: PortDir::Output,
        },
        |g| g.export("bad", b, "bad", PortDir::Output),
    );
    refuses(&mut g, UnknownNode(missing), |g| {
        g.export("missing", missing, "x", PortDir::Input)
    });
    refuses(&mut g, NonFinite("param image".into()), |g| {
        g.add_node(
            &r,
            "t.image",
            vec![Value::Image(nodegraph::value::ImageBuf {
                width: 1,
                height: 1,
                pixels: vec![f32::NAN],
            })],
            ORIGIN,
        )
    });
    // Failed fixed-id insertion must not advance the automatic allocator.
    assert_eq!(add(&mut g, &r, "t.pass"), NodeId(2));
    for value in [f64::MAX, -f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
        assert_eq!(
            g.set_param(a, "value", Value::F64(value)).unwrap().type_of(),
            ValueType::F64
        );
        assert_eq!(g.node(a).unwrap().params[0].1, Value::F64(value));
    }
    let mut exhausted = Graph::new();
    exhausted
        .add_node_with_id(&r, NodeId(u64::MAX - 1), "t.pass", vec![], ORIGIN)
        .unwrap();
    exhausted.remove_node(NodeId(u64::MAX - 1)).unwrap();
    refuses(&mut exhausted, IdsExhausted, |g| {
        g.add_node(&r, "t.pass", vec![], ORIGIN)
    });
}

#[test]
fn connect_rejects_type_mismatch() {
    let r = registry();
    let mut g = Graph::new();
    for (from_kind, out, to_kind, input, from_type, to_type) in [
        ("t.num", "v", "t.show", "s", ValueType::F64, ValueType::Text),
        ("t.int", "i", "t.pass", "x", ValueType::Int, ValueType::F64),
        (
            "t.na",
            "o",
            "t.nb",
            "i",
            ValueType::Named("a".into()),
            ValueType::Named("b".into()),
        ),
    ] {
        let from = add(&mut g, &r, from_kind);
        let to = add(&mut g, &r, to_kind);
        refuses(
            &mut g,
            GraphError::TypeMismatch {
                from: from_type,
                to: to_type,
            },
            |g| g.connect(from, out, to, input),
        );
    }
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.pass");
    g.connect(a, "v", b, "x").unwrap();
}

#[test]
fn connect_rejects_cycle() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.pass");
    let b = add(&mut g, &r, "t.pass");
    let c = add(&mut g, &r, "t.pass");
    refuses(&mut g, GraphError::Cycle { from: a, to: a }, |g| {
        g.connect(a, "y", a, "x")
    });
    g.connect(a, "y", b, "x").unwrap();
    refuses(&mut g, GraphError::Cycle { from: b, to: a }, |g| {
        g.connect(b, "y", a, "x")
    });
    g.connect(b, "y", c, "x").unwrap();
    refuses(&mut g, GraphError::Cycle { from: c, to: a }, |g| {
        g.connect(c, "y", a, "x")
    });
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let r = registry();
            let mut g = Graph::new();
            let first = add(&mut g, &r, "t.pass");
            let mut last = first;
            for _ in 1..100_000 {
                let next = add(&mut g, &r, "t.pass");
                g.connect(last, "y", next, "x").unwrap();
                last = next;
            }
            refuses(
                &mut g,
                GraphError::Cycle {
                    from: last,
                    to: first,
                },
                |g| g.connect(last, "y", first, "x"),
            );
            assert_eq!(g.descendants(first).unwrap().len(), 99_999);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn connect_rejects_second_source_on_one_input() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.num");
    let c = add(&mut g, &r, "t.add");
    g.connect(a, "v", c, "a").unwrap();
    for source in [b, a] {
        refuses(
            &mut g,
            GraphError::InputTaken {
                node: c,
                port: "a".into(),
                source: a,
            },
            |g| g.connect(source, "v", c, "a"),
        );
    }
    g.disconnect(c, "a").unwrap();
    g.connect(b, "v", c, "a").unwrap();
}

fn no_dangling(g: &Graph) {
    for e in g.edges() {
        assert!(g.node(e.from).is_some());
        assert!(g.node(e.to).is_some());
    }
    for e in g.exports().values() {
        assert!(g.node(e.node).is_some());
    }
}

#[test]
fn remove_and_disconnect_leave_no_dangling_edge() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.num");
    let c = add(&mut g, &r, "t.add");
    g.connect(b, "v", c, "b").unwrap();
    g.connect(a, "v", c, "a").unwrap();
    g.export("value", a, "v", PortDir::Output).unwrap();
    let expected: Vec<_> = g.edges().cloned().collect();
    let old = g.node(c).unwrap().clone();
    assert_eq!(g.remove_node(c).unwrap(), (old, expected));
    no_dangling(&g);
    assert_eq!(g.edges().count(), 0);
    let d = add(&mut g, &r, "t.pass");
    assert!(d > c && d > b && d > a);
    g.connect(a, "v", d, "x").unwrap();
    let edge = g.source_of(d, "x").unwrap().clone();
    assert_eq!(g.disconnect(d, "x").unwrap(), edge);
    assert_eq!(g.source_of(d, "x"), None);
    no_dangling(&g);
}

#[test]
fn descendants_are_exactly_downstream() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.pass");
    let c = add(&mut g, &r, "t.pass");
    let d = add(&mut g, &r, "t.add");
    let e = add(&mut g, &r, "t.pass");
    g.connect(a, "v", b, "x").unwrap();
    g.connect(a, "v", c, "x").unwrap();
    g.connect(b, "y", d, "a").unwrap();
    g.connect(c, "y", d, "b").unwrap();
    assert_eq!(
        g.descendants(a).unwrap(),
        std::collections::BTreeSet::from([b, c, d])
    );
    assert_eq!(
        g.descendants(b).unwrap(),
        std::collections::BTreeSet::from([d])
    );
    assert!(g.descendants(d).unwrap().is_empty());
    assert!(g.descendants(e).unwrap().is_empty());
    assert_eq!(
        g.descendants(NodeId(999)),
        Err(GraphError::UnknownNode(NodeId(999)))
    );
}

#[test]
fn graph_equality_ignores_counters_and_edit_order() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    let b = add(&mut g, &r, "t.add");
    let mut h = g.clone();
    g.connect(a, "v", b, "a").unwrap();
    g.connect(a, "v", b, "b").unwrap();
    h.connect(a, "v", b, "b").unwrap();
    h.connect(a, "v", b, "a").unwrap();
    assert_eq!(g, h);
    let mut third = h.clone();
    let extra = add(&mut third, &r, "t.pass");
    third.remove_node(extra).unwrap();
    assert_eq!(g, third);
    assert_ne!(g.version(), third.version());
    h.set_param(a, "value", Value::F64(2.0)).unwrap();
    assert_ne!(g, h);
    h = g.clone();
    h.set_position(a, Position { x: 1.0, y: 2.0 }).unwrap();
    assert_ne!(g, h);
    h = g.clone();
    h.export("out", a, "v", PortDir::Output).unwrap();
    assert_ne!(g, h);
}

#[test]
fn version_and_revisions_track_edits() {
    let r = registry();
    let mut g = Graph::new();
    let a = add(&mut g, &r, "t.num");
    assert_eq!(g.version(), 1);
    assert_eq!(g.revision(a), Some(1));
    let b = add(&mut g, &r, "t.pass");
    assert_eq!(g.version(), 2);
    assert_eq!(g.revision(b), Some(2));
    g.set_position(
        a,
        Position {
            x: f32::MAX,
            y: f32::MIN_POSITIVE,
        },
    )
    .unwrap();
    assert_eq!(g.version(), 3);
    assert_eq!(g.revision(a), Some(1));
    assert_eq!(g.revision(b), Some(2));
    g.export("out", a, "v", PortDir::Output).unwrap();
    assert_eq!(g.version(), 4);
    assert_eq!(g.revision(a), Some(1));
    assert_eq!(g.revision(b), Some(2));
    assert_eq!(
        g.set_param(a, "value", Value::F64(f64::MAX)).unwrap(),
        Value::F64(1.0)
    );
    assert_eq!(g.version(), 5);
    assert_eq!(g.revision(a), Some(5));
    assert_eq!(g.revision(b), Some(2));
    g.connect(a, "v", b, "x").unwrap();
    assert_eq!(g.version(), 6);
    assert_eq!(g.revision(a), Some(5));
    assert_eq!(g.revision(b), Some(6));
    g.disconnect(b, "x").unwrap();
    assert_eq!(g.version(), 7);
    assert_eq!(g.revision(a), Some(5));
    assert_eq!(g.revision(b), Some(7));
    g.unexport("out").unwrap();
    assert_eq!(g.version(), 8);
    assert_eq!(g.revision(a), Some(5));
    g.remove_node(b).unwrap();
    assert_eq!(g.version(), 9);
    assert_eq!(g.revision(b), None);
    g.add_node_with_id(&r, b, "t.pass", vec![], ORIGIN).unwrap();
    assert_eq!(g.version(), 10);
    assert_eq!(g.revision(b), Some(10));
    g.add_node_with_id(&r, NodeId(100), "t.pass", vec![], ORIGIN)
        .unwrap();
    assert_eq!(g.version(), 11);
    assert_eq!(g.revision(NodeId(100)), Some(11));
    assert_eq!(add(&mut g, &r, "t.pass"), NodeId(101));
    assert_eq!(g.version(), 12);
    assert_eq!(g.revision(NodeId(101)), Some(12));
}

#[test]
fn exports_name_inner_ports() {
    let r = registry();
    let mut g = Graph::new();
    let n = add(&mut g, &r, "t.pass");
    let input = Export {
        node: n,
        port: "x".into(),
        dir: PortDir::Input,
    };
    let output = Export {
        node: n,
        port: "y".into(),
        dir: PortDir::Output,
    };
    g.export("in", n, "x", PortDir::Input).unwrap();
    g.export("out", n, "y", PortDir::Output).unwrap();
    assert_eq!(g.exports().get("in"), Some(&input));
    assert_eq!(g.exports().get("out"), Some(&output));
    refuses(&mut g, GraphError::DuplicateExport("in".into()), |g| {
        g.export("in", n, "y", PortDir::Output)
    });
    assert_eq!(g.unexport("in").unwrap(), input);
    g.export("in", n, "y", PortDir::Output).unwrap();
    assert_eq!(g.exports().get("in"), Some(&output));
}
