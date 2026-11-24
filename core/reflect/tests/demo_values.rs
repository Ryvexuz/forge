#[path = "../examples/reflect_demo.rs"]
mod demo;

use reflect::desc::Reflect;
use reflect::limits::Limits;
use reflect::sample::Vehicle;
use reflect::text::load_text;
use reflect::value::Value;
use std::path::{Path, PathBuf};

const V2: &str = include_str!("../fixtures/vehicle.v2.txt");
const V1: &str = include_str!("../fixtures/vehicle.v1.txt");
const SCRIPT: &str = include_str!("../fixtures/edit.script");
const EDITED: &str = include_str!("../fixtures/edit.expected.txt");
const ERRORS: &str = "error line 10: speed: no field speed\n\
error line 11: wheels[9].radius_m: index 9 out of range (length 2)\n\
error line 12: mass_kg: value outside the field's range\n";

fn invoke(args: &[&str], root: &Path) -> (u8, String, String) {
    let args: Vec<_> = args.iter().map(|s| (*s).to_owned()).collect();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = demo::run(&args, root, &mut out, &mut err);
    (
        code,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

fn output(args: &[&str], root: &Path, expected_code: u8) -> String {
    let (code, out, err) = invoke(args, root);
    assert_eq!(code, expected_code, "{args:?}\n{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    out
}

fn temp_root(label: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "reflect-values-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    root
}

fn finish(root: &Path, names: &[&str]) {
    let mut actual: Vec<_> = std::fs::read_dir(root)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    actual.sort();
    let mut expected = names.to_vec();
    expected.sort();
    assert_eq!(actual, expected, "demo must not write files");
    for name in names {
        std::fs::remove_file(root.join(name)).unwrap();
    }
    std::fs::remove_dir(root).unwrap();
}

#[test]
fn demo_roundtrip_matches_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        output(&["--roundtrip", "Vehicle", "fixtures/vehicle.v2.txt"], root, 0),
        "roundtrip Vehicle bytes=252 golden=true text_identical=true value_equal=true result=PASS\n"
    );
    let out = output(&["--roundtrip", "Wheel", "fixtures/vehicle.v2.txt"], root, 1);
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with("roundtrip Wheel bytes="), "{out}");
    assert!(
        lines[0].ends_with(" golden=false text_identical=true value_equal=true result=FAIL"),
        "{out}"
    );
    assert_eq!(lines[1], "diff line=2");
    assert!(out.ends_with('\n'));

    let root = temp_root("roundtrip");
    for (bytes, line) in [
        (V2.replace("1350.0", "1350.1"), 4),
        (V2.replace('\n', "\r\n"), 1),
        (V2.trim_end_matches('\n').to_owned(), 9),
        (String::new(), 1),
    ] {
        std::fs::write(root.join("a.txt"), bytes).unwrap();
        assert_eq!(
            output(&["--roundtrip", "Vehicle", "a.txt"], &root, 1),
            format!("roundtrip Vehicle bytes=252 golden=false text_identical=true value_equal=true result=FAIL\ndiff line={line}\n")
        );
    }
    finish(&root, &["a.txt"]);
}

#[test]
fn demo_load_v1_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        output(&["--load", "Vehicle", "fixtures/vehicle.v1.txt"], root, 0),
        "load Vehicle file=fixtures/vehicle.v1.txt from_version=1 to_version=2 defaulted=1 renamed=1 ignored=1 result=PASS\n\
note: field mass_kg read from old name mass (line 4)\n\
note: unknown field colour ignored (line 7)\n\
note: field drive set to its default\n"
    );
    let limits = Limits::parse(include_str!("../reflect.toml")).unwrap();
    let desc = Vehicle::desc();
    let loaded = load_text(&desc, V1, &limits).unwrap();
    let Value::Struct(fields) = loaded.value else {
        panic!("loaded Vehicle must be a struct");
    };
    for (name, value) in [
        ("mass_kg", Value::F64(1234.5)),
        ("name", Value::Text("Old".into())),
        ("drive", Value::Enum("Rear".into())),
    ] {
        assert_eq!(fields[desc.field_index(name).unwrap()], value, "{name}");
    }
    assert_eq!(
        output(&["--load", "Vehicle", "fixtures/vehicle.v2.txt"], root, 0),
        "load Vehicle file=fixtures/vehicle.v2.txt from_version=2 to_version=2 defaulted=0 renamed=0 ignored=0 result=PASS\n"
    );
    assert_eq!(
        output(&["--load", "Wheel", "fixtures/vehicle.v1.txt"], root, 1),
        "load Wheel file=fixtures/vehicle.v1.txt result=FAIL\nerror: line 2: file holds type Vehicle, expected Wheel\n"
    );

    let root = temp_root("load");
    std::fs::write(root.join("a.txt"), V1.replace("mass = 1234.5\n", "")).unwrap();
    assert_eq!(
        output(&["--load", "Vehicle", "a.txt"], &root, 1),
        "load Vehicle file=a.txt result=FAIL\nerror: missing field mass_kg\n"
    );
    std::fs::write(root.join("a.txt"), [0xff]).unwrap();
    assert_eq!(
        output(&["--load", "Vehicle", "a.txt"], &root, 1),
        "load Vehicle file=a.txt result=FAIL\nerror: file is not UTF-8\n"
    );
    let large = " ".repeat(usize::try_from(limits.max_text_file_bytes).unwrap() + 1);
    std::fs::write(root.join("a.txt"), &large).unwrap();
    assert_eq!(
        output(&["--load", "Vehicle", "a.txt"], &root, 1),
        format!(
            "load Vehicle file=a.txt result=FAIL\nerror: text is {} bytes, over the limit {}\n",
            large.len(),
            limits.max_text_file_bytes
        )
    );
    finish(&root, &["a.txt"]);
}

#[test]
fn demo_script_matches_expected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    assert_eq!(
        output(&["--script", "fixtures/edit.script", "fixtures/edit.expected.txt"], root, 0),
        format!("script fixtures/edit.script commands=11 ok=8 errors=3 expected=fixtures/edit.expected.txt result=PASS\n{ERRORS}")
    );
    let root = temp_root("save");
    std::fs::write(root.join("a.script"), "save snapshot\n").unwrap();
    std::fs::write(root.join("b.txt"), V2).unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 0),
        "script a.script commands=1 ok=1 errors=0 expected=b.txt result=PASS\n"
    );
    finish(&root, &["a.script", "b.txt"]);
}

#[test]
fn demo_script_reports_mismatch_and_failure() {
    let root = temp_root("failure");
    std::fs::write(root.join("a.script"), SCRIPT).unwrap();
    for (expected, line) in [
        (EDITED.replace("1500.0", "1501.0"), 4),
        (EDITED.replace('\n', "\r\n"), 1),
        (EDITED.trim_end_matches('\n').to_owned(), 9),
    ] {
        std::fs::write(root.join("b.txt"), expected).unwrap();
        assert_eq!(
            output(&["--script", "a.script", "b.txt"], &root, 1),
            format!("script a.script commands=11 ok=8 errors=3 expected=b.txt result=FAIL\n{ERRORS}diff line={line}\n")
        );
    }
    std::fs::write(root.join("b.txt"), EDITED).unwrap();
    std::fs::write(
        root.join("a.script"),
        SCRIPT.replace("9000.0 # expect-error", "9000.0"),
    )
    .unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 1),
        "script a.script result=FAIL\nerror: line 12: mass_kg: value outside the field's range\n"
    );
    std::fs::write(
        root.join("a.script"),
        SCRIPT.replace("get vehicle.mass_kg\n", "get vehicle.mass_kg # expect-error\n"),
    )
    .unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 1),
        "script a.script result=FAIL\nerror: line 9: expected an error, the command succeeded\n"
    );
    std::fs::write(
        root.join("a.script"),
        format!("{SCRIPT}# skipped # expect-error\n"),
    )
    .unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 1),
        format!("script a.script commands=11 ok=8 errors=3 expected=b.txt result=FAIL\n{ERRORS}diff line=0\n")
    );
    std::fs::write(root.join("a.script"), [0xff]).unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 1),
        "script a.script result=FAIL\nerror: file is not UTF-8\n"
    );
    let limits = Limits::parse(include_str!("../reflect.toml")).unwrap();
    let large = "#".repeat(usize::try_from(limits.max_text_file_bytes).unwrap() + 1);
    std::fs::write(root.join("a.script"), &large).unwrap();
    assert_eq!(
        output(&["--script", "a.script", "b.txt"], &root, 1),
        format!(
            "script a.script result=FAIL\nerror: script is {} bytes, over the limit {}\n",
            large.len(),
            limits.max_text_file_bytes
        )
    );
    for args in [
        vec!["--script", "missing.script", "b.txt"],
        vec!["--script", "a.script", "missing.txt"],
        vec!["--roundtrip", "Vehicle", "missing.txt"],
        vec!["--load", "Vehicle", "missing.txt"],
    ] {
        let (code, out, err) = invoke(&args, &root);
        assert_eq!(code, 2, "{args:?}: {out}\n{err}");
        assert!(out.is_empty(), "{out}");
        assert!(err.starts_with("reflect_demo: usage: cannot read "), "{err}");
        assert_eq!(err.lines().count(), 1, "{err}");
    }
    finish(&root, &["a.script", "b.txt"]);
}
