use std::cell::Cell;
use std::collections::BTreeSet;
use std::rc::Rc;

use nodegraph::editor::{EditError, Editor, EditorInput, Key, NODE_HEIGHT, NODE_WIDTH};
use nodegraph::eval::{EvalError, Evaluator};
use nodegraph::format;
use nodegraph::graph::{Graph, GraphError, NodeId, PortDir, Position};
use nodegraph::history::{Command, History, MAX_HISTORY};
use nodegraph::kind::KindRegistry;
use nodegraph::kinds::register_all;
use nodegraph::value::{ImageBuf, Value};

fn registry() -> KindRegistry {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).unwrap();
    registry
}

fn pos(x: f32, y: f32) -> Position {
    Position { x, y }
}

fn solid(value: f64) -> Vec<Value> {
    vec![Value::Int(8), Value::Int(8), Value::F64(value)]
}

fn pixels(value: f32) -> Value {
    Value::Image(ImageBuf {
        width: 8,
        height: 8,
        pixels: vec![value; 64],
    })
}

fn selected_exist(editor: &Editor<'_>) {
    for id in editor.selection() {
        assert!(editor.graph().node(*id).is_some(), "selected={id:?}");
    }
}

fn same_graph(actual: &Graph, expected: &Graph, seed: u64) {
    assert_eq!(actual, expected, "seed={seed}");
    let text = format::write(expected).unwrap();
    assert_eq!(format::write(actual).unwrap(), text, "seed={seed}");
}

fn add(editor: &mut Editor<'_>, name: &str, params: Vec<Value>, at: Position) -> NodeId {
    editor.add_node(name, params, at).unwrap()
}

fn position(editor: &Editor<'_>, id: NodeId) -> Position {
    editor.graph().node(id).unwrap().position
}

enum Step {
    Add(&'static str, Vec<Value>),
    Connect(usize, usize, &'static str),
    Move(usize),
    Param(usize, f64),
    Undo,
    Redo,
    Check(usize, Value),
}

fn session(editor: &mut Editor<'_>, steps: &[Step]) -> Vec<NodeId> {
    let mut ids = Vec::new();
    let mut depth = 0;
    for step in steps {
        match step {
            Step::Add(name, params) => {
                ids.push(add(editor, name, params.clone(), pos(0.0, 0.0)));
                depth += 1;
            }
            Step::Connect(from, to, port) => {
                editor.connect(ids[*from], "out", ids[*to], port).unwrap();
                depth += 1;
            }
            Step::Move(node) => {
                editor.move_node(ids[*node], pos(70.0, 80.0)).unwrap();
                depth += 1;
            }
            Step::Param(node, value) => {
                let result = editor.set_param(ids[*node], "value", Value::F64(*value));
                result.unwrap();
                depth += 1;
            }
            Step::Undo => {
                assert_eq!(editor.undo(), Ok(true));
                depth -= 1;
            }
            Step::Redo => {
                assert_eq!(editor.redo(), Ok(true));
                depth += 1;
            }
            Step::Check(node, expected) => {
                let mut ev = Evaluator::new();
                let result = ev.evaluate(editor.graph(), editor.registry(), (ids[*node], "out"));
                assert_eq!(result, Ok(expected.clone()));
            }
        }
        assert_eq!(editor.undo_len(), depth);
        selected_exist(editor);
    }
    ids
}

#[test]
fn one_editor_two_kinds() {
    let registry = registry();
    let mut scalar = Editor::new(&registry);
    let steps = [
        Step::Add("math.constant", vec![Value::F64(3.0)]),
        Step::Add("math.constant", vec![Value::F64(4.0)]),
        Step::Add("math.add", vec![]),
        Step::Connect(0, 2, "a"),
        Step::Connect(1, 2, "b"),
        Step::Undo,
        Step::Redo,
        Step::Move(2),
        Step::Param(0, 5.0),
        Step::Check(2, Value::F64(9.0)),
    ];
    session(&mut scalar, &steps);
    assert_eq!(scalar.undo_len(), 7);
    let mut raster = Editor::new(&registry);
    let steps = [
        Step::Add("image.solid", solid(0.25)),
        Step::Add("image.invert", vec![]),
        Step::Connect(0, 1, "a"),
        Step::Move(1),
        Step::Param(0, 0.5),
        Step::Undo,
        Step::Check(1, pixels(0.75)),
        Step::Redo,
        Step::Check(1, pixels(0.5)),
    ];
    let ids = session(&mut raster, &steps);
    assert_eq!(raster.undo_len(), 5);
    let mut ev = Evaluator::new();
    let value = ev.evaluate(raster.graph(), &registry, (ids[1], "out"));
    let Value::Image(result) = value.unwrap() else {
        panic!("expected raster output");
    };
    let sum: f64 = result.pixels.iter().map(|&p| f64::from(p)).sum();
    assert_eq!(sum * 1000.0, 32000.0);
}

struct Rng(u64);

impl Rng {
    fn draw(&mut self, count: usize) -> usize {
        assert!(count > 0);
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) as usize) % count
    }
}

const SCALAR: &[&str] = &[
    "math.constant",
    "math.add",
    "math.subtract",
    "math.negate",
    "math.min",
    "math.max",
];
const RASTER: &[&str] = &[
    "image.solid",
    "image.gradient",
    "image.invert",
    "image.multiply",
    "image.threshold",
    "image.blur",
    "image.blend",
];

fn params(name: &str, rng: &mut Rng) -> Vec<Value> {
    match name {
        "math.constant" => vec![Value::F64(rng.draw(21) as f64 - 10.0)],
        "image.solid" => solid(rng.draw(5) as f64 * 0.25),
        "image.gradient" => vec![Value::Int(8), Value::Int(8)],
        "image.threshold" => vec![Value::F64((rng.draw(3) + 1) as f64 * 0.25)],
        "image.blend" => vec![Value::F64(rng.draw(3) as f64 * 0.5)],
        _ => vec![],
    }
}

fn start_graph(registry: &KindRegistry, raster: bool) -> Graph {
    let mut graph = Graph::new();
    let mut rng = Rng(17);
    let source = if raster { RASTER[0] } else { SCALAR[0] };
    let binary = if raster { "image.multiply" } else { "math.add" };
    let unary = if raster { "image.invert" } else { "math.negate" };
    let a = graph.add_node(registry, source, params(source, &mut rng), pos(0.0, 0.0));
    let b = graph.add_node(registry, source, params(source, &mut rng), pos(200.0, 0.0));
    let c = graph
        .add_node(registry, binary, vec![], pos(0.0, 100.0))
        .unwrap();
    graph
        .add_node(registry, unary, vec![], pos(200.0, 100.0))
        .unwrap();
    graph.connect(a.unwrap(), "out", c, "a").unwrap();
    graph.connect(b.unwrap(), "out", c, "b").unwrap();
    graph
}

// Returns operation, success, and whether removal exercised outgoing edges.
fn random_command(editor: &mut Editor<'_>, raster: bool, rng: &mut Rng) -> (usize, bool, bool) {
    let before = editor.graph().clone();
    let lengths = (editor.undo_len(), editor.redo_len());
    let ids: Vec<_> = before.nodes().map(|(id, _)| id).collect();
    let op = if ids.is_empty() { 0 } else { rng.draw(6) };
    let mut outgoing = false;
    let result = if op == 0 {
        let types = if raster { RASTER } else { SCALAR };
        let name = types[rng.draw(types.len())];
        editor
            .add_node(name, params(name, rng), pos(0.0, 0.0))
            .map(|_| ())
    } else {
        let id = ids[rng.draw(ids.len())];
        editor.select(id, true).unwrap();
        let node = before.node(id).unwrap();
        let port = if node.inputs.is_empty() {
            "a"
        } else {
            &node.inputs[rng.draw(node.inputs.len())].name
        };
        match op {
            1 => {
                outgoing = before.edges().any(|edge| edge.from == id);
                editor.remove_node(id)
            }
            2 => editor.connect(ids[rng.draw(ids.len())], "out", id, port),
            3 => editor.disconnect(id, port),
            4 => {
                let name = node.params.iter().find(|(_, v)| matches!(v, Value::F64(_)));
                let name = name.map_or("value", |(name, _)| name.as_str());
                let value = match name {
                    "level" => (rng.draw(3) + 1) as f64 * 0.25,
                    "factor" => rng.draw(3) as f64 * 0.5,
                    _ if raster => rng.draw(5) as f64 * 0.25,
                    _ => rng.draw(21) as f64 - 10.0,
                };
                editor.set_param(id, name, Value::F64(value))
            }
            _ => editor.move_node(id, pos(rng.draw(1000) as f32, rng.draw(1000) as f32)),
        }
    };
    if result.is_err() {
        assert_eq!(editor.graph(), &before);
        assert_eq!(editor.graph().version(), before.version());
        assert_eq!((editor.undo_len(), editor.redo_len()), lengths);
    }
    selected_exist(editor);
    (op, result.is_ok(), outgoing)
}

#[test]
fn undo_all_returns_to_start_1000_edits() {
    let registry = registry();
    for raster in [false, true] {
        let seed = if raster { 0x7219 } else { 0x9153 };
        let mut rng = Rng(seed);
        let start = start_graph(&registry, raster);
        let mut editor = Editor::with_graph(&registry, start.clone());
        let (mut successes, mut attempts, mut removed_readers) = (0, 0, 0);
        let mut counts = [0; 6];
        while successes < 1000 && attempts < 20_000 {
            attempts += 1;
            let (op, ok, outgoing) = random_command(&mut editor, raster, &mut rng);
            if ok {
                successes += 1;
                counts[op] += 1;
                removed_readers += usize::from(outgoing);
            }
        }
        println!("raster={raster} seed={seed} attempts={attempts} successes={successes}");
        assert_eq!(successes, 1000, "seed={seed} attempts={attempts}");
        assert!(counts.iter().all(|&n| n > 0), "seed={seed} counts={counts:?}");
        assert!(removed_readers > 0, "seed={seed}");
        let end = editor.graph().clone();
        for step in 0..1000 {
            assert_eq!(editor.undo(), Ok(true), "seed={seed} undo={step}");
            selected_exist(&editor);
        }
        assert_eq!(editor.undo(), Ok(false), "seed={seed}");
        same_graph(editor.graph(), &start, seed);
        for step in 0..1000 {
            assert_eq!(editor.redo(), Ok(true), "seed={seed} redo={step}");
            selected_exist(&editor);
        }
        assert_eq!(editor.redo(), Ok(false), "seed={seed}");
        same_graph(editor.graph(), &end, seed);
    }
}

#[test]
fn undo_redo_keep_cache_correct() {
    let registry = registry();
    for raster in [false, true] {
        let seed = if raster { 0x8719 } else { 0x1234 };
        let mut rng = Rng(seed);
        let mut editor = Editor::with_graph(&registry, start_graph(&registry, raster));
        let mut ev = Evaluator::new();
        let mut demands = 0;
        for step in 0..300 {
            match rng.draw(3) {
                0 => {
                    random_command(&mut editor, raster, &mut rng);
                }
                1 => {
                    editor.undo().unwrap();
                }
                _ => {
                    editor.redo().unwrap();
                }
            }
            selected_exist(&editor);
            let dirty = ev.dirty_nodes(editor.graph());
            let calls = ev.stats().calls;
            let mut expected_calls = 0;
            for (id, _) in editor.graph().nodes() {
                let fresh = Evaluator::new().evaluate(editor.graph(), &registry, (id, "out"));
                let valid =
                    fresh.is_ok() || matches!(&fresh, Err(EvalError::UnconnectedInput { .. }));
                assert!(valid, "seed={seed} step={step} result={fresh:?}");
                if dirty.contains(&id) && fresh.is_ok() {
                    expected_calls += 1;
                }
                let actual = ev.evaluate(editor.graph(), &registry, (id, "out"));
                assert_eq!(actual, fresh, "seed={seed} step={step} node={id:?}");
                demands += 1;
            }
            assert_eq!(
                ev.stats().calls - calls,
                expected_calls,
                "seed={seed} step={step}"
            );
        }
        assert!(demands > 0, "seed={seed}");
    }
}

struct Push {
    value: i32,
    fail_apply: Rc<Cell<bool>>,
    fail_undo: Rc<Cell<bool>>,
}

impl Command<Vec<i32>, &'static str> for Push {
    fn apply(&mut self, target: &mut Vec<i32>) -> Result<(), &'static str> {
        if self.fail_apply.get() {
            return Err("apply refused");
        }
        target.push(self.value);
        Ok(())
    }
    fn undo(&mut self, target: &mut Vec<i32>) -> Result<(), &'static str> {
        if self.fail_undo.get() {
            return Err("undo refused");
        }
        assert_eq!(target.pop(), Some(self.value));
        Ok(())
    }
}

fn push(value: i32) -> Box<Push> {
    Box::new(Push {
        value,
        fail_apply: Rc::new(Cell::new(false)),
        fail_undo: Rc::new(Cell::new(false)),
    })
}

#[test]
fn history_cap_drops_oldest_and_failed_commands_leave_no_entry() {
    let mut history = History::with_cap(3);
    let mut target = Vec::new();
    assert_eq!(History::<Vec<i32>, &'static str>::new().undo_len(), 0);
    assert_eq!(MAX_HISTORY, 10_000);
    for value in 0..5 {
        history.run(&mut target, push(value)).unwrap();
    }
    assert_eq!(history.undo_len(), 3);
    for _ in 0..3 {
        assert_eq!(history.undo(&mut target), Ok(true));
    }
    assert_eq!(target, vec![0, 1]);
    assert_eq!(history.undo(&mut target), Ok(false));
    let failed = push(9);
    failed.fail_apply.set(true);
    assert_eq!(history.run(&mut target, failed), Err("apply refused"));
    assert_eq!(target, vec![0, 1]);
    assert_eq!((history.undo_len(), history.redo_len()), (0, 3));
    history.run(&mut target, push(7)).unwrap();
    assert_eq!(history.redo_len(), 0);
    assert_eq!(history.redo(&mut target), Ok(false));
    let command = push(8);
    let fail_apply = command.fail_apply.clone();
    let fail_undo = command.fail_undo.clone();
    history.run(&mut target, command).unwrap();
    fail_undo.set(true);
    assert_eq!(history.undo(&mut target), Err("undo refused"));
    assert_eq!(target, vec![0, 1, 7, 8]);
    assert_eq!((history.undo_len(), history.redo_len()), (2, 0));
    fail_undo.set(false);
    assert_eq!(history.undo(&mut target), Ok(true));
    fail_apply.set(true);
    assert_eq!(history.redo(&mut target), Err("apply refused"));
    assert_eq!(target, vec![0, 1, 7]);
    assert_eq!((history.undo_len(), history.redo_len()), (1, 1));
    fail_apply.set(false);
    assert_eq!(history.redo(&mut target), Ok(true));
    let mut zero = History::with_cap(0);
    zero.run(&mut target, push(9)).unwrap();
    zero.run(&mut target, push(10)).unwrap();
    assert_eq!(zero.undo_len(), 1);
    assert_eq!(zero.undo(&mut target), Ok(true));
    assert_eq!(target, vec![0, 1, 7, 8, 9]);
}

fn point(x: f32, y: f32) -> EditorInput {
    EditorInput::Pointer { x, y }
}

fn handle(editor: &mut Editor<'_>, inputs: &[EditorInput]) {
    for &input in inputs {
        editor.handle(input).unwrap();
        selected_exist(editor);
    }
}

fn group_export(editor: &Editor<'_>, id: NodeId, prefix: &str, port: &str, dir: PortDir) {
    let group = editor.group_selection().unwrap();
    assert_eq!(group.node_count(), 1);
    assert_eq!(group.node(id), editor.graph().node(id));
    assert_eq!(group.exports().len(), 1);
    let export = &group.exports()[&format!("{prefix}_{}_{port}", id.0)];
    assert_eq!((export.node, export.port.as_str(), export.dir), (id, port, dir));
}

#[test]
fn editor_input_selects_drags_and_deletes() {
    use EditorInput::{ButtonDown, ButtonUp};
    let registry = registry();
    let mut editor = Editor::new(&registry);
    let params = vec![Value::F64(1.0)];
    let first = add(&mut editor, "math.constant", params, pos(0.0, 0.0));
    let second = add(&mut editor, "math.negate", vec![], pos(200.0, 0.0));
    handle(&mut editor, &[point(210.0, 10.0), ButtonDown]);
    assert_eq!(editor.selection(), &BTreeSet::from([second]));
    handle(&mut editor, &[point(260.0, 40.0), ButtonUp]);
    assert_eq!(position(&editor, second), pos(250.0, 30.0));
    assert_eq!(editor.undo_len(), 3);
    handle(&mut editor, &[EditorInput::Key(Key::Undo)]);
    assert_eq!(position(&editor, second), pos(200.0, 0.0));
    handle(&mut editor, &[point(-200.0, -200.0), ButtonDown]);
    assert!(editor.selection().is_empty());
    assert_eq!(editor.group_selection(), Err(EditError::EmptySelection));
    editor.select(first, false).unwrap();
    handle(&mut editor, &[EditorInput::Key(Key::Delete)]);
    assert!(editor.graph().node(first).is_none());
    handle(&mut editor, &[EditorInput::Key(Key::Undo)]);
    assert!(editor.graph().node(first).is_some());
    assert!(editor.selection().is_empty());
    editor.connect(first, "out", second, "a").unwrap();
    let original = editor.graph().clone();
    let lengths = (editor.undo_len(), editor.redo_len());
    editor.select(first, false).unwrap();
    group_export(&editor, first, "out", "out", PortDir::Output);
    editor.select(second, false).unwrap();
    group_export(&editor, second, "in", "a", PortDir::Input);
    editor.select(first, true).unwrap();
    assert_eq!(editor.group_selection().unwrap(), original);
    assert_eq!(editor.graph(), &original);
    assert_eq!((editor.undo_len(), editor.redo_len()), lengths);
    editor.remove_node(first).unwrap();
    assert!(editor.graph().source_of(second, "a").is_none());
    selected_exist(&editor);
    assert_eq!(editor.undo(), Ok(true));
    assert_eq!(editor.graph(), &original);
    assert_eq!(editor.redo(), Ok(true));
    selected_exist(&editor);
    assert_eq!(editor.undo(), Ok(true));
    editor.disconnect(second, "a").unwrap();
    editor.select(second, false).unwrap();
    group_export(&editor, second, "in", "a", PortDir::Input);
    let missing = NodeId(u64::MAX);
    let error = Err(EditError::Graph(GraphError::UnknownNode(missing)));
    assert_eq!(editor.select(missing, false), error);
    assert_eq!(editor.selection(), &BTreeSet::from([second]));
    assert_eq!(editor.remove_node(missing), error);
    let before = editor.graph().clone();
    let depth = editor.undo_len();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let result = editor.move_node(first, pos(bad, 0.0));
        assert!(matches!(result, Err(EditError::Graph(GraphError::NonFinite(_)))));
        let result = editor.handle(point(0.0, bad));
        assert!(matches!(result, Err(EditError::Graph(GraphError::NonFinite(_)))));
    }
    assert_eq!(editor.graph(), &before);
    assert_eq!(editor.undo_len(), depth);
    editor.move_node(second, pos(0.0, 0.0)).unwrap();
    let depth = editor.undo_len();
    handle(&mut editor, &[point(0.0, 0.0), ButtonDown, ButtonUp]);
    assert_eq!(editor.selection(), &BTreeSet::from([second]));
    assert_eq!(editor.undo_len(), depth);
    for (x, y) in [(NODE_WIDTH, 0.0), (0.0, NODE_HEIGHT)] {
        handle(&mut editor, &[point(x, y), ButtonDown]);
        assert!(editor.selection().is_empty());
    }
    // Exact independent identity: when start equals anchor, result is pointer.
    for c in [f32::from_bits(1), f32::MIN_POSITIVE, f32::MAX, f32::MIN] {
        editor.move_node(second, pos(c, c)).unwrap();
        let depth = editor.undo_len();
        let tiny = f32::from_bits(2);
        let inputs = [point(c, c), ButtonDown, point(tiny, tiny), ButtonUp];
        handle(&mut editor, &inputs);
        assert_eq!(position(&editor, second), pos(tiny, tiny));
        assert_eq!(editor.undo_len(), depth + 1);
    }
    // Near cancellation: unchanged x must preserve its subnormal start.
    let tiny = f32::from_bits(1);
    editor.move_node(second, pos(tiny, 0.0)).unwrap();
    let inputs = [point(64.0, 0.0), ButtonDown, point(64.0, tiny), ButtonUp];
    handle(&mut editor, &inputs);
    assert_eq!(position(&editor, second), pos(tiny, tiny));
    editor.select(first, true).unwrap();
    let depth = editor.undo_len();
    handle(&mut editor, &[EditorInput::Key(Key::Delete)]);
    assert_eq!(editor.graph().node_count(), 0);
    assert_eq!(editor.undo_len(), depth + 2);
    handle(&mut editor, &[EditorInput::Key(Key::Undo)]);
    assert!(editor.graph().node(second).is_some());
    assert!(editor.graph().node(first).is_none());
    handle(&mut editor, &[EditorInput::Key(Key::Redo)]);
    assert_eq!(editor.graph().node_count(), 0);
    let mut exported = start_graph(&registry, false);
    let id = exported.nodes().next().unwrap().0;
    let last = exported.nodes().last().unwrap().0;
    exported.export("kept", id, "out", PortDir::Output).unwrap();
    let mut editor = Editor::with_graph(&registry, exported.clone());
    editor.select(id, false).unwrap();
    editor.select(last, true).unwrap();
    let error = GraphError::NodeExported {
        node: id,
        name: "kept".into(),
    };
    assert_eq!(editor.handle(EditorInput::Key(Key::Delete)), Err(error.into()));
    assert_eq!(editor.graph(), &exported);
    assert_eq!(editor.undo_len(), 0);
    selected_exist(&editor);
    handle(&mut editor, &[EditorInput::Key(Key::ClearSelection)]);
    assert!(editor.selection().is_empty());
}
