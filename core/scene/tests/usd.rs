use scene::hierarchy::HierarchyError;
use scene::layer::Layer;
use scene::math::{Mat4, Quat, Vec3};
use scene::scene::Scene;
use scene::transform::Transform;
use scene::usd::{compare_scenes, import_usda, rotate_xyz_degrees, UsdError, MAX_DEPTH};
use std::path::Path;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/usd/{name}"));
    std::fs::read_to_string(path).unwrap()
}
fn edit(text: &str, from: &str, to: &str) -> String {
    assert_eq!(text.matches(from).count(), 1, "unique edit: {from:?}");
    text.replacen(from, to, 1)
}
fn refuses(text: &str, expected: UsdError) {
    assert_eq!(import_usda(text).unwrap_err(), expected);
}
fn unsupported(line: usize, what: &str) -> UsdError {
    UsdError::Unsupported {
        line,
        what: what.into(),
    }
}
fn invalid(line: usize, what: &str) -> UsdError {
    UsdError::Invalid {
        line,
        what: what.into(),
    }
}
fn bits(v: Vec3, e: [f32; 3]) {
    assert_eq!([v.x, v.y, v.z].map(f32::to_bits), e.map(f32::to_bits));
}
fn close(actual: Vec3, expected: [f32; 3], tolerance: f32) {
    for (a, e) in [actual.x, actual.y, actual.z].into_iter().zip(expected) {
        assert!((a - e).abs() <= tolerance, "{actual:?} != {expected:?}");
    }
}
fn mismatch(a: &Scene, b: &Scene, what: &str) {
    let want = Err(UsdError::Mismatch { what: what.into() });
    assert_eq!(compare_scenes(a, b), want);
}
const ROOT: &str = "def Xform \"other\"";
const TRANSLATE: &str = "double3 xformOp:translate = (1, 2, 3)";
const SCALE: &str = "float3 xformOp:scale = (2, 2, 2)";
const ARM_ORDER: &str = "[\"xformOp:translate\", \"xformOp:rotateXYZ\"]";
const TRI_ORDER: &str = "[\"xformOp:scale\"]";
const ORIENT: &str = "(0.70710677, 0, 0.70710677, 0)";

#[test]
fn usd_import_hierarchy_fixture() {
    let scene = import_usda(&fixture("hierarchy.usda")).unwrap();
    let nodes = &scene.hierarchy.nodes;
    assert_eq!(nodes.len(), 5);
    assert_eq!(
        nodes.iter().map(|n| n.parent).collect::<Vec<_>>(),
        [None, Some(0), Some(1), Some(0), None]
    );
    assert_eq!(
        nodes.iter().map(|n| n.mesh).collect::<Vec<_>>(),
        [None, None, Some(0), None, None]
    );
    bits(nodes[0].transform.translation, [1.0, 2.0, 3.0]);
    bits(nodes[1].transform.translation, [0.5, 0.0, -0.25]);
    bits(nodes[2].transform.scale, [2.0; 3]);
    for node in &nodes[3..] {
        assert_eq!(node.transform, Transform::default());
    }
    for (a, e) in nodes[1]
        .transform
        .rotation
        .components()
        .into_iter()
        .zip([0.0, 0.0, 0.70710677, 0.70710677])
    {
        assert!((a - e).abs() <= 1e-7);
    }
    assert_eq!(scene.meshes.len(), 1);
    assert_eq!(
        scene.meshes[0].positions,
        [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(scene.meshes[0].indices, [0, 1, 2]);
    let world = scene.hierarchy.world_matrices().unwrap();
    close(
        world[2].transform_point3(Vec3::new(1.0, 0.0, 0.0)),
        [1.5, 4.0, 2.75],
        1e-5,
    );
    assert!(scene.layers.is_empty());
    assert!(scene.instances.nodes().is_empty());
    for text in ["#usda 1.0", "#usda 1.0\n"] {
        assert!(import_usda(text).unwrap().hierarchy.nodes.is_empty());
    }
    let child = "#usda 1.0\ndef Mesh \"m\" { point3f[] points = []\n\
        int[] faceVertexCounts = [] int[] faceVertexIndices = [] def Xform \"c\" {} }";
    let child = import_usda(child).unwrap();
    assert_eq!(child.hierarchy.nodes[1].parent, Some(0));
    assert_eq!(child.hierarchy.nodes[0].mesh, Some(0));
}

#[test]
fn usd_import_transforms_fixture() {
    let scene = import_usda(&fixture("transforms.usda")).unwrap();
    let nodes = &scene.hierarchy.nodes;
    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().all(|n| n.parent.is_none()));
    assert_eq!(nodes[2].mesh, Some(0));
    bits(nodes[0].transform.translation, [-1.5, 0.125, 1000.0]);
    bits(nodes[0].transform.scale, [1.0, 2.0, 0.5]);
    assert_eq!(nodes[0].transform.rotation.components(), [0.5; 4]);
    let world = scene.hierarchy.world_matrices().unwrap();
    for (col, expected) in [
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 2.0],
        [0.5, 0.0, 0.0],
        [-1.5, 0.125, 1000.0],
    ]
    .into_iter()
    .enumerate()
    {
        close(
            Vec3::new(world[0].m[0][col], world[0].m[1][col], world[0].m[2][col]),
            expected,
            1e-6,
        );
    }
    close(
        world[1].transform_dir(Vec3::new(1.0, 0.0, 0.0)),
        [0.0, 0.0, -1.0],
        1e-6,
    );
    assert_eq!(scene.meshes.len(), 1);
    assert_eq!(
        scene.meshes[0].positions,
        [[0.0; 3], [1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]]
    );
    assert_eq!(scene.meshes[0].indices, [0, 1, 2, 0, 2, 3]);
}

#[test]
fn usd_rotate_xyz_applies_x_then_y_then_z() {
    let matrix = |angles| {
        Mat4::from_trs(
            Vec3::default(),
            rotate_xyz_degrees(angles).unwrap(),
            Vec3::new(1.0, 1.0, 1.0),
        )
        .unwrap()
    };
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = Vec3::new(0.0, 1.0, 0.0);
    for (angles, input, expected) in [
        ([90.0, 0.0, 0.0], y, [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 90.0], x, [0.0, 1.0, 0.0]),
        ([90.0, 0.0, 90.0], y, [0.0, 0.0, 1.0]),
        ([90.0, 0.0, 90.0], x, [0.0, 1.0, 0.0]),
        ([0.0, 90.0, 0.0], x, [0.0, 0.0, -1.0]),
    ] {
        close(matrix(angles).transform_dir(input), expected, 1e-6);
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for axis in 0..3 {
            let mut angles = [0.0; 3];
            angles[axis] = value;
            assert!(rotate_xyz_degrees(angles).is_none());
        }
    }
    // Independent f64 reference: rotate a vector by three axis matrices, not quaternions.
    for angles in [
        [f32::from_bits(1), -f32::from_bits(1), 0.0],
        [f32::MIN_POSITIVE; 3],
        [f32::MAX, -f32::MAX, f32::MAX],
        [180.0, f32::from_bits(180.0_f32.to_bits() + 1), -180.0],
    ] {
        let [(sx, cx), (sy, cy), (sz, cz)] = angles.map(|d| f64::from(d).to_radians().sin_cos());
        for input in [x, y, Vec3::new(0.0, 0.0, 1.0)] {
            let [a, b, c] = [input.x, input.y, input.z].map(f64::from);
            let (b, c) = (cx * b - sx * c, sx * b + cx * c);
            let (a, c) = (cy * a + sy * c, -sy * a + cy * c);
            let expected = [cz * a - sz * b, sz * a + cz * b, c].map(|v| v as f32);
            close(matrix(angles).transform_dir(input), expected, 1e-6);
        }
    }
}

#[test]
fn usd_refuses_unsupported_features() {
    let text = fixture("hierarchy.usda");
    let reject = |from: &str, to: &str, line: usize, what: &str| {
        refuses(&edit(&text, from, to), unsupported(line, what))
    };
    reject(ROOT, "def Cube \"other\"", 29, "Cube");
    reject(ROOT, "over Xform \"other\"", 29, "over");
    reject(ROOT, "class Xform \"other\"", 29, "class");
    reject(ROOT, "# note\ndef Xform \"other\"", 29, "#");
    reject(
        "def Xform \"other\"\n{",
        "def Xform \"other\" (\n    kind = \"component\"\n)\n{",
        29,
        "(",
    );
    reject(
        "float3 xformOp:rotateXYZ",
        "float3 xformOp:rotateZYX",
        11,
        "xformOp:rotateZYX",
    );
    reject(
        TRANSLATE,
        "float xformOp:translate = (1, 2, 3)",
        5,
        "float xformOp:translate",
    );
    reject(
        TRANSLATE,
        "point3f[] points = [(0, 0, 0)]",
        5,
        "points on Xform",
    );
    reject("[3]", "[4]", 14, "faceVertexCounts");
    reject(TRI_ORDER, "[\"xformOp:rotateX\"]", 14, "xformOp:rotateX");
    reject(TRI_ORDER, "[\"]\"]", 14, "]");
    let both = edit(
        &text,
        "float3 xformOp:rotateXYZ = (0, 0, 90)",
        "float3 xformOp:rotateXYZ = (0, 0, 90)\n        quatf xformOp:orient = (1, 0, 0, 0)",
    );
    refuses(
        &edit(
            &both,
            ARM_ORDER,
            "[\"xformOp:translate\", \"xformOp:rotateXYZ\", \"xformOp:orient\"]",
        ),
        unsupported(8, "rotateXYZ and orient"),
    );
    reject(
        ARM_ORDER,
        "[\"xformOp:rotateXYZ\", \"xformOp:translate\"]",
        8,
        "xformOpOrder order",
    );
    reject(
        "uniform token[] xformOpOrder = [\"xformOp:scale\"]",
        "token[] xformOpOrder = [\"xformOp:scale\"]",
        17,
        "token[] xformOpOrder",
    );
    refuses("#usda 1.0\n(\n)", unsupported(2, "("));
    // Unknown attributes are rejected before their (possibly hostile) values are lexed.
    reject(TRANSLATE, "float3 mystery = @", 5, "mystery");
}

#[test]
fn usd_refuses_invalid_files() {
    let text = fixture("hierarchy.usda");
    refuses(&edit(&text, "#usda 1.0", "#usda 1.1"), UsdError::Header);
    refuses(&text.replace('\n', "\r\n"), UsdError::Header);
    let reject = |from: &str, to: &str, line: usize, what: &str| {
        refuses(&edit(&text, from, to), invalid(line, what))
    };
    reject(
        "def Xform \"empty\"",
        "def Xform \"arm\"",
        24,
        "duplicate prim name",
    );
    reject("\"other\"", "\"9other\"", 29, "bad prim name");
    reject("[0, 1, 2]", "[0, 1, 3]", 14, "faceVertexIndices range");
    reject("[0, 1, 2]", "[0, 1]", 14, "faceVertexIndices length");
    for value in [
        "[0, -1, 2]",
        "[0, 4294967296, 2]",
        "[0, 1.0, 2]",
        "[0, 1e0, 2]",
    ] {
        reject("[0, 1, 2]", value, 19, "bad integer");
    }
    // All three required mesh attributes are checked independently.
    for (line, what) in [
        (
            "point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]",
            "missing points",
        ),
        ("int[] faceVertexCounts = [3]", "missing faceVertexCounts"),
        (
            "int[] faceVertexIndices = [0, 1, 2]",
            "missing faceVertexIndices",
        ),
    ] {
        reject(&format!("            {line}\n"), "", 14, what);
    }
    reject(TRI_ORDER, "[]", 14, "op not in xformOpOrder");
    reject(
        "[\"xformOp:translate\"]",
        "[\"xformOp:translate\", \"xformOp:scale\"]",
        3,
        "xformOpOrder names a missing op",
    );
    reject(
        TRI_ORDER,
        "[\"xformOp:scale\", \"xformOp:scale\"]",
        14,
        "duplicate op in xformOpOrder",
    );
    reject(
        SCALE,
        &format!("{SCALE}\n            {SCALE}"),
        17,
        "duplicate xformOp:scale",
    );
    refuses(
        &edit(&text, "(1, 2, 3)", "(1, 2, 1e39)"),
        UsdError::NonFinite { line: 5 },
    );
    let syntax = |from: &str, to: &str, line: usize| {
        refuses(&edit(&text, from, to), UsdError::Syntax { line })
    };
    for value in ["nan", "+1", ".5", "1.", "1e", "1\r", "1\u{b}"] {
        syntax("(1, 2, 3)", &format!("(1, 2, {value})"), 5);
    }
    syntax(ROOT, "@def Xform \"other\"", 29);
    syntax("def Xform \"other\"\n{", ROOT, 30);
    refuses(
        &edit(&fixture("transforms.usda"), ORIENT, "(0, 0, 0, 0)"),
        invalid(11, "zero orient"),
    );
    let overflow = edit(
        &edit(&text, "(1, 2, 3)", "(3e38, 0, 0)"),
        "(0.5, 0, -0.25)",
        "(3e38, 0, 0)",
    );
    refuses(
        &overflow,
        UsdError::Hierarchy(HierarchyError::InvalidTransform { node: 1 }),
    );
    assert_eq!(MAX_DEPTH, 128);
    for depth in [MAX_DEPTH, MAX_DEPTH + 1] {
        let mut nested = String::from("#usda 1.0\n");
        for i in 0..depth {
            nested.push_str(&format!("def Xform \"n{i}\" {{\n"));
        }
        nested.push_str(&"}\n".repeat(depth));
        if depth == MAX_DEPTH {
            assert_eq!(
                import_usda(&nested).unwrap().hierarchy.nodes.len(),
                MAX_DEPTH
            );
        } else {
            refuses(&nested, UsdError::TooDeep { line: 130 });
        }
    }
    for name in ["", "a:b", "a/b", "C:drive"] {
        refuses(
            &format!("#usda 1.0\ndef Xform \"{name}\" {{}}"),
            invalid(2, "bad prim name"),
        );
    }
    for tail in [
        "}",
        "def",
        "def Xform \"a",
        "def Xform \"a\\b\" {}",
        "def Xform \"a\" {",
        "def Xform \"a\" { (",
    ] {
        refuses(&format!("#usda 1.0\n{tail}"), UsdError::Syntax { line: 2 });
    }
    // Flat size does not consume the nesting budget.
    let mut flat = String::from("#usda 1.0\n");
    for i in 0..4096 {
        flat.push_str(&format!("def Xform \"p{i}\" {{}}\n"));
    }
    assert_eq!(import_usda(&flat).unwrap().hierarchy.nodes.len(), 4096);
    let hostile = format!(
        "#usda 1.0\ndef Xform \"a\" {{ float3 unknown = \"{}",
        "x".repeat(65536)
    );
    refuses(&hostile, unsupported(2, "unknown"));
    for value in [f32::from_bits(1), f32::MIN_POSITIVE, f32::MAX] {
        let parsed = import_usda(&edit(&text, "(1, 2, 3)", &format!("({value}, 0, 0)"))).unwrap();
        let got = parsed.hierarchy.nodes[0].transform.translation.x.to_bits();
        assert_eq!(got, value.to_bits());
    }
}

#[test]
fn usd_compare_names_first_difference() {
    let text = fixture("hierarchy.usda");
    let a = import_usda(&text).unwrap();
    assert_eq!(compare_scenes(&a, &import_usda(&text).unwrap()), Ok(()));
    for (from, to, what) in [
        ("(1, 2, 3)", "(1, 2, 3.5)", "node 0 translation"),
        ("(1, 0, 0)", "(1, 0, 0.5)", "mesh 0 positions"),
        ("(0.5, 0, -0.25)", "(0.5, -0, -0.25)", "node 1 translation"),
    ] {
        mismatch(&a, &import_usda(&edit(&text, from, to)).unwrap(), what);
    }
    mismatch(
        &a,
        &import_usda(&fixture("transforms.usda")).unwrap(),
        "node count",
    );
    let mut b = import_usda(&text).unwrap();
    b.hierarchy.nodes[0].parent = Some(1);
    b.hierarchy.nodes[0].mesh = Some(0);
    mismatch(&a, &b, "node 0 parent");
    b.hierarchy.nodes[0].parent = None;
    mismatch(&a, &b, "node 0 mesh");
    b.hierarchy.nodes[0].mesh = None;
    b.hierarchy.nodes[0].transform.rotation = Quat::new(-0.0, 0.0, 0.0, 1.0).unwrap();
    mismatch(&a, &b, "node 0 rotation");
    b.hierarchy.nodes[0].transform.rotation = Quat::IDENTITY;
    b.hierarchy.nodes[0].transform.scale.x = -0.0;
    mismatch(&a, &b, "node 0 scale");
    b.hierarchy.nodes[0].transform.scale.x = 1.0;
    b.meshes.push(Default::default());
    mismatch(&a, &b, "mesh count");
    b.meshes.truncate(1);
    b.meshes[0].positions[0][0] = -0.0;
    mismatch(&a, &b, "mesh 0 positions");
    b.meshes[0].positions[0][0] = 0.0;
    b.meshes[0].indices.swap(0, 1);
    mismatch(&a, &b, "mesh 0 indices");
    b.meshes[0].indices.swap(0, 1);
    assert_eq!(compare_scenes(&a, &b), Ok(()));
    b.layers.push(Layer {
        id: 0,
        name: "test".into(),
        visible: true,
    });
    mismatch(&a, &b, "layers");
    b.layers.clear();
    b.instances.push(0, 0, 0.0, 0.0).unwrap();
    mismatch(&a, &b, "instances");
    for (layer, node, angle, speed) in [
        (1, 0, 0.0, 0.0),
        (0, 1, 0.0, 0.0),
        (0, 0, -0.0, 0.0),
        (0, 0, 0.0, -0.0),
    ] {
        let mut a = import_usda(&text).unwrap();
        let mut b = import_usda(&text).unwrap();
        a.instances.push(0, 0, 0.0, 0.0).unwrap();
        b.instances.push(layer, node, angle, speed).unwrap();
        mismatch(&a, &b, "instances");
    }
}
