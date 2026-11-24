use scene::format::{read_scene, write_scene, FormatError, SceneFile, CURRENT_SCHEMA, MIGRATIONS};
use scene::hierarchy::Node;
use scene::instance::matrix_from_angle;
use scene::layer::Layer;
use scene::scene::Mesh;
use scene::transform::Transform;

const FIXTURE: &str = include_str!("fixtures/scene_v1.bsn");
const XF: &str = "00000000 00000000 00000000,00000000 00000000 00000000 3f800000,3f800000 3f800000 3f800000";

fn base() -> String {
    format!("schema=1\nnode,0,-,a\nxform,0,{XF}\n")
}

#[test]
fn scene_file_round_trip_byte_identical() {
    assert!(!FIXTURE.contains('\r'));
    assert!(FIXTURE.ends_with('\n'));
    let written = write_scene(&read_scene(FIXTURE).unwrap()).unwrap();
    assert_eq!(written.as_bytes(), FIXTURE.as_bytes());
    assert_eq!(write_scene(&read_scene(&written).unwrap()).unwrap(), written);
}

#[test]
fn scene_file_v1_still_loads() {
    assert_eq!(CURRENT_SCHEMA, 1);
    assert!(std::hint::black_box(MIGRATIONS).is_empty());
    let file = read_scene(FIXTURE).unwrap();
    assert_eq!(
        file.node_names,
        ["root", "Walls, exterior", "50% done\nline two"]
    );
    let nodes = &file.scene.hierarchy.nodes;
    assert_eq!(nodes.len(), 3);
    let translations = [
        [0, 0, 0],
        [0x3f800000, 0x80000000, 0xc0000000],
        [0, 0x40400000, 0],
    ];
    let rotations = [
        [0, 0, 0, 0x3f800000],
        [0x3f800000, 0, 0, 0],
        [0x80000000, 0, 0, 0x3f800000],
    ];
    let scales = [[0x3f800000; 3], [0x3f800000; 3], [0x40000000; 3]];
    for (i, node) in nodes.iter().enumerate() {
        assert_eq!(node.parent, [None, Some(0), Some(0)][i]);
        assert_eq!(node.mesh, None);
        let t = node.transform;
        assert_eq!(
            [t.translation.x, t.translation.y, t.translation.z].map(f32::to_bits),
            translations[i]
        );
        assert_eq!(t.rotation.components().map(f32::to_bits), rotations[i]);
        assert_eq!(
            [t.scale.x, t.scale.y, t.scale.z].map(f32::to_bits),
            scales[i]
        );
    }
    assert!(file.scene.meshes.is_empty());
    let layers = &file.scene.layers;
    assert_eq!(layers.len(), 2);
    for (layer, (id, name, visible)) in layers
        .iter()
        .zip([(7, "Props", true), (3, "Hidden, for now", false)])
    {
        assert_eq!(layer.id, id);
        assert_eq!(layer.name, name);
        assert_eq!(layer.visible, visible);
    }
    let inst = &file.scene.instances;
    assert_eq!(inst.layers(), [7, 3]);
    assert_eq!(inst.nodes(), [1, 2]);
    assert_eq!(
        inst.angles().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [0x3f000000, 0x80000000]
    );
    assert_eq!(
        inst.speeds().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
        [0x3dcccccd, 0xbf800000]
    );
    for (matrix, angle) in inst.matrices().iter().zip(inst.angles()) {
        assert_eq!(
            matrix.map(f32::to_bits),
            matrix_from_angle(*angle).unwrap().map(f32::to_bits)
        );
    }
    assert_eq!(file.scene.visible_instances(), [0]);
}

#[test]
fn scene_file_refuses_malformed_input() {
    use FormatError::*;
    let b = base();
    let mut cases = vec![
        ("".into(), MissingHeader),
        ("node,0,-,a\n".into(), MissingHeader),
        ("schema=1".into(), MissingFinalNewline),
        (b.trim_end_matches('\n').into(), MissingFinalNewline),
        ("schema=1\n\n".into(), UnknownKind { line: 2 }),
        ("schema=1\nmesh,0\n".into(), UnknownKind { line: 2 }),
        (
            "schema=1\nnode,0,-\n".into(),
            FieldCount {
                line: 2,
                expected: 4,
                found: 3,
            },
        ),
        (
            "schema=1\nnode,0,-,a,b\n".into(),
            FieldCount {
                line: 2,
                expected: 4,
                found: 5,
            },
        ),
        (
            "schema=1\nnode,00,-,a\n".into(),
            BadNumber { line: 2, field: 1 },
        ),
        (
            "schema=1\nnode,0,-1,a\n".into(),
            BadNumber { line: 2, field: 2 },
        ),
        (format!("{b}layer,7,L,2\n"), BadNumber { line: 4, field: 3 }),
        (
            format!("schema=1\nnode,0,1,a\nxform,0,{XF}\n"),
            UnknownNode { line: 2, id: 1 },
        ),
        (
            format!("{b}layer,7,L,1\ninst,7,1,00000000,00000000\n"),
            UnknownNode { line: 5, id: 1 },
        ),
        (
            format!("{b}layer,7,L,1\ninst,8,0,00000000,00000000\n"),
            UnknownLayer { line: 5, id: 8 },
        ),
        ("schema=1\nnode,1,-,a\n".into(), OutOfOrder { line: 2 }),
        (format!("{b}node,1,-,b\n"), OutOfOrder { line: 4 }),
        (
            format!("schema=1\nnode,0,-,a\nnode,1,-,b\nxform,1,{XF}\n"),
            OutOfOrder { line: 4 },
        ),
        (
            format!("{b}inst,7,0,00000000,00000000\nlayer,7,L,1\n"),
            OutOfOrder { line: 5 },
        ),
        ("schema=1\nnode,0,-,a\n".into(), MissingTransform { node: 0 }),
    ];
    for found in ["2", "0", "01", "1\r"] {
        cases.push((
            format!("schema={found}\n"),
            UnknownSchema {
                found: found.into(),
            },
        ));
    }
    cases.push((
        "schema=1\r\nnode,0,-,a\r\n".into(),
        UnknownSchema { found: "1\r".into() },
    ));
    for name in ["a%2c", "a%41", "a%", "a%2", "a ", "%20a", "a\r", "a %20"] {
        cases.push((
            format!("schema=1\nnode,0,-,{name}\nxform,0,{XF}\n"),
            BadEscape { line: 2 },
        ));
    }
    for value in ["0000000", "000000000", "+0000000"] {
        cases.push((
            b.replacen("00000000", value, 1),
            BadNumber { line: 3, field: 2 },
        ));
    }
    cases.push((
        b.replacen("00000000 00000000 00000000", "00000000 00000000", 1),
        BadNumber { line: 3, field: 2 },
    ));
    cases.push((
        b.replace("3f800000 3f800000 3f800000", "3F800000 3f800000 3f800000"),
        BadNumber { line: 3, field: 4 },
    ));
    for value in ["7f800000", "ff800000", "7fc00001"] {
        cases.push((
            b.replacen("00000000", value, 1),
            NonFinite { line: 3, field: 2 },
        ));
    }
    for rotation in [
        "00000000 00000000 00000000 40000000",
        "00000000 00000000 00000000 00000000",
    ] {
        cases.push((
            b.replace("00000000 00000000 00000000 3f800000", rotation),
            NotUnitQuat { line: 3 },
        ));
    }
    // This large hostile name still reaches the same escape validator.
    cases.push((
        format!("schema=1\nnode,0,-,{}%\n", "x".repeat(100_000)),
        BadEscape { line: 2 },
    ));
    for (input, expected) in cases {
        assert_eq!(read_scene(&input).unwrap_err(), expected, "input: {input:?}");
    }
}

fn two_nodes() -> SceneFile {
    let mut file = SceneFile::default();
    for parent in [None, Some(0)] {
        file.scene.hierarchy.nodes.push(Node {
            parent,
            mesh: None,
            transform: Transform::default(),
        });
    }
    file.node_names = vec!["a,b".into(), "100%".into()];
    file
}

#[test]
fn scene_file_write_refuses_unstorable_scenes() {
    use FormatError::*;
    let mut file = two_nodes();
    let expected = format!("schema=1\nnode,0,-,a%2Cb\nnode,1,0,100%25\nxform,0,{XF}\nxform,1,{XF}\n");
    assert_eq!(write_scene(&file).unwrap(), expected);
    let loaded = read_scene(&expected).unwrap();
    assert_eq!(loaded.node_names, file.node_names);
    assert_eq!(loaded.scene.hierarchy.nodes[0].parent, None);
    assert_eq!(loaded.scene.hierarchy.nodes[1].parent, Some(0));
    file.node_names = vec!["a\r".into(), "b  ".into()];
    let written = write_scene(&file).unwrap();
    assert!(written.contains("node,0,-,a%0D\nnode,1,0,b%20%20\n"));
    assert_eq!(read_scene(&written).unwrap().node_names, file.node_names);
    file.node_names.pop();
    assert_eq!(write_scene(&file), Err(NameCount { nodes: 2, names: 1 }));
    file.node_names.push("b".into());
    file.scene.meshes.push(Mesh::default());
    assert_eq!(write_scene(&file), Err(MeshNotStored { node: None }));
    file.scene.meshes.clear();
    file.scene.hierarchy.nodes[1].mesh = Some(0);
    assert_eq!(write_scene(&file), Err(MeshNotStored { node: Some(1) }));
    file.scene.hierarchy.nodes[1].mesh = None;
    file.scene.hierarchy.nodes[0].transform.translation.x = f32::NAN;
    assert!(matches!(write_scene(&file), Err(NonFinite { .. })));
    let mut file = two_nodes();
    file.scene.hierarchy.nodes.pop();
    file.node_names.pop();
    file.scene.instances.push(9, 0, 0.0, 0.0).unwrap();
    assert!(matches!(write_scene(&file), Err(UnknownLayer { id: 9, .. })));
    file.scene.layers.push(Layer {
        id: 9,
        name: "L".into(),
        visible: true,
    });
    file.scene.instances.push(9, 5, 0.0, 0.0).unwrap();
    assert!(matches!(write_scene(&file), Err(UnknownNode { id: 5, .. })));

    // Finite extremes and subnormals are stored, not numerically transformed.
    for bits in [1, 0x007fffff, 0x00800000, 0x7f7fffff, 0xff7fffff, 0x80000000] {
        let mut file = two_nodes();
        file.scene.hierarchy.nodes[0].transform.translation.x = f32::from_bits(bits);
        let loaded = read_scene(&write_scene(&file).unwrap()).unwrap();
        assert_eq!(
            loaded.scene.hierarchy.nodes[0]
                .transform
                .translation
                .x
                .to_bits(),
            bits
        );
    }
    let mut file = two_nodes();
    file.node_names = vec!["\u{03bb}\t% \n".into(), " ".repeat(100_000)];
    file.scene.hierarchy.nodes[0].parent = Some(1);
    file.scene.layers = vec![
        Layer {
            id: 7,
            name: "".into(),
            visible: false,
        },
        Layer {
            id: 7,
            name: "duplicate".into(),
            visible: true,
        },
    ];
    let loaded = read_scene(&write_scene(&file).unwrap()).unwrap();
    assert_eq!(loaded.node_names, file.node_names);
    assert_eq!(loaded.scene.hierarchy.nodes[0].parent, Some(1));
    assert_eq!(loaded.scene.layers.len(), 2);
}
