use std::collections::BTreeSet;

use nodegraph::editor::Editor;
use nodegraph::eval::Evaluator;
use nodegraph::graph::{Graph, NodeId, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::kinds::register_all;
use nodegraph::value::{Value, ValueType};
use nodegraph::view::{DrawList, ViewError, MAX_GRID_SIDE};

const MATH_GRID: &str = include_str!("fixtures/view_math.txt");
const IMAGE_GRID: &str = include_str!("fixtures/view_image.txt");

fn registry() -> KindRegistry {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).unwrap();
    registry
}

fn pos(x: f32, y: f32) -> Position {
    Position { x, y }
}

fn build<'r>(
    registry: &'r KindRegistry,
    descriptions: [(&str, Vec<Value>); 4],
    edited: NodeId,
    value: f64,
    automatic: bool,
) -> (Editor<'r>, Evaluator) {
    let mut editor = Editor::new(registry);
    let placed = [
        pos(0.0, 0.0),
        pos(0.0, 72.0),
        pos(150.0, 0.0),
        pos(300.0, 0.0),
    ];
    let mut ids = Vec::new();
    for ((name, params), at) in descriptions.into_iter().zip(placed) {
        ids.push(
            editor
                .add_node(name, params, if automatic { pos(0.0, 0.0) } else { at })
                .unwrap(),
        );
    }
    for (from, to, port) in [(0, 2, "a"), (1, 2, "b"), (2, 3, "a")] {
        editor.connect(ids[from], "out", ids[to], port).unwrap();
    }
    let mut evaluator = Evaluator::new();
    evaluator
        .evaluate(editor.graph(), registry, (ids[3], "out"))
        .unwrap();
    editor.set_param(edited, "value", Value::F64(value)).unwrap();
    (editor, evaluator)
}

fn math(registry: &KindRegistry, automatic: bool) -> (Editor<'_>, Evaluator) {
    build(
        registry,
        [
            ("math.constant", vec![Value::F64(3.0)]),
            ("math.constant", vec![Value::F64(4.0)]),
            ("math.add", vec![]),
            ("math.negate", vec![]),
        ],
        NodeId(1),
        5.0,
        automatic,
    )
}

fn image(registry: &KindRegistry) -> (Editor<'_>, Evaluator) {
    build(
        registry,
        [
            (
                "image.solid",
                vec![Value::Int(8), Value::Int(8), Value::F64(0.25)],
            ),
            ("image.gradient", vec![Value::Int(8), Value::Int(8)]),
            ("image.multiply", vec![]),
            ("image.invert", vec![]),
        ],
        NodeId(0),
        0.5,
        false,
    )
}

#[test]
fn view_matches_golden_math_and_image() {
    let registry = registry();
    for ((editor, evaluator), expected) in [
        (math(&registry, false), MATH_GRID),
        (image(&registry), IMAGE_GRID),
    ] {
        let actual = DrawList::from_editor(&editor, &evaluator.dirty_nodes(editor.graph()))
            .render()
            .unwrap();
        assert_eq!(actual, expected, "actual:\n{actual}\nexpected:\n{expected}");
    }
}

#[test]
fn view_marks_dirty_nodes() {
    let registry = registry();
    for ((mut editor, mut evaluator), edited, expected) in [
        (
            math(&registry, false),
            NodeId(1),
            BTreeSet::from([NodeId(1), NodeId(2), NodeId(3)]),
        ),
        (
            image(&registry),
            NodeId(0),
            BTreeSet::from([NodeId(0), NodeId(2), NodeId(3)]),
        ),
    ] {
        let dirty = evaluator.dirty_nodes(editor.graph());
        let list = DrawList::from_editor(&editor, &dirty);
        let marked: BTreeSet<_> = list
            .boxes
            .iter()
            .filter(|b| b.dirty)
            .map(|b| b.node)
            .collect();
        assert_eq!(marked, expected);
        let mut downstream = editor.graph().descendants(edited).unwrap();
        downstream.insert(edited);
        assert_eq!(marked, downstream);
        let text = list.render().unwrap();
        let lines: Vec<_> = text.lines().collect();
        for node in &list.boxes {
            assert_eq!(
                lines[(node.row + 1) as usize].as_bytes()[(node.col + 1) as usize],
                if expected.contains(&node.node) {
                    b'*'
                } else {
                    b' '
                },
                "marker for {:?}",
                node.node
            );
        }
        evaluator
            .evaluate(editor.graph(), &registry, (NodeId(3), "out"))
            .unwrap();
        assert!(evaluator.dirty_nodes(editor.graph()).is_empty());
        let before = DrawList::from_editor(&editor, &evaluator.dirty_nodes(editor.graph()));
        assert!(!before.render().unwrap().contains('*'));
        editor.move_node(NodeId(0), pos(0.0, 24.0)).unwrap();
        assert!(evaluator.dirty_nodes(editor.graph()).is_empty());
        let after = DrawList::from_editor(&editor, &evaluator.dirty_nodes(editor.graph()));
        assert!(!after.render().unwrap().contains('*'));
        let mut expected = before;
        expected.boxes[0].row = 2;
        for wire in &mut expected.wires {
            if wire.from == NodeId(0) {
                wire.start.1 += 2;
            }
        }
        assert_eq!(after, expected);
    }
}

#[test]
fn view_auto_layout_matches_placed_layout() {
    let registry = registry();
    let (mut editor, evaluator) = math(&registry, true);
    let dirty = evaluator.dirty_nodes(editor.graph());
    assert_eq!(
        DrawList::from_editor(&editor, &dirty).render().unwrap(),
        MATH_GRID
    );
    editor.move_node(NodeId(3), pos(300.0, 0.0)).unwrap();
    let list = DrawList::from_editor(&editor, &dirty);
    assert_eq!((list.boxes[0].col, list.boxes[0].row), (0, 0));
    assert_eq!((list.boxes[1].col, list.boxes[1].row), (0, 0));
    assert_eq!((list.boxes[2].col, list.boxes[2].row), (0, 0));
    assert_ne!(list.render().unwrap(), MATH_GRID);
}

struct NamedPorts {
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
}

impl NodeKind for NamedPorts {
    fn type_name(&self) -> &str {
        "t.very_long_type_name"
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
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        if params.is_empty() && matches!(inputs, [Value::F64(_), Value::F64(_), Value::F64(_)]) {
            Ok(vec![inputs[0].clone()])
        } else {
            Err(NodeError::Evaluation(
                "expected three F64 inputs and no parameters".into(),
            ))
        }
    }
}

#[test]
fn view_draw_list_names_ports_and_wires() {
    let mut registry = registry();
    let port = |name: &str| PortDesc {
        name: name.into(),
        value_type: ValueType::F64,
    };
    registry
        .add(Box::new(NamedPorts {
            inputs: vec![port("abcdefghij"), port("b"), port("ünïcode")],
            outputs: vec![port("out")],
        }))
        .unwrap();
    let mut editor = Editor::new(&registry);
    let target = editor
        .add_node("t.very_long_type_name", vec![], pos(150.0, 0.0))
        .unwrap();
    let source = editor
        .add_node("math.constant", vec![Value::F64(1.0)], pos(0.0, 0.0))
        .unwrap();
    editor.connect(source, "out", target, "abcdefghij").unwrap();
    editor.connect(source, "out", target, "ünïcode").unwrap();
    let list = DrawList::from_editor(&editor, &BTreeSet::new());
    assert_eq!(list.boxes[0].node, NodeId(0));
    assert_eq!(list.boxes[0].rows, 6);
    assert_eq!(list.boxes[0].title, "0:very_long_t");
    assert_eq!(list.boxes[0].inputs, ["abcdef", "b", "?n?cod"]);
    assert_eq!(list.boxes[0].outputs, ["out"]);
    assert_eq!(list.wires.len(), 2);
    for (wire, (port, end)) in list
        .wires
        .iter()
        .zip([("abcdefghij", (19, 2)), ("ünïcode", (19, 4))])
    {
        assert_eq!(wire.from, source);
        assert_eq!(wire.to, target);
        assert_eq!(wire.from_port, "out");
        assert_eq!(wire.to_port, port);
        assert_eq!(wire.start, (16, 2));
        assert_eq!(wire.end, end);
    }
    assert_eq!(
        DrawList::from_graph(&Graph::new(), &BTreeSet::new()).render(),
        Ok(String::new())
    );

    // Negative subnormal coordinates floor to -1, not zero. Finite extremes
    // saturate; these expected cell values are independent integer references.
    for (x, y, expected) in [
        (-f32::from_bits(1), -f32::from_bits(1), (-1, -1)),
        (f32::from_bits(1), f32::from_bits(1), (0, 0)),
        (-7.5, -12.0, (-1, -1)),
        (f32::MAX, -f32::MAX, (i64::MAX, i64::MIN)),
    ] {
        editor.move_node(target, pos(x, y)).unwrap();
        let list = DrawList::from_editor(&editor, &BTreeSet::new());
        assert_eq!((list.boxes[0].col, list.boxes[0].row), expected);
    }
}

#[test]
fn view_refuses_grid_over_limit() {
    let registry = registry();
    let mut editor = Editor::new(&registry);
    let far = editor
        .add_node("math.constant", vec![Value::F64(1.0)], pos(30000.0, 0.0))
        .unwrap();
    editor
        .add_node("math.constant", vec![Value::F64(2.0)], pos(0.0, 0.0))
        .unwrap();
    assert!(
        matches!(
            DrawList::from_editor(&editor, &BTreeSet::new()).render(),
            Err(ViewError::TooLarge { width, .. }) if width > MAX_GRID_SIDE as u64
        ),
        "oversize guard must reject before testing saturated coordinates"
    );
    editor.move_node(far, pos(3.0e38, -3.0e38)).unwrap();
    assert!(matches!(
        DrawList::from_editor(&editor, &BTreeSet::new()).render(),
        Err(ViewError::TooLarge { .. })
    ));

    // Exercise the public lower-level renderer independently of coordinate
    // conversion, including widened rectangle and bend offsets.
    let mut list = DrawList::from_editor(&editor, &BTreeSet::new());
    list.boxes.clear();
    list.wires = vec![nodegraph::view::Wire {
        from: NodeId(1),
        from_port: "out".into(),
        to: NodeId(2),
        to_port: "a".into(),
        start: (i64::MAX, i64::MIN),
        end: (i64::MIN, i64::MAX),
    }];
    assert_eq!(
        list.render(),
        Err(ViewError::TooLarge {
            width: u64::MAX,
            height: u64::MAX,
        })
    );
    list.wires.clear();
    let mut node = DrawList::from_editor(&editor, &BTreeSet::new()).boxes.remove(0);
    node.col = i64::MAX;
    node.row = i64::MAX;
    node.rows = i64::MAX;
    list.boxes.push(node);
    assert_eq!(
        list.render(),
        Err(ViewError::TooLarge {
            width: 16,
            height: i64::MAX as u64,
        })
    );
}
