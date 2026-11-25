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
fn demo_glue_proof_passes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (code, out, err) = invoke(&["--glue-proof"], root);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        out,
        "glue-proof types=Vehicle,VehicleWide extra_field=armor_rating schema_diff_lines=1 inspector_diff_lines=1 save_diff_lines=1 script_ok=true generic_modules_clean=true result=PASS\n"
    );
    let o = demo::glue_proof::outcome(root).unwrap();
    assert_eq!(o.extra_count, 1);
    let error = match demo::glue_proof::outcome(&root.join("fixtures")) {
        Ok(_) => panic!("glue proof accepted a root without generic modules"),
        Err(error) => error,
    };
    assert!(
        error.starts_with("FAIL glue-proof cannot read src/text.rs: "),
        "{error}"
    );
}

#[test]
fn demo_vehiclewide_inspector_matches_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (code, out, err) = invoke(
        &[
            "--expect-inspector",
            "VehicleWide",
            "fixtures/vehiclewide.inspector.txt",
        ],
        root,
    );
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        out,
        "inspect VehicleWide widgets=11 expected=fixtures/vehiclewide.inspector.txt result=PASS\n"
    );
    let base: Vec<_> = include_str!("../fixtures/vehicle.inspector.txt")
        .lines()
        .collect();
    let wide_text = include_str!("../fixtures/vehiclewide.inspector.txt");
    let wide: Vec<_> = wide_text.lines().collect();
    assert_eq!(base.len(), 10);
    assert_eq!(wide.len(), 11);
    assert_eq!(&wide[..10], base.as_slice());
    assert_eq!(
        wide[10],
        "widget path=armor_rating kind=f64 label=\"Armor rating\" min=0 max=10"
    );
    assert!(wide_text.ends_with('\n'));
    assert!(!wide_text.contains('\r'));

    let (code, out, err) = invoke(
        &["--expect-schema", "VehicleWide", "fixtures/vehicle.schema.txt"],
        root,
    );
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    let lines: Vec<_> = out.lines().collect();
    assert_eq!(lines.len(), 2, "{out}");
    assert!(lines[0].starts_with("schema VehicleWide version=2 fields=7 hash="));
    assert!(lines[0].ends_with("result=FAIL"));
    assert_eq!(lines[1], "diff line=2");
}

#[test]
fn diff_lines_counts_insertions_and_changes() {
    let diff_lines = demo::glue_proof::diff_lines;
    assert_eq!(diff_lines("", ""), 0);
    assert_eq!(diff_lines("", "a\n"), 1);
    assert_eq!(diff_lines("a\n", ""), 1);
    assert_eq!(diff_lines("type A\n", "type B\n"), 0);
    assert_eq!(diff_lines("a\nb\n", "a\nx\nb\n"), 1);
    assert_eq!(diff_lines("a\nb\n", "a\nc\n"), 2);
    assert_eq!(diff_lines("a\nb\n", "a\n"), 1);
    assert_eq!(diff_lines("a\nb\n", "a\nb\n"), 0);
    assert_eq!(diff_lines("h\ntype A v=1\nb\n", "h\ntype B v=1\nb\n"), 0);
    assert_eq!(diff_lines("x\nb\n", "y\nb\n"), 2);
    assert_eq!(
        diff_lines("type A\ntype W x=1\nb\n", "type B\ntype W x=2\nb\n"),
        2
    );
}
