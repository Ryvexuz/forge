#[path = "../examples/reflect_demo.rs"]
mod demo;

use reflect::desc::Reflect;
use reflect::sample::Vehicle;
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
fn demo_inject_fault_is_caught() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (code, out, err) = invoke(&["--inject-fault"], root);
    assert_eq!(code, 1, "{out}\n{err}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        out,
        concat!(
            "fault injected: Vehicle field mass_kg dropped from the descriptor\n",
            "roundtrip Vehicle bytes=235 golden=false text_identical=true value_equal=false result=FAIL\n",
            "diff line=2\n"
        )
    );
    let (code, out, err) = invoke(&["--inject-fault", "x"], root);
    assert_eq!(code, 2, "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.starts_with("reflect_demo: usage: "), "{err}");
}

#[test]
fn demo_inject_fault_leaves_real_descriptor_intact() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let b = demo::fault::broken_descriptor().unwrap();
    let real = Vehicle::desc();
    assert_eq!(b.name(), "Vehicle");
    assert_eq!(b.version(), real.version());
    assert_eq!(b.fields().len(), 5);
    assert_eq!(b.field_index("mass_kg"), None);
    let names: Vec<_> = b.fields().iter().map(|f| f.name.as_str()).collect();
    let expected: Vec<_> = real
        .fields()
        .iter()
        .filter(|f| f.name != "mass_kg")
        .map(|f| f.name.as_str())
        .collect();
    assert_eq!(names, expected);
    assert_eq!(Vehicle::desc().field_index("mass_kg"), Some(1));
    let o = demo::fault::outcome(root).unwrap();
    assert!(o.text_identical && !o.golden && !o.value_equal);
    let (code, _, err) = invoke(&["--inject-fault"], root);
    assert_eq!(code, 1, "{err}");
    let (code, out, err) = invoke(&["--roundtrip", "Vehicle", "fixtures/vehicle.v2.txt"], root);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(out, "roundtrip Vehicle bytes=252 golden=true text_identical=true value_equal=true result=PASS\n");
}
