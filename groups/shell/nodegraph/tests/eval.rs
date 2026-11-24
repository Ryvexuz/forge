use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use nodegraph::eval::{EvalError, Evaluator};
use nodegraph::graph::{Graph, NodeId, PortDir, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::value::{Value, ValueType};

const ORIGIN: Position = Position { x: 0.0, y: 0.0 };

// Fixtures use small integer-valued F64s, so expected sums compare exactly.
struct TestKind {
    name: &'static str,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
    calls: Rc<Cell<u64>>,
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
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        self.calls.set(self.calls.get() + 1);
        let result = match (self.name, params, inputs) {
            ("t.const", [Value::F64(value)], []) => *value,
            ("t.add", [Value::F64(k)], [Value::F64(value)]) => value + k,
            ("t.sum", [Value::F64(k)], [Value::F64(a), Value::F64(b)]) => a + b + k,
            ("t.fail", [Value::Int(mode)], [Value::F64(value)]) => match mode {
                0 => *value,
                1 => return Err(NodeError::Evaluation("planted".into())),
                2 => panic!("planted panic"),
                3 => return Ok(vec![Value::F64(*value), Value::F64(*value)]),
                4 => return Ok(vec![Value::Bool(true)]),
                5 => std::panic::panic_any(String::from("owned panic")),
                6 => std::panic::panic_any(7_u8),
                _ => return Err(NodeError::Evaluation("unknown mode".into())),
            },
            _ => return Err(NodeError::Evaluation("invalid test arguments".into())),
        };
        Ok(vec![Value::F64(result)])
    }
}

fn registry() -> (KindRegistry, Rc<Cell<u64>>) {
    let calls = Rc::new(Cell::new(0));
    let mut registry = KindRegistry::new();
    for (name, inputs, param, value_type) in [
        ("t.const", vec![], "value", ValueType::F64),
        ("t.add", vec!["in"], "k", ValueType::F64),
        ("t.sum", vec!["a", "b"], "k", ValueType::F64),
        ("t.fail", vec!["in"], "mode", ValueType::Int),
    ] {
        let port = |name: &str| PortDesc {
            name: name.into(),
            value_type: ValueType::F64,
        };
        registry
            .add(Box::new(TestKind {
                name,
                inputs: inputs.into_iter().map(port).collect(),
                outputs: vec![port("out")],
                params: vec![ParamDesc {
                    name: param.into(),
                    value_type,
                }],
                calls: Rc::clone(&calls),
            }))
            .unwrap();
    }
    (registry, calls)
}

fn add(g: &mut Graph, r: &KindRegistry, kind: &str, param: f64) -> NodeId {
    let params = vec![Value::F64(param)];
    g.add_node(r, kind, params, ORIGIN).unwrap()
}

fn wire(g: &mut Graph, from: NodeId, to: NodeId, port: &str) {
    g.connect(from, "out", to, port).unwrap();
}

fn read(e: &mut Evaluator, g: &Graph, r: &KindRegistry, id: NodeId, expected: f64) {
    assert_eq!(e.evaluate(g, r, (id, "out")), Ok(Value::F64(expected)));
}

// Both counters are reset only after independently asserting the expected delta.
fn calls(e: &mut Evaluator, counter: &Cell<u64>, expected: u64) {
    assert_eq!(e.stats().calls, expected, "evaluator call delta");
    assert_eq!(counter.get(), expected, "independent kind call delta");
    e.reset_stats();
    counter.set(0);
}

fn dirty(e: &Evaluator, g: &Graph, expected: BTreeSet<NodeId>) {
    assert_eq!(e.dirty_nodes(g), expected);
    for (id, _) in g.nodes() {
        assert_eq!(e.is_dirty(g, id), expected.contains(&id), "{id:?}");
    }
    assert!(!e.is_dirty(g, NodeId(u64::MAX)));
}

fn shape_a(r: &KindRegistry) -> (Graph, Vec<NodeId>, Vec<NodeId>) {
    let mut g = Graph::new();
    let mut heads = Vec::new();
    let mut tails = Vec::new();
    for c in 0..50 {
        let head = add(&mut g, r, "t.const", f64::from(c));
        heads.push(head);
        let mut tail = head;
        for _ in 0..9 {
            let next = add(&mut g, r, "t.add", 1.0);
            wire(&mut g, tail, next, "in");
            tail = next;
        }
        tails.push(tail);
    }
    (g, heads, tails)
}

fn read_tails(e: &mut Evaluator, g: &Graph, r: &KindRegistry, tails: &[NodeId], edited: bool) {
    for (c, &tail) in tails.iter().enumerate() {
        let expected = if edited && c == 7 {
            109.0
        } else {
            f64::from(u32::try_from(c).unwrap()) + 9.0
        };
        read(e, g, r, tail, expected);
    }
}

#[test]
fn reeval_only_dirty_nodes_500() {
    let (r, counter) = registry();
    let (mut g, heads, tails) = shape_a(&r);
    let mut e = Evaluator::new();
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 500);
    g.set_param(heads[7], "value", Value::F64(100.0)).unwrap();
    let mut expected = g.descendants(heads[7]).unwrap();
    expected.insert(heads[7]);
    assert_eq!(expected.len(), 10);
    dirty(&e, &g, expected);
    read_tails(&mut e, &g, &r, &tails, true);
    calls(&mut e, &counter, 10);
    read_tails(&mut e, &g, &r, &tails, true);
    calls(&mut e, &counter, 0);
}

#[test]
fn dirty_marks_exactly_downstream() {
    let (r, counter) = registry();
    let mut g = Graph::new();
    let c0 = add(&mut g, &r, "t.const", 2.0);
    let a1 = add(&mut g, &r, "t.add", 1.0);
    let a2 = add(&mut g, &r, "t.add", 2.0);
    let s = add(&mut g, &r, "t.sum", 4.0);
    let a3 = add(&mut g, &r, "t.add", 3.0);
    let c9 = add(&mut g, &r, "t.const", 9.0);
    let a9 = add(&mut g, &r, "t.add", 1.0);
    for (from, to, port) in [
        (c0, a1, "in"),
        (c0, a2, "in"),
        (a1, s, "a"),
        (a2, s, "b"),
        (s, a3, "in"),
        (c9, a9, "in"),
    ] {
        wire(&mut g, from, to, port);
    }
    for (n, param, expected_a3, expected_a9) in [
        (c0, "value", 50.0, 10.0),
        (a1, "k", 33.0, 10.0),
        (a2, "k", 32.0, 10.0),
        (s, "k", 30.0, 10.0),
        (a3, "k", 31.0, 10.0),
        (c9, "value", 14.0, 21.0),
        (a9, "k", 14.0, 29.0),
    ] {
        let mut edited = g.clone();
        let mut e = Evaluator::new();
        read(&mut e, &edited, &r, a3, 14.0);
        read(&mut e, &edited, &r, a9, 10.0);
        calls(&mut e, &counter, 7);
        edited.set_param(n, param, Value::F64(20.0)).unwrap();
        let mut expected = edited.descendants(n).unwrap();
        expected.insert(n);
        let count = expected.len() as u64;
        dirty(&e, &edited, expected);
        read(&mut e, &edited, &r, a3, expected_a3);
        read(&mut e, &edited, &r, a9, expected_a9);
        calls(&mut e, &counter, count);
    }

    let mut e = Evaluator::new();
    read(&mut e, &g, &r, a3, 14.0);
    read(&mut e, &g, &r, a9, 10.0);
    calls(&mut e, &counter, 7);
    g.disconnect(s, "b").unwrap();
    dirty(&e, &g, BTreeSet::from([s, a3]));
    assert_eq!(
        e.evaluate(&g, &r, (a3, "out")),
        Err(EvalError::UnconnectedInput {
            node: s,
            port: "b".into(),
        })
    );
    calls(&mut e, &counter, 0);
    wire(&mut g, c9, s, "b");
    dirty(&e, &g, BTreeSet::from([s, a3]));
    read(&mut e, &g, &r, a3, 19.0);
    calls(&mut e, &counter, 2);
    // Restore a2's reader before checking its downstream invalidation.
    g.disconnect(s, "b").unwrap();
    wire(&mut g, a2, s, "b");
    read(&mut e, &g, &r, a3, 14.0);
    calls(&mut e, &counter, 2);
    e.invalidate(a2);
    dirty(&e, &g, BTreeSet::from([a2, s, a3]));
    read(&mut e, &g, &r, a3, 14.0);
    calls(&mut e, &counter, 3);
    let position = Position { x: 5.0, y: -2.0 };
    g.set_position(a1, position).unwrap();
    g.export("x", a3, "out", PortDir::Output).unwrap();
    dirty(&e, &g, BTreeSet::new());
    g.unexport("x").unwrap();
    dirty(&e, &g, BTreeSet::new());
    let added = add(&mut g, &r, "t.const", 8.0);
    dirty(&e, &g, BTreeSet::from([added]));
    read(&mut e, &g, &r, added, 8.0);
    calls(&mut e, &counter, 1);
    g.remove_node(added).unwrap();
    read(&mut e, &g, &r, a3, 14.0);
    calls(&mut e, &counter, 0);
    let cached = e.cached_len();
    g.remove_node(a9).unwrap();
    dirty(&e, &g, BTreeSet::new());
    assert_eq!(
        e.evaluate(&g, &r, (a9, "out")),
        Err(EvalError::UnknownNode(a9))
    );
    assert_eq!(e.cached_len(), cached - 1);
    calls(&mut e, &counter, 0);
}

#[test]
fn demand_one_output_evaluates_only_its_upstream() {
    let (r, counter) = registry();
    let mut g = Graph::new();
    let c = add(&mut g, &r, "t.const", 0.0);
    let x1 = add(&mut g, &r, "t.add", 1.0);
    let x2 = add(&mut g, &r, "t.add", 1.0);
    let y1 = add(&mut g, &r, "t.add", 1.0);
    wire(&mut g, c, x1, "in");
    wire(&mut g, x1, x2, "in");
    wire(&mut g, c, y1, "in");
    let mut e = Evaluator::new();
    read(&mut e, &g, &r, x2, 2.0);
    read(&mut e, &g, &r, y1, 1.0);
    calls(&mut e, &counter, 4);
    g.set_param(c, "value", Value::F64(5.0)).unwrap();
    read(&mut e, &g, &r, x2, 7.0);
    calls(&mut e, &counter, 3);
    dirty(&e, &g, BTreeSet::from([y1]));
    read(&mut e, &g, &r, y1, 6.0);
    calls(&mut e, &counter, 1);
    dirty(&e, &g, BTreeSet::new());
    let mut fresh = Evaluator::new();
    read(&mut fresh, &g, &r, x2, 7.0);
    calls(&mut fresh, &counter, 3);
    dirty(&fresh, &g, BTreeSet::from([y1]));
    fresh.clear();
    assert_eq!(fresh.cached_len(), 0);
    dirty(&fresh, &g, BTreeSet::from([c, x1, x2, y1]));
    read(&mut fresh, &g, &r, x2, 7.0);
    calls(&mut fresh, &counter, 3);
}

#[test]
fn diamond_node_evaluated_once() {
    let (r, counter) = registry();
    let mut g = Graph::new();
    let c = add(&mut g, &r, "t.const", 2.0);
    let a1 = add(&mut g, &r, "t.add", 1.0);
    let a2 = add(&mut g, &r, "t.add", 2.0);
    let s = add(&mut g, &r, "t.sum", 4.0);
    wire(&mut g, c, a1, "in");
    wire(&mut g, c, a2, "in");
    wire(&mut g, a1, s, "a");
    wire(&mut g, a2, s, "b");
    let mut e = Evaluator::new();
    read(&mut e, &g, &r, s, 11.0);
    calls(&mut e, &counter, 4);
    g.set_param(c, "value", Value::F64(5.0)).unwrap();
    read(&mut e, &g, &r, s, 17.0);
    calls(&mut e, &counter, 4);
    // The shared source also runs once when cache-skipping cannot hide a revisit.
    e.set_force_full(true);
    read(&mut e, &g, &r, s, 17.0);
    calls(&mut e, &counter, 4);
}

#[test]
fn unchanged_graph_second_eval_zero_calls() {
    let (r, counter) = registry();
    let (mut g, heads, tails) = shape_a(&r);
    let mut e = Evaluator::new();
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 500);
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 0);
    let position = Position { x: 1.0, y: 2.0 };
    g.set_position(heads[0], position).unwrap();
    g.export("tail", tails[0], "out", PortDir::Output).unwrap();
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 0);
}

#[test]
fn eval_chain_20000_no_stack_overflow() {
    let (r, counter) = registry();
    let mut g = Graph::new();
    let head = add(&mut g, &r, "t.const", 0.0);
    let mut tail = head;
    for _ in 0..20_000 {
        let next = add(&mut g, &r, "t.add", 1.0);
        wire(&mut g, tail, next, "in");
        tail = next;
    }
    let mut e = Evaluator::new();
    read(&mut e, &g, &r, tail, 20000.0);
    calls(&mut e, &counter, 20_001);
    assert!(e.dirty_nodes(&g).is_empty());
    g.set_param(head, "value", Value::F64(5.0)).unwrap();
    assert_eq!(e.dirty_nodes(&g).len(), 20_001);
    read(&mut e, &g, &r, tail, 20005.0);
    calls(&mut e, &counter, 20_001);
}

#[test]
fn eval_error_and_panic_do_not_poison_cache() {
    let (r, counter) = registry();
    let mut g = Graph::new();
    let c = add(&mut g, &r, "t.const", 2.0);
    let params = vec![Value::Int(1)];
    let f = g.add_node(&r, "t.fail", params, ORIGIN).unwrap();
    let a = add(&mut g, &r, "t.add", 1.0);
    wire(&mut g, c, f, "in");
    wire(&mut g, f, a, "in");
    let mut e = Evaluator::new();
    let planted = EvalError::Node {
        node: f,
        error: NodeError::Evaluation("planted".into()),
    };
    assert_eq!(e.evaluate(&g, &r, (a, "out")), Err(planted.clone()));
    calls(&mut e, &counter, 2);
    dirty(&e, &g, BTreeSet::from([f, a]));
    assert_eq!(e.cached_len(), 1);
    g.set_param(f, "mode", Value::Int(0)).unwrap();
    read(&mut e, &g, &r, a, 3.0);
    calls(&mut e, &counter, 2);
    for (mode, error) in [
        (
            2,
            EvalError::Panic {
                node: f,
                message: "planted panic".into(),
            },
        ),
        (3, EvalError::BadOutputs { node: f }),
        (4, EvalError::BadOutputs { node: f }),
        (
            5,
            EvalError::Panic {
                node: f,
                message: "owned panic".into(),
            },
        ),
        (
            6,
            EvalError::Panic {
                node: f,
                message: "non-string panic payload".into(),
            },
        ),
        (1, planted),
    ] {
        g.set_param(f, "mode", Value::Int(mode)).unwrap();
        assert_eq!(e.evaluate(&g, &r, (a, "out")), Err(error.clone()));
        assert!(error.to_string().contains(&format!("{f:?}")));
        calls(&mut e, &counter, 1);
        dirty(&e, &g, BTreeSet::from([f, a]));
        assert_eq!(e.cached_len(), 2, "failed result must be removed");
        g.set_param(f, "mode", Value::Int(0)).unwrap();
        read(&mut e, &g, &r, a, 3.0);
        calls(&mut e, &counter, 2);
        dirty(&e, &g, BTreeSet::new());
    }
    let unconnected = add(&mut g, &r, "t.add", 1.0);
    assert_eq!(
        e.evaluate(&g, &r, (unconnected, "out")),
        Err(EvalError::UnconnectedInput {
            node: unconnected,
            port: "in".into(),
        })
    );
    calls(&mut e, &counter, 0);
    let empty = KindRegistry::new();
    // Preserve old entries first, then prove a failed lookup drops the stale one.
    g.set_param(f, "mode", Value::Int(0)).unwrap();
    assert_eq!(
        e.evaluate(&g, &empty, (a, "out")),
        Err(EvalError::Kind {
            node: f,
            error: NodeError::UnknownType("t.fail".into()),
        })
    );
    assert_eq!(e.cached_len(), 2);
    calls(&mut e, &counter, 0);
    read(&mut e, &g, &r, a, 3.0);
    calls(&mut e, &counter, 2);
    let mut fresh = Evaluator::new();
    assert_eq!(
        fresh.evaluate(&g, &empty, (c, "out")),
        Err(EvalError::Kind {
            node: c,
            error: NodeError::UnknownType("t.const".into()),
        })
    );
    calls(&mut fresh, &counter, 0);
    assert_eq!(
        e.evaluate(&g, &r, (a, "nope")),
        Err(EvalError::UnknownOutput {
            node: a,
            port: "nope".into(),
        })
    );
    assert_eq!(e.cached_len(), 3);
    calls(&mut e, &counter, 0);
}

#[test]
fn force_full_recomputes_every_demanded_node() {
    let (r, counter) = registry();
    let (g, _, tails) = shape_a(&r);
    let mut e = Evaluator::new();
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 500);
    e.set_force_full(true);
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 500);
    read(&mut e, &g, &r, tails[3], 12.0);
    calls(&mut e, &counter, 10);
    dirty(&e, &g, g.nodes().map(|(id, _)| id).collect());
    e.set_force_full(false);
    read_tails(&mut e, &g, &r, &tails, false);
    calls(&mut e, &counter, 0);
}
