use nodegraph::editor::Editor;
use nodegraph::eval::Evaluator;
use nodegraph::graph::Position;
use nodegraph::kind::KindRegistry;
use nodegraph::kinds::register_all;
use nodegraph::value::Value;
use nodegraph::view::DrawList;

fn run(kind: &str) -> Result<(), String> {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).map_err(|e| format!("{e:?}"))?;
    let mut editor = Editor::new(&registry);
    let descriptions = if kind == "math" {
        [
            ("math.constant", vec![Value::F64(3.0)]),
            ("math.constant", vec![Value::F64(4.0)]),
            ("math.add", vec![]),
            ("math.negate", vec![]),
        ]
    } else {
        [
            (
                "image.solid",
                vec![Value::Int(8), Value::Int(8), Value::F64(0.25)],
            ),
            ("image.gradient", vec![Value::Int(8), Value::Int(8)]),
            ("image.multiply", vec![]),
            ("image.invert", vec![]),
        ]
    };
    let positions = [(0.0, 0.0), (0.0, 72.0), (150.0, 0.0), (300.0, 0.0)];
    let mut ids = Vec::new();
    for ((name, params), (x, y)) in descriptions.into_iter().zip(positions) {
        ids.push(
            editor
                .add_node(name, params, Position { x, y })
                .map_err(|e| format!("{e:?}"))?,
        );
    }
    for (from, to, port) in [(0, 2, "a"), (1, 2, "b"), (2, 3, "a")] {
        editor
            .connect(ids[from], "out", ids[to], port)
            .map_err(|e| format!("{e:?}"))?;
    }
    let mut evaluator = Evaluator::new();
    evaluator
        .evaluate(editor.graph(), &registry, (ids[3], "out"))
        .map_err(|e| format!("{e:?}"))?;
    let (edited, value) = if kind == "math" {
        (ids[1], 5.0)
    } else {
        (ids[0], 0.5)
    };
    editor
        .set_param(edited, "value", Value::F64(value))
        .map_err(|e| format!("{e:?}"))?;
    let dirty = evaluator.dirty_nodes(editor.graph());
    let text = DrawList::from_editor(&editor, &dirty)
        .render()
        .map_err(|e| format!("{e:?}"))?;
    print!("{text}");
    println!("editor_view: ok kind={kind} nodes=4 dirty=3");
    Ok(())
}

fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let kind = match args.as_slice() {
        [flag, kind] if flag == "--kind" => kind.to_str(),
        _ => None,
    };
    let kind = match kind {
        Some(kind @ ("math" | "image")) => kind,
        _ => {
            eprintln!("usage: editor_view --kind math|image");
            std::process::exit(2);
        }
    };
    if let Err(error) = run(kind) {
        eprintln!("editor_view: FAIL {error}");
        std::process::exit(1);
    }
}
