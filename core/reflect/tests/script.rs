use reflect::change::{Change, ChangeSink};
use reflect::desc::Reflect;
use reflect::limits::Limits;
use reflect::path::{AccessError, PathError};
use reflect::sample::{DriveMode, Vehicle, Wheel};
use reflect::script::{run_script, ScriptError as E, ScriptFailure as F, ScriptReport};
use reflect::text::{load_text, save_text};
use reflect::value::Value as V;

const L: Limits = Limits {
    max_path_length: 256,
    max_text_file_bytes: 1048576,
    max_list_length: 4096,
    max_nesting_depth: 16,
};

fn object() -> Vehicle {
    Vehicle {
        name: "car".into(),
        mass_kg: 1200.0,
        position: [0.0, 0.0, 0.0],
        wheels: vec![
            Wheel {
                radius_m: 0.31,
                width_m: 0.21,
                steered: true,
            },
            Wheel {
                radius_m: 0.32,
                width_m: 0.22,
                steered: false,
            },
        ],
        note: None,
        drive: DriveMode::Rear,
    }
}

fn snapshot(root: &dyn Reflect) -> V {
    V::Struct(
        (0..root.dyn_desc().fields().len())
            .map(|i| root.get(i).unwrap())
            .collect(),
    )
}

#[derive(Default)]
struct Recording(Vec<Change>);

impl ChangeSink for Recording {
    fn changed(&mut self, change: &Change) {
        self.0.push(change.clone());
    }
}

fn run(text: &str, root: &mut dyn Reflect, sink: &mut Recording) -> ScriptReport {
    run_script(text, "vehicle", root, &L, sink).unwrap()
}

fn bad_value(text: &str, kind: &'static str) -> E {
    E::BadValue {
        text: text.into(),
        kind,
    }
}

fn bad_root(path: &str) -> E {
    E::BadRoot {
        path: path.into(),
        root: "vehicle".into(),
    }
}

fn bad_name(name: &str) -> E {
    E::BadSaveName { name: name.into() }
}

fn missing(command: &str) -> E {
    E::MissingArgument {
        command: command.into(),
    }
}

fn refusal(command: &str) -> E {
    let mut root = object();
    let before = snapshot(&root);
    let mut sink = Recording::default();
    let report = run(&format!("{command} # expect-error\n"), &mut root, &mut sink);
    assert_eq!((report.commands, report.ok), (1, 0), "{command}");
    assert_eq!(report.errors.len(), 1, "{command}");
    let error = report.errors[0].error.clone();
    assert_eq!(report.errors[0].line, 1, "{command}");
    assert_eq!(report.output, [format!("error line 1: {error}")]);
    assert!(report.saves.is_empty());
    assert_eq!(snapshot(&root), before, "{command}");
    assert!(sink.0.is_empty(), "{command}");
    error
}

#[test]
fn script_set_get_known_values() {
    let script = concat!(
        "# set and get\n",
        "set vehicle.mass_kg 1350.0\n",
        "get vehicle.mass_kg\n",
        "set vehicle.wheels[1].radius_m 0.34\n",
        "get vehicle.wheels[1]\n",
        "set vehicle.note some(\"hi\")\n",
        "set vehicle.drive All\n",
        "get vehicle.position\n",
        "set vehicle.position[2] -1.5\n",
        "get vehicle.position\n",
    );
    let mut root = object();
    let mut sink = Recording::default();
    let report = run(script, &mut root, &mut sink);
    assert_eq!((report.commands, report.ok), (9, 9));
    assert!(report.errors.is_empty());
    assert!(report.saves.is_empty());
    assert_eq!(
        report.output,
        [
            "get vehicle.mass_kg = 1350.0",
            "get vehicle.wheels[1] = {radius_m=0.34,width_m=0.22,steered=false}",
            "get vehicle.position = [0.0,0.0,0.0]",
            "get vehicle.position = [0.0,0.0,-1.5]",
        ]
    );
    assert_eq!(root.mass_kg, 1350.0);
    assert_eq!(root.wheels[1].radius_m, 0.34);
    assert_eq!(root.note.as_deref(), Some("hi"));
    assert_eq!(root.get(5), Some(V::Enum("All".into())));
    assert_eq!(root.position, [0.0, 0.0, -1.5]);
    assert_eq!(sink.0.len(), 5);
    assert_eq!(
        sink.0.iter().map(Change::path).collect::<Vec<_>>(),
        [
            "mass_kg",
            "wheels[1].radius_m",
            "note",
            "drive",
            "position[2]",
        ]
    );
    assert!(sink.0.iter().all(|change| change.type_name() == "Vehicle"));
    assert_eq!(sink.0[0].old(), &V::F64(1200.0));
    assert_eq!(sink.0[0].new_value(), &V::F64(1350.0));
    for number in [
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -f64::MAX,
        -0.0,
    ] {
        let report = run(
            &format!("set vehicle.position[0] {number:?}\n"),
            &mut root,
            &mut sink,
        );
        assert_eq!((report.commands, report.ok), (1, 1));
        assert_eq!(root.position[0].to_bits(), number.to_bits());
    }

    let mut other = object().wheels.remove(0);
    let mut sink = Recording::default();
    let report = run_script(
        "set part.radius_m 0.4\nget part.radius_m\n",
        "part",
        &mut other,
        &L,
        &mut sink,
    )
    .unwrap();
    assert_eq!(report.output, ["get part.radius_m = 0.4"]);
    assert_eq!(other.radius_m, 0.4);
    assert_eq!(sink.0[0].type_name(), "Wheel");
}

#[test]
fn script_error_lines_numbered() {
    let script = concat!(
        "# three expected errors\n",
        "set vehicle.mass_kg 1400.0\n",
        "set vehicle.speed 1.0 # expect-error\n",
        "\n",
        "set vehicle.wheels[5].radius_m 0.3 # expect-error\n",
        "set vehicle.mass_kg 9000.0 # expect-error\n",
        "get vehicle.mass_kg\n",
    );
    let mut root = object();
    let mut sink = Recording::default();
    let report = run(script, &mut root, &mut sink);
    assert_eq!((report.commands, report.ok), (5, 2));
    assert_eq!(
        report.errors.iter().map(|e| e.line).collect::<Vec<_>>(),
        [3, 5, 6]
    );
    assert_eq!(
        report.output,
        [
            "error line 3: speed: no field speed",
            "error line 5: wheels[5].radius_m: index 5 out of range (length 2)",
            "error line 6: mass_kg: value outside the field's range",
            "get vehicle.mass_kg = 1400.0",
        ]
    );
    assert_eq!(root.mass_kg, 1400.0);
    assert_eq!(sink.0.len(), 1);
    let unmarked = script.replace("9000.0 # expect-error", "9000.0");
    let mut root = object();
    let mut sink = Recording::default();
    let error = run_script(&unmarked, "vehicle", &mut root, &L, &mut sink).unwrap_err();
    assert_eq!(
        error,
        F::Unexpected {
            line: 6,
            error: E::Access(AccessError::OutOfRange {
                path: "mass_kg".into(),
            }),
        }
    );
    assert!(error.to_string().starts_with("line 6: mass_kg: value outside"));
    assert_eq!(root.mass_kg, 1400.0);
    assert_eq!(sink.0.len(), 1);
    let mut root = object();
    let mut sink = Recording::default();
    let result = run_script(
        "set vehicle.mass_kg 1300.0\nget vehicle.mass_kg # expect-error\n",
        "vehicle",
        &mut root,
        &L,
        &mut sink,
    );
    assert_eq!(result, Err(F::ExpectedErrorMissing { line: 2 }));
    assert_eq!(root.mass_kg, 1300.0);
    assert_eq!(sink.0.len(), 1);
    assert_eq!(
        F::ExpectedErrorMissing { line: 2 }.to_string(),
        "line 2: expected an error, the command succeeded"
    );
}

#[test]
fn script_add_remove_go_through_set_path() {
    let script = concat!(
        "add vehicle.wheels {radius_m=0.5,width_m=0.25,steered=false}\n",
        "remove vehicle.wheels[0]\n",
        "get vehicle.wheels\n",
        "add vehicle.wheels {radius_m=2.0,width_m=0.25,steered=false} # expect-error\n",
        "add vehicle.position 1.0 # expect-error\n",
        "remove vehicle.wheels # expect-error\n",
        "remove vehicle.wheels[7] # expect-error\n",
    );
    let mut root = object();
    let original_wheels = root.get(3).unwrap();
    let mut sink = Recording::default();
    let report = run(script, &mut root, &mut sink);
    assert_eq!((report.commands, report.ok, report.errors.len()), (7, 3, 4));
    assert_eq!(
        report.output,
        [
            "get vehicle.wheels = [{radius_m=0.32,width_m=0.22,steered=false},{radius_m=0.5,width_m=0.25,steered=false}]",
            "error line 4: wheels[2].radius_m: value outside the field's range",
            "error line 5: position is not a list (array)",
            "error line 6: remove needs a list item path, got wheels",
            "error line 7: wheels[7]: index 7 out of range (length 2)",
        ]
    );
    assert_eq!(sink.0.len(), 2);
    assert!(sink.0.iter().all(|change| change.path() == "wheels"));
    assert!(sink.0.iter().all(|change| change.type_name() == "Vehicle"));
    let new_item = V::Struct(vec![V::F64(0.5), V::F64(0.25), V::Bool(false)]);
    let V::List(mut appended) = original_wheels.clone() else {
        panic!("list")
    };
    appended.push(new_item.clone());
    assert_eq!(sink.0[0].old(), &original_wheels);
    assert_eq!(sink.0[0].new_value(), &V::List(appended.clone()));
    assert_eq!(sink.0[1].old(), &V::List(appended));
    let expected = V::List(vec![
        V::Struct(vec![V::F64(0.32), V::F64(0.22), V::Bool(false)]),
        new_item,
    ]);
    assert_eq!(sink.0[1].new_value(), &expected);
    assert_eq!(root.get(3), Some(expected));

    let mut root = object();
    let before = snapshot(&root);
    let mut sink = Recording::default();
    let limited = Limits {
        max_list_length: 2,
        ..L
    };
    let report = run_script(
        "add vehicle.wheels {radius_m=0.5,width_m=0.25,steered=false} # expect-error\n\
         remove vehicle.position[0] # expect-error\n",
        "vehicle",
        &mut root,
        &limited,
        &mut sink,
    )
    .unwrap();
    assert_eq!(
        report.errors[0].error,
        E::Access(AccessError::TooLong {
            path: "wheels".into(),
            max: 2,
        })
    );
    assert_eq!(
        report.errors[1].error,
        E::NotAList {
            path: "position".into(),
            kind: "array",
        }
    );
    assert_eq!(snapshot(&root), before);
    assert!(sink.0.is_empty());
}

#[test]
fn script_save_uses_text_format() {
    let mut root = object();
    let mut sink = Recording::default();
    let report = run(
        "set vehicle.mass_kg 1350.0\nsave edit_out.txt\n",
        &mut root,
        &mut sink,
    );
    assert_eq!((report.commands, report.ok), (2, 2));
    assert!(report.errors.is_empty());
    assert_eq!(report.saves.len(), 1);
    assert_eq!(report.saves[0].name, "edit_out.txt");
    let value = snapshot(&root);
    let expected = save_text(&Vehicle::desc(), &value, &L).unwrap();
    assert_eq!(report.saves[0].text, expected);
    let loaded = load_text(&Vehicle::desc(), &report.saves[0].text, &L).unwrap();
    assert_eq!(loaded.value, value);
    assert_eq!(
        report.output,
        [format!("saved edit_out.txt bytes={}", expected.len())]
    );
    assert_eq!(sink.0.len(), 1);
    let short = Limits {
        max_nesting_depth: 1,
        ..L
    };
    let report = run_script(
        "save safe.txt # expect-error\n",
        "vehicle",
        &mut root,
        &short,
        &mut sink,
    )
    .unwrap();
    assert!(matches!(&report.errors[0].error, E::Save(_)));
    assert!(report.errors[0].error.to_string().starts_with("cannot save: "));
    assert!(report.saves.is_empty());
}

#[test]
fn script_refusals_named() {
    let cases = [
        (
            "move vehicle.x",
            E::UnknownCommand {
                name: "move".into(),
            },
        ),
        ("get", missing("get")),
        ("set vehicle.name", missing("set")),
        (
            "get vehicle.name extra",
            E::ExtraArgument {
                command: "get".into(),
            },
        ),
        ("set mass_kg 1300.0", bad_root("mass_kg")),
        ("set vehicles.mass_kg 1300.0", bad_root("vehicles.mass_kg")),
        (
            "set vehicle.a..b 1",
            E::Path {
                path: "a..b".into(),
                error: PathError::EmptySegment { at: 2 },
            },
        ),
        ("set vehicle.mass_kg 1350", bad_value("1350", "f64")),
        ("set vehicle.drive Sideways", bad_value("Sideways", "enum")),
        ("save ../x", bad_name("../x")),
        ("save a/b", bad_name("a/b")),
    ];
    for (command, error) in cases {
        assert_eq!(refusal(command), error, "{command}");
    }
    for name in [
        "a\\b",
        "C:\\x",
        "a..b",
        ".hidden",
        "-x",
        "a\tb",
        "a\u{2028}b",
    ] {
        assert_eq!(refusal(&format!("save {name}")), bad_name(name));
    }
    for command in [
        "set vehicle.mass_kg NaN",
        "set vehicle.mass_kg inf",
        "set vehicle.mass_kg -inf",
        "set vehicle.mass_kg 1e999",
        "set vehicle.mass_kg  1350.0",
        "set vehicle.mass_kg 1350.0 # other",
        "set vehicle.name \"bad\\q\"",
        "set vehicle.name \"raw\tcontrol\"",
        "set vehicle.position [0.0,0.0]",
        "set vehicle.wheels {radius_m=0.5}",
        "add vehicle.wheels {radius_m=0.5,radius_m=0.4,width_m=0.25,steered=false}",
        "set vehicle.wheels[999999999999999999999999] 1.0",
        "get vehicle.name[0]",
        "get vehicle.note.x",
        "get vehicle.\u{e9}",
        "get vehicle.C:\\x",
        "get vehicle.wheels[-1]",
        "get vehicle.wheels[01]",
        "get vehicle.",
        "get vehicle.name extra # other",
        " get vehicle.name",
        " # comment",
        "get\tvehicle.name",
        "add vehicle.wheels",
        "remove",
    ] {
        refusal(command);
    }
    let long = "x".repeat(L.max_path_length + 1);
    assert_eq!(
        refusal(&format!("get vehicle.{long}")),
        E::Path {
            path: long.clone(),
            error: PathError::TooLong {
                len: long.len(),
                max: L.max_path_length,
            },
        }
    );
    let token = format!("[{}0.0]", "0.0,".repeat(L.max_list_length));
    assert_eq!(
        refusal(&format!("set vehicle.position {token}")),
        bad_value(&token, "array")
    );
    let mut root = object();
    let before = snapshot(&root);
    let mut sink = Recording::default();
    let error = run_script(
        "set vehicle.mass_kg 1300.0\n# comment\r\n",
        "vehicle",
        &mut root,
        &L,
        &mut sink,
    )
    .unwrap_err();
    assert_eq!(error, F::CarriageReturn { line: 2 });
    assert_eq!(
        error.to_string(),
        "line 2: carriage return (scripts use LF line ends)"
    );
    assert_eq!(snapshot(&root), before);
    assert!(sink.0.is_empty());
    let huge = "x".repeat(usize::try_from(L.max_text_file_bytes).unwrap() + 1);
    let error = run_script(&huge, "vehicle", &mut root, &L, &mut sink).unwrap_err();
    assert_eq!(
        error,
        F::TooLarge {
            bytes: huge.len() as u64,
            max: L.max_text_file_bytes,
        }
    );
    assert_eq!(
        error.to_string(),
        format!(
            "script is {} bytes, over the limit {}",
            huge.len(),
            L.max_text_file_bytes
        )
    );
    assert_eq!(snapshot(&root), before);
    assert!(sink.0.is_empty());
    let size = usize::try_from(L.max_text_file_bytes).unwrap();
    let report = run(&format!("#{}", "x".repeat(size - 1)), &mut root, &mut sink);
    assert_eq!((report.commands, report.ok), (0, 0));
    let report = run("\n \t\n# comment\n", &mut root, &mut sink);
    assert_eq!((report.commands, report.ok), (0, 0));
    assert!(report.output.is_empty());
    assert!(report.errors.is_empty());
}
