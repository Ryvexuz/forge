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
fn demo_watch_prints_events() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (code, out, err) = invoke(&["--watch"], root);
    assert_eq!(code, 0, "{err}\n{out}");
    assert!(err.is_empty(), "{err}");
    assert_eq!(
        out,
        concat!(
            "watch Vehicle sets=4 accepted=3 rejected=1 events=3 first=\"vehicle.mass_kg 1200.0 -> 1350.0\" result=PASS\n",
            "event 1 watcher=a path=vehicle.mass_kg old=1200.0 new=1350.0\n",
            "event 1 watcher=b path=vehicle.mass_kg old=1200.0 new=1350.0\n",
            "event 2 watcher=a path=vehicle.wheels[0].radius_m old=0.31 new=0.33\n",
            "event 2 watcher=b path=vehicle.wheels[0].radius_m old=0.31 new=0.33\n",
            "event 3 watcher=a path=vehicle.wheels[1].steered old=false new=true\n",
            "event 3 watcher=b path=vehicle.wheels[1].steered old=false new=true\n"
        )
    );
    let (code, out, err) = invoke(&["--watch", "x"], root);
    assert_eq!(code, 2, "{err}");
    assert!(out.is_empty(), "{out}");
    assert!(err.starts_with("reflect_demo: usage: "), "{err}");
}

#[test]
fn demo_watch_rejected_set_changes_nothing() {
    let s = demo::watch::scenario().unwrap();
    assert_eq!(s.sets, 4);
    assert_eq!(s.accepted, 3);
    assert_eq!(s.rejected, 1);
    assert_eq!(s.mass_after, 1350.0);
    assert_eq!(s.a, s.b);
    assert_eq!(s.a.len(), 3);
    let paths: Vec<_> = s.a.entries().iter().map(|entry| entry.path()).collect();
    assert_eq!(paths, ["mass_kg", "wheels[0].radius_m", "wheels[1].steered"]);
}
