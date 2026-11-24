use nodegraph::editor::{Editor, EditorInput, Key};
use nodegraph::eval::Evaluator;
use nodegraph::graph::{NodeId, Position};
use nodegraph::kind::KindRegistry;
use nodegraph::kinds::register_all;
use nodegraph::value::{ImageBuf, Value};

const COMMANDS: usize = 11;
const UNDO_LEN: usize = 9;
const MATH_RESULT: f64 = 9.0;
const IMAGE_CHECKSUM: f64 = 32000.0;
const SIDE: u32 = 8;
const FINAL_PIXEL: f32 = 0.5;
const UNDONE_PIXEL: f32 = 0.75;

enum Step {
    Add(&'static str, Vec<Value>, f32, f32),
    Connect(u64, u64, &'static str),
    Disconnect(u64, &'static str),
    Move(u64, f32, f32),
    Set(u64, f64),
    Undo(Option<(u64, f32)>),
    Redo,
    Delete(u64),
}

fn equal<T: PartialEq + std::fmt::Debug>(what: &str, expected: T, got: T) -> Result<(), String> {
    if expected == got {
        Ok(())
    } else {
        Err(format!("{what} expected={expected:?} got={got:?}"))
    }
}

fn pixels(value: Value, expected: f32) -> Result<ImageBuf, String> {
    match value {
        Value::Image(buf) => {
            equal("width", SIDE, buf.width)?;
            equal("height", SIDE, buf.height)?;
            equal("pixel count", (SIDE * SIDE) as usize, buf.pixels.len())?;
            for &pixel in &buf.pixels {
                equal("pixel", expected, pixel)?;
            }
            Ok(buf)
        }
        other => Err(format!("value type expected=Image got={other:?}")),
    }
}

fn session(editor: &mut Editor<'_>, steps: &[Step]) -> Result<usize, String> {
    let mut next_id = 0;
    for (index, step) in steps.iter().enumerate() {
        let result = match step {
            Step::Add(name, params, x, y) => editor
                .add_node(name, params.clone(), Position { x: *x, y: *y })
                .map(|id| {
                    let expected = NodeId(next_id);
                    next_id += 1;
                    (expected, id)
                })
                .map_err(|e| format!("add expected=success got={e:?}"))
                .and_then(|(expected, id)| equal("node id", expected, id)),
            Step::Connect(from, to, port) => editor
                .connect(NodeId(*from), "out", NodeId(*to), port)
                .map_err(|e| format!("connect expected=success got={e:?}")),
            Step::Disconnect(to, port) => editor
                .disconnect(NodeId(*to), port)
                .map_err(|e| format!("disconnect expected=success got={e:?}")),
            Step::Move(id, x, y) => editor
                .move_node(NodeId(*id), Position { x: *x, y: *y })
                .map_err(|e| format!("move expected=success got={e:?}")),
            Step::Set(id, value) => editor
                .set_param(NodeId(*id), "value", Value::F64(*value))
                .map_err(|e| format!("set expected=success got={e:?}")),
            Step::Undo(check) => {
                let undone = editor
                    .undo()
                    .map_err(|e| format!("undo expected=true got={e:?}"))?;
                equal("undo", true, undone)?;
                if let Some((id, pixel)) = check {
                    let value = Evaluator::new()
                        .evaluate(editor.graph(), editor.registry(), (NodeId(*id), "out"))
                        .map_err(|e| format!("undo evaluation expected=success got={e:?}"))?;
                    pixels(value, *pixel)?;
                }
                Ok(())
            }
            Step::Redo => editor
                .redo()
                .map_err(|e| format!("redo expected=true got={e:?}"))
                .and_then(|redone| equal("redo", true, redone)),
            Step::Delete(id) => editor
                .select(NodeId(*id), false)
                .and_then(|()| editor.handle(EditorInput::Key(Key::Delete)))
                .map_err(|e| format!("delete expected=success got={e:?}")),
        };
        result.map_err(|e| format!("step {} {e}", index + 1))?;
    }
    Ok(steps.len())
}

fn run() -> Result<(), String> {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).map_err(|e| format!("registry expected=success got={e:?}"))?;
    let mut math = Editor::new(&registry);
    let math_steps = [
        Step::Add("math.constant", vec![Value::F64(3.0)], 0.0, 0.0),
        Step::Add("math.constant", vec![Value::F64(4.0)], 0.0, 72.0),
        Step::Add("math.add", vec![], 150.0, 0.0),
        Step::Add("math.constant", vec![Value::F64(0.0)], 0.0, 144.0),
        Step::Connect(0, 2, "a"),
        Step::Connect(1, 2, "b"),
        Step::Undo(None),
        Step::Redo,
        Step::Move(2, 165.0, 12.0),
        Step::Set(0, 5.0),
        Step::Delete(3),
    ];
    let commands = session(&mut math, &math_steps)?;
    equal("commands", COMMANDS, commands)?;
    equal("undo length", UNDO_LEN, math.undo_len())?;
    equal(
        "deleted node present",
        false,
        math.graph().node(NodeId(3)).is_some(),
    )?;
    let math_value = Evaluator::new()
        .evaluate(math.graph(), &registry, (NodeId(2), "out"))
        .map_err(|e| format!("evaluation expected=success got={e:?}"))?;
    let math_result = match math_value {
        Value::F64(value) => value,
        other => return Err(format!("value type expected=F64 got={other:?}")),
    };
    equal("math result", MATH_RESULT, math_result)?;
    let mut image = Editor::new(&registry);
    let image_steps = [
        Step::Add(
            "image.solid",
            vec![Value::Int(8), Value::Int(8), Value::F64(0.25)],
            0.0,
            0.0,
        ),
        Step::Add("image.invert", vec![], 150.0, 0.0),
        Step::Add(
            "image.gradient",
            vec![Value::Int(8), Value::Int(8)],
            0.0,
            72.0,
        ),
        Step::Connect(2, 1, "a"),
        Step::Disconnect(1, "a"),
        Step::Connect(0, 1, "a"),
        Step::Set(0, 0.5),
        Step::Undo(Some((1, UNDONE_PIXEL))),
        Step::Redo,
        Step::Move(1, 165.0, 12.0),
        Step::Delete(2),
    ];
    equal("commands", COMMANDS, session(&mut image, &image_steps)?)?;
    equal("undo length", UNDO_LEN, image.undo_len())?;
    equal(
        "deleted node present",
        false,
        image.graph().node(NodeId(2)).is_some(),
    )?;
    let value = Evaluator::new()
        .evaluate(image.graph(), &registry, (NodeId(1), "out"))
        .map_err(|e| format!("evaluation expected=success got={e:?}"))?;
    let buf = pixels(value, FINAL_PIXEL)?;
    let checksum = buf.pixels.iter().map(|&v| f64::from(v)).sum::<f64>() * 1000.0;
    equal("checksum", IMAGE_CHECKSUM, checksum)?;
    println!("share_editor: ok kinds=math,image commands={commands} math_result={math_result} image_checksum={checksum}");
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("share_editor: FAIL {error}");
        std::process::exit(1);
    }
}
