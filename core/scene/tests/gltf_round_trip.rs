use scene::gltf::{import_gltf, GltfError};
use scene::gltf_write::{export_gltf, GltfExportError};
use scene::hierarchy::{HierarchyError, Node};
use scene::layer::Layer;
use scene::math::{Quat, Vec3};
use scene::scene::{Mesh, Scene};
use scene::transform::Transform;
use scene::usd::{compare_scenes, UsdError};
use std::path::{Path, PathBuf};

#[path = "../examples/gltf_roundtrip.rs"]
mod example;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/gltf/{name}"));
    std::fs::read(path).unwrap()
}

fn scene(parents: &[Option<usize>]) -> Scene {
    let mut scene = Scene::default();
    scene.hierarchy.nodes = parents
        .iter()
        .map(|&parent| Node {
            parent,
            mesh: None,
            transform: Transform::default(),
        })
        .collect();
    scene
}

fn triangle() -> Scene {
    let mut s = scene(&[None]);
    s.hierarchy.nodes[0].mesh = Some(0);
    s.meshes.push(Mesh {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
    });
    s
}

fn round_trip(a: &Scene) -> (String, Vec<u8>) {
    let first = export_gltf(a, "rt.bin").unwrap();
    let b = import_gltf(first.0.as_bytes(), "rt.bin", &first.1).unwrap();
    assert_eq!(compare_scenes(a, &b), Ok(()));
    assert_eq!(export_gltf(&b, "rt.bin").unwrap(), first);
    first
}

fn reimport_refused(s: &Scene) {
    assert!(matches!(
        export_gltf(s, "rt.bin"),
        Err(GltfExportError::Reimport(_))
    ));
}

struct ExampleRoot(PathBuf);

impl ExampleRoot {
    fn new() -> Self {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("forge-gltf-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for ExampleRoot {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn example_fixture_routes() {
    let root = ExampleRoot::new();
    assert!(example::find_root(&root.0).is_err());
    std::fs::write(root.0.join("broadsword.toml"), "").unwrap();
    std::fs::create_dir(root.0.join("fixtures")).unwrap();
    for name in ["hierarchy.gltf", "hierarchy.bin"] {
        std::fs::write(root.0.join("fixtures").join(name), fixture(name)).unwrap();
    }
    let found = example::find_root(&root.0.join("fixtures")).unwrap();
    assert_eq!(found, root.0.canonicalize().unwrap());
    for (flag, input) in [
        ("--in", "tests/fixtures/gltf/hierarchy.gltf"),
        ("--in-shared", "fixtures/hierarchy.gltf"),
    ] {
        let args = [flag, input, "--out", "output/roundtrip.gltf"].map(str::to_owned);
        example::run(example::parse(args.clone()).unwrap(), &found).unwrap();
        let disk = std::fs::read_to_string(root.0.join("output/roundtrip.gltf")).unwrap();
        assert!(disk.contains(r#""uri":"roundtrip.bin""#));
        let bin = std::fs::read(root.0.join("output/roundtrip.bin")).unwrap();
        let back = import_gltf(disk.as_bytes(), "roundtrip.bin", &bin).unwrap();
        assert_eq!(back.meshes.len(), 1);
        let corrupt = args.into_iter().chain(["--corrupt-translation".into()]);
        assert_eq!(
            example::run(example::parse(corrupt).unwrap(), &found),
            Err("mismatch node 0 translation".into())
        );
    }
    // Invalid external URIs must be refused before a file is opened.
    let hostile = String::from_utf8(fixture("hierarchy.gltf"))
        .unwrap()
        .replace("hierarchy.bin", "../outside.bin");
    std::fs::write(root.0.join("fixtures/hostile.gltf"), hostile).unwrap();
    let args = ["--in-shared", "fixtures/hostile.gltf", "--out", "out.gltf"];
    let error = example::run(example::parse(args.map(str::to_owned)).unwrap(), &found).unwrap_err();
    assert!(error.starts_with("import BufferUri("), "{error}");
    assert!(!root.0.join("out.gltf").exists());
    std::fs::create_dir(root.0.join("blocked.gltf")).unwrap();
    let args = ["--in-shared", "fixtures/hierarchy.gltf", "--out", "blocked.gltf"];
    let error = example::run(example::parse(args.map(str::to_owned)).unwrap(), &found).unwrap_err();
    assert!(error.starts_with("write gltf "), "{error}");
    assert!(root.0.join("blocked.bin").is_file());
}

#[test]
fn gltf_round_trip_fixtures() {
    for (name, count) in [("hierarchy", 3), ("instances", 100)] {
        let a = import_gltf(
            &fixture(&format!("{name}.gltf")),
            &format!("{name}.bin"),
            &fixture(&format!("{name}.bin")),
        )
        .unwrap();
        assert_eq!(a.hierarchy.nodes.len(), count, "{name}");
        assert_eq!(a.meshes.len(), 1, "{name}");
        round_trip(&a);
    }
    example_fixture_routes();
}

#[test]
fn gltf_export_layout_golden() {
    let expected = r#"{"asset":{"version":"2.0","generator":"broadsword forge scene"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}]}"#;
    assert_eq!(
        export_gltf(&scene(&[None]), "t.bin").unwrap(),
        (expected.into(), vec![])
    );
    let expected = r#"{"asset":{"version":"2.0","generator":"broadsword forge scene"},"scene":0,"scenes":[{"nodes":[0]}],"nodes":[{"mesh":0,"translation":[0,0,0],"rotation":[0,0,0,1],"scale":[1,1,1]}],"meshes":[{"primitives":[{"attributes":{"POSITION":0},"indices":1,"mode":4}]}],"accessors":[{"bufferView":0,"componentType":5126,"count":3,"type":"VEC3","min":[0,0,0],"max":[1,1,0]},{"bufferView":1,"componentType":5125,"count":3,"type":"SCALAR"}],"bufferViews":[{"buffer":0,"byteOffset":0,"byteLength":36,"target":34962},{"buffer":0,"byteOffset":36,"byteLength":12,"target":34963}],"buffers":[{"uri":"t.bin","byteLength":48}]}"#;
    let (text, bin) = export_gltf(&triangle(), "t.bin").unwrap();
    assert_eq!(text, expected);
    let mut expected_bin = Vec::new();
    for n in [0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
        expected_bin.extend_from_slice(&n.to_le_bytes());
    }
    expected_bin.extend_from_slice(&[0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(bin.len(), 48);
    assert_eq!(bin, expected_bin);
    let mut children = scene(&[None, None, None]);
    children.hierarchy.nodes[2].parent = Some(0);
    children.hierarchy.nodes[1].parent = Some(0);
    assert!(round_trip(&children).0.contains(r#""children":[1,2]"#));
    round_trip(&scene(&[Some(2), Some(2), None]));

    let mut two = triangle();
    two.meshes.push(two.meshes[0].clone());
    let text = round_trip(&two).0;
    assert!(text.contains(r#""bufferView":2,"componentType":5126"#));
    assert!(text.contains(r#""byteOffset":48,"byteLength":36,"target":34962"#));
    assert!(text.contains(r#""byteOffset":84,"byteLength":12,"target":34963"#));
    assert!(text.contains(r#""uri":"rt.bin","byteLength":96"#));
    let mut zero_bounds = triangle();
    zero_bounds.meshes[0].positions = vec![[-0.0; 3], [0.0; 3], [-0.0; 3]];
    assert!(round_trip(&zero_bounds)
        .0
        .contains(r#""min":[-0,-0,-0],"max":[-0,-0,-0]"#));
}

#[test]
fn gltf_export_refuses_what_cannot_round_trip() {
    let mut layered = scene(&[Some(1), Some(0)]);
    layered.layers.push(Layer {
        id: 0,
        name: "a".into(),
        visible: true,
    });
    layered.instances.push(0, 0, 0.0, 0.0).unwrap();
    assert_eq!(export_gltf(&layered, "a/b.bin"), Err(GltfExportError::Layers));
    layered.layers.clear();
    assert_eq!(
        export_gltf(&layered, "a/b.bin"),
        Err(GltfExportError::Instances)
    );
    assert_eq!(
        export_gltf(&scene(&[Some(1), Some(0)]), "rt.bin"),
        Err(GltfExportError::Hierarchy(HierarchyError::Cycle))
    );
    assert_eq!(
        export_gltf(&scene(&[Some(1)]), "rt.bin"),
        Err(GltfExportError::Hierarchy(HierarchyError::InvalidParent {
            node: 0,
            parent: 1
        }))
    );
    let (i, q) = (1..=10000)
        .find_map(|i| {
            let q = Quat::new(1.0, i as f32 / 10000.0, 0.3, 0.7).unwrap();
            let [x, y, z, w] = q.components();
            let again = Quat::new(x, y, z, w).unwrap();
            let changed = q.components().map(f32::to_bits) != again.components().map(f32::to_bits);
            changed.then_some((i, q))
        })
        .expect("the prescribed search must find a non-fixed rotation");
    println!("first non-fixed rotation: i={i}");
    let mut rotated = scene(&[None, None]);
    for node in &mut rotated.hierarchy.nodes {
        node.transform.rotation = q;
    }
    assert_eq!(
        export_gltf(&rotated, "rt.bin"),
        Err(GltfExportError::RotationNotFixed { node: 0 })
    );
    rotated.hierarchy.nodes[0].transform.rotation = Quat::IDENTITY;
    assert_eq!(
        export_gltf(&rotated, "rt.bin"),
        Err(GltfExportError::RotationNotFixed { node: 1 })
    );
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = triangle();
        invalid.meshes[0].positions[0][0] = value;
        assert!(matches!(
            export_gltf(&invalid, "rt.bin"),
            Err(GltfExportError::Json(_))
        ));
        invalid.hierarchy.nodes[0].transform.rotation = q;
        assert_eq!(
            export_gltf(&invalid, "rt.bin"),
            Err(GltfExportError::RotationNotFixed { node: 0 })
        );
        invalid.hierarchy.nodes[0].transform.translation.x = value;
        assert_eq!(
            export_gltf(&invalid, "rt.bin"),
            Err(GltfExportError::Hierarchy(
                HierarchyError::InvalidTransform { node: 0 }
            ))
        );
    }
    for uri in [
        "a/b.bin",
        r"a\b.bin",
        "C:a.bin",
        "",
        ".hidden.bin",
        "a\r.bin",
        "a\n.bin",
    ] {
        assert_eq!(
            export_gltf(&triangle(), uri),
            Err(GltfExportError::Reimport(GltfError::BufferUri(uri.into())))
        );
    }
    let huge_uri = format!("{}.bin", "a".repeat(1_000_000));
    assert_eq!(
        export_gltf(&triangle(), &huge_uri),
        Err(GltfExportError::Reimport(GltfError::BufferUri(huge_uri)))
    );
    let mut empty = triangle();
    empty.meshes[0] = Mesh::default();
    reimport_refused(&empty);
    for indices in [vec![0, 1], vec![0, 1, u32::MAX], vec![]] {
        let mut invalid = triangle();
        invalid.meshes[0].indices = indices;
        reimport_refused(&invalid);
    }
    let mut invalid = triangle();
    invalid.hierarchy.nodes[0].mesh = Some(u32::MAX);
    reimport_refused(&invalid);
    invalid.hierarchy.nodes[0].mesh = Some(0);
    invalid.meshes[0].positions[1][0] = f32::NAN;
    reimport_refused(&invalid);

    for args in [
        vec![],
        vec!["--unknown"],
        vec!["--in"],
        vec!["--in", "a", "--in-shared", "b", "--out", "c.gltf"],
        vec!["--in", "a", "--out", "c.gltf", "--out", "d.gltf"],
        vec!["--in", "a", "--out", "c.bin"],
        vec![
            "--in",
            "a",
            "--out",
            "c.gltf",
            "--corrupt-translation",
            "--corrupt-translation",
        ],
    ] {
        assert!(example::parse(args.into_iter().map(str::to_owned)).is_err());
    }
    for path in [
        "", "../a", "/a", r"C:\a", "C:a", r"a\b", "a\r", "a\n", "a//b", "NUL.gltf",
    ] {
        assert!(example::parse(["--in", path, "--out", "ok.gltf"].map(str::to_owned)).is_err());
        assert!(example::parse(["--in", "a", "--out", path].map(str::to_owned)).is_err());
    }
}

#[test]
fn gltf_round_trip_keeps_signed_zero_and_empty_scene() {
    let mut s = scene(&[None]);
    s.hierarchy.nodes[0].transform.translation = Vec3::new(-0.0, 0.0, 1.5);
    s.hierarchy.nodes[0].transform.scale = Vec3::new(1e-30, 1e30, 0.1);
    round_trip(&s);
    for value in [
        -0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::MAX,
        -f32::MAX,
    ] {
        let mut edge = triangle();
        edge.hierarchy.nodes[0].transform.translation.x = value;
        edge.hierarchy.nodes[0].transform.scale.x = value;
        edge.meshes[0].positions[0] = [value, -value, 0.0];
        let first = round_trip(&edge);
        let back = import_gltf(first.0.as_bytes(), "rt.bin", &first.1).unwrap();
        assert_eq!(
            back.hierarchy.nodes[0].transform.translation.x.to_bits(),
            value.to_bits()
        );
    }
    let empty = Scene::default();
    let first = round_trip(&empty);
    assert!(first.1.is_empty());
    let back = import_gltf(first.0.as_bytes(), "rt.bin", &first.1).unwrap();
    assert_eq!(back.hierarchy.nodes.len(), 0);
    assert_eq!(export_gltf(&back, "rt.bin").unwrap(), first);
    // f64 independently references +1 at the extremes and near cancellation.
    for x in [
        -0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::MAX,
        -f32::MAX,
        -1.0000001,
        -1.0,
    ] {
        let reference = (f64::from(x) + 1.0) as f32;
        let actual = example::corrupt_x(x);
        if reference.to_bits() == x.to_bits() {
            assert_eq!(actual, Err("usage corrupt translation unchanged".into()));
        } else {
            assert_eq!(actual.unwrap().to_bits(), reference.to_bits());
        }
    }
    assert!(example::corrupt_x(f32::NAN).is_err());
    assert!(example::corrupt_x(f32::INFINITY).is_err());
    // A behavioural negative control for the comparator, independent of export's self-check.
    let mut changed = scene(&[None]);
    changed.hierarchy.nodes[0].transform.translation.x = 1.0;
    assert_eq!(
        compare_scenes(&scene(&[None]), &changed),
        Err(UsdError::Mismatch {
            what: "node 0 translation".into(),
        })
    );
}
