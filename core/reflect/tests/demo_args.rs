#[path = "../examples/reflect_demo.rs"]
mod demo;

use std::path::Path;

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

#[test]
fn demo_args_usage_errors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let cases: &[&[&str]] = &[
        &[],
        &["--bogus"],
        &["--expect-schema", "Vehicle"],
        &["--expect-schema", "Car", "fixtures/vehicle.schema.txt"],
        &["--expect-schema", "Vehicle", "/abs/x.txt"],
        &["--expect-schema", "Vehicle", "../x.txt"],
        &["--expect-schema", "Vehicle", "./x.txt"],
        &["--expect-schema", "Vehicle", "C:\\x.txt"],
        &["--expect-schema", "Vehicle", "..\\x.txt"],
        &["--expect-schema", "Vehicle", "bad\nfile"],
        &["--expect-schema", "Vehicle", ""],
        &["--expect-schema", "Vehicle", "--watch"],
        &[
            "--expect-schema",
            "Vehicle",
            "fixtures/vehicle.schema.txt",
            "--watch",
        ],
        &["--expect-schema", "Vehicle", "fixtures/missing.txt"],
        &["--glue-proof"],
    ];
    for args in cases {
        let (code, out, err) = invoke(args, root);
        assert_eq!(code, 2, "{args:?}: {err}");
        assert!(out.is_empty(), "{args:?}: {out}");
        assert!(err.starts_with("reflect_demo: usage: "), "{err}");
        assert_eq!(err.lines().count(), 1, "{err}");
        let stage = match args.first().copied() {
            Some("--glue-proof") => Some("R10"),
            _ => None,
        };
        if let Some(stage) = stage {
            assert_eq!(
                err,
                format!(
                    "reflect_demo: usage: {} is not implemented yet ({stage})\n",
                    args[0]
                )
            );
        }
    }
}

#[test]
fn demo_schema_and_inspector_match_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (code, out, err) = invoke(
        &["--expect-schema", "Vehicle", "fixtures/vehicle.schema.txt"],
        root,
    );
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty());
    assert_eq!(out, "schema Vehicle version=2 fields=6 hash=8b1bd5d2c9f51001 expected=fixtures/vehicle.schema.txt result=PASS\n");
    let (code, out, err) = invoke(
        &["--expect-inspector", "Vehicle", "fixtures/vehicle.inspector.txt"],
        root,
    );
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty());
    assert_eq!(
        out,
        "inspect Vehicle widgets=10 expected=fixtures/vehicle.inspector.txt result=PASS\n"
    );
    let count = include_str!("../fixtures/vehicle.inspector.txt")
        .lines()
        .filter(|s| s.starts_with("widget "))
        .count();
    assert_eq!(count, 10);
    assert!(out.contains(&format!("widgets={count} ")));
}

#[test]
fn demo_reports_each_mismatch() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "reflect-demo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&root).unwrap();
    let schema = include_str!("../fixtures/vehicle.schema.txt");
    let inspector = include_str!("../fixtures/vehicle.inspector.txt");
    let cases = [
        ("--expect-schema", schema.trim_end_matches('\n').to_owned(), 14),
        ("--expect-schema", schema.replace('\n', "\r\n"), 1),
        ("--expect-schema", schema.replace("version=2", "version=3"), 2),
        ("--expect-schema", String::new(), 1),
        (
            "--expect-inspector",
            inspector.replace("path=wheels kind=list", "path=wheels kind=List"),
            5,
        ),
        ("--expect-schema", format!("{schema} "), 15),
    ];
    for (flag, text, line) in cases {
        std::fs::write(root.join("a.txt"), text).unwrap();
        let (code, out, err) = invoke(&[flag, "Vehicle", "a.txt"], &root);
        assert_eq!(code, 1, "{out}\n{err}");
        assert!(err.is_empty(), "{err}");
        let lines: Vec<_> = out.lines().collect();
        assert_eq!(lines.len(), 2, "{out}");
        assert!(lines[0].ends_with("result=FAIL"), "{out}");
        assert_eq!(lines[1], format!("diff line={line}"));
    }
    std::fs::remove_file(root.join("a.txt")).unwrap();
    std::fs::remove_dir(root).unwrap();
}
