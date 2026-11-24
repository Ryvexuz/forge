use scene::gltf::{import_gltf, GltfError};
use scene::hierarchy::HierarchyError;
use scene::math::{Quat, Vec3};
use scene::scene::Scene;
use scene::transform::Transform;
use std::path::Path;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/gltf/{name}"));
    std::fs::read(path).unwrap()
}

fn hierarchy_text() -> String {
    String::from_utf8(fixture("hierarchy.gltf")).unwrap()
}

fn edit(text: &str, from: &str, to: &str) -> String {
    let edited = text.replacen(from, to, 1);
    assert_ne!(text, edited, "edit must change {from:?}");
    edited
}

fn import(text: &str, bin_uri: &str, bin: &[u8]) -> Result<Scene, GltfError> {
    import_gltf(text.as_bytes(), bin_uri, bin)
}

fn refuses(text: &str, bin: &[u8], expected: GltfError) {
    assert_eq!(import(text, "hierarchy.bin", bin).unwrap_err(), expected);
}

fn bad_accessor(accessor: usize, reason: &'static str) -> GltfError {
    GltfError::BadAccessor { accessor, reason }
}

fn hierarchy_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in [-0.5_f32, 0.0, 0.25, 0.5, 0.0, 0.25, 0.0, 1.0, -0.75] {
        bytes.extend(n.to_le_bytes());
    }
    for n in [1_u16, 2, 0] {
        bytes.extend(n.to_le_bytes());
    }
    bytes.extend([0, 0]);
    bytes
}

fn instances_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for n in [
        0.0_f32, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0,
    ] {
        bytes.extend(n.to_le_bytes());
    }
    for n in [0_u32, 1, 2, 0, 2, 3] {
        bytes.extend(n.to_le_bytes());
    }
    bytes
}

fn instances_text() -> String {
    let mut text = String::from(
        "{\n  \"asset\": {\"version\": \"2.0\", \"generator\": \"broadsword forge fixture\"},\n  \"scene\": 0,\n",
    );
    let roots = (0..100)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    text.push_str(&format!(
        "  \"scenes\": [{{\"nodes\": [{roots}]}}],\n  \"nodes\": [\n"
    ));
    for k in 0..100 {
        text.push_str(&format!(
            "    {{\"name\": \"i{:02}\", \"mesh\": 0, \"translation\": [{}.0, 0.0, {}.0]}}{}\n",
            k,
            2 * (k % 10),
            2 * (k / 10),
            if k < 99 { "," } else { "" }
        ));
    }
    text.push_str(
        r#"  ],
  "meshes": [
    {"name": "quad", "primitives": [{"attributes": {"POSITION": 0}, "indices": 1, "mode": 4}]}
  ],
  "accessors": [
    {"bufferView": 0, "byteOffset": 0, "componentType": 5126, "count": 4, "type": "VEC3", "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]},
    {"bufferView": 1, "byteOffset": 0, "componentType": 5125, "count": 6, "type": "SCALAR"}
  ],
  "bufferViews": [
    {"buffer": 0, "byteOffset": 0, "byteLength": 48, "target": 34962},
    {"buffer": 0, "byteOffset": 48, "byteLength": 24, "target": 34963}
  ],
  "buffers": [
    {"uri": "instances.bin", "byteLength": 72}
  ]
}
"#,
    );
    text
}

fn assert_positions(actual: &[[f32; 3]], expected: &[[f32; 3]]) {
    assert_eq!(
        actual
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>(),
        expected
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect::<Vec<_>>()
    );
}

fn minimal(node: &str) -> String {
    format!(r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"nodes":[{node}]}}"#)
}

fn matrix_doc(a: [f32; 16]) -> String {
    let values = a.map(|v| v.to_string()).join(",");
    minimal(&format!(r#"{{"matrix":[{values}]}}"#))
}

#[test]
fn gltf_fixtures_match_their_tables() {
    let h = hierarchy_bytes();
    let i = instances_bytes();
    assert_eq!(h.len(), 44);
    assert_eq!(i.len(), 72);
    assert_eq!(h, fixture("hierarchy.bin"));
    assert_eq!(i, fixture("instances.bin"));
    assert_eq!(instances_text().as_bytes(), fixture("instances.gltf"));
}

#[test]
fn gltf_import_hierarchy_fixture() {
    let scene = import(
        &hierarchy_text(),
        "hierarchy.bin",
        &fixture("hierarchy.bin"),
    )
    .unwrap();
    let nodes = &scene.hierarchy.nodes;
    assert_eq!(nodes.len(), 3);
    assert_eq!(
        nodes.iter().map(|n| n.parent).collect::<Vec<_>>(),
        [None, Some(0), Some(0)]
    );
    assert_eq!(
        nodes.iter().map(|n| n.mesh).collect::<Vec<_>>(),
        [None, Some(0), Some(0)]
    );
    let tables = [
        ([1.0, 2.0, 3.0], [0.5, 0.5, 0.5, 0.5], [2.0, 2.0, 2.0]),
        ([-1.5, 0.0, 0.0], [0.5, -0.5, 0.5, 0.5], [1.0, 0.5, 0.25]),
        ([1.5, 0.0, 0.0], [-0.5, 0.5, 0.5, 0.5], [0.5, 0.5, 0.5]),
    ];
    for (node, (t, q, s)) in nodes.iter().zip(tables) {
        assert_eq!(
            node.transform,
            Transform {
                translation: Vec3::new(t[0], t[1], t[2]),
                rotation: Quat::new(q[0], q[1], q[2], q[3]).unwrap(),
                scale: Vec3::new(s[0], s[1], s[2]),
            }
        );
    }
    assert_eq!(scene.meshes.len(), 1);
    assert_positions(
        &scene.meshes[0].positions,
        &[[-0.5, 0.0, 0.25], [0.5, 0.0, 0.25], [0.0, 1.0, -0.75]],
    );
    assert_eq!(scene.meshes[0].indices, [1, 2, 0]);
    assert!(scene.instances.matrices().is_empty());
    assert!(scene.layers.is_empty());
    assert!(scene.hierarchy.world_matrices().is_ok());
}

#[test]
fn gltf_import_instances_fixture() {
    let scene = import_gltf(
        &fixture("instances.gltf"),
        "instances.bin",
        &fixture("instances.bin"),
    )
    .unwrap();
    assert_eq!(scene.hierarchy.nodes.len(), 100);
    for (k, node) in scene.hierarchy.nodes.iter().enumerate() {
        assert_eq!(node.parent, None);
        assert_eq!(node.mesh, Some(0));
        assert_eq!(
            node.transform,
            Transform {
                translation: Vec3::new((2 * (k % 10)) as f32, 0.0, (2 * (k / 10)) as f32),
                rotation: Quat::IDENTITY,
                scale: Vec3::new(1.0, 1.0, 1.0),
            }
        );
    }
    assert_eq!(scene.meshes.len(), 1);
    assert_positions(
        &scene.meshes[0].positions,
        &[
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
        ],
    );
    assert_eq!(scene.meshes[0].indices, [0, 1, 2, 0, 2, 3]);
    assert!(scene.instances.matrices().is_empty());
    assert!(scene.layers.is_empty());
}

#[test]
fn gltf_matrix_decomposes_and_skew_is_refused() {
    let a = [
        0.0, 2.0, 0.0, 0.0, -3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 4.0, 0.0, 1.0, 2.0, 3.0, 1.0,
    ];
    let scene = import(&matrix_doc(a), "m.bin", &[]).unwrap();
    let t = scene.hierarchy.nodes[0].transform;
    assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(t.scale, Vec3::new(2.0, 3.0, 4.0));
    let m = t.to_matrix().unwrap();
    for (r, row) in m.m.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            assert!((f64::from(*value) - f64::from(a[c * 4 + r])).abs() <= 1e-6);
        }
    }
    let identity = [
        1.0, 0.0, 0.0, 0.0, // column x
        0.0, 1.0, 0.0, 0.0, // column y
        0.0, 0.0, 1.0, 0.0, // column z
        0.0, 0.0, 0.0, 1.0, // translation
    ];
    for (slot, value) in [(4, 1.0), (0, -1.0), (3, 0.5), (0, 0.0)] {
        let mut a = identity;
        a[slot] = value;
        assert_eq!(
            import(&matrix_doc(a), "m.bin", &[]).unwrap_err(),
            GltfError::Skew { node: 0 }
        );
    }
    let text = edit(
        &matrix_doc(identity),
        r#""matrix":["#,
        r#""translation":[0,0,0],"matrix":["#,
    );
    assert_eq!(
        import(&text, "m.bin", &[]).unwrap_err(),
        GltfError::Invalid("nodes[0].matrix with TRS".into())
    );
    // Independent f64 references: diagonal matrices remain exact at both f32 extremes.
    for s in [f32::from_bits(1), f32::MIN_POSITIVE, f32::MAX] {
        let mut a = identity;
        for k in [0, 5, 10] {
            a[k] = s;
        }
        let scene = import(&matrix_doc(a), "m.bin", &[]).unwrap();
        let t = scene.hierarchy.nodes[0].transform;
        assert_eq!(t.scale, Vec3::new(s, s, s));
        let m = t.to_matrix().unwrap();
        for (r, row) in m.m.iter().enumerate() {
            for (c, value) in row.iter().enumerate() {
                assert_eq!(f64::from(*value), f64::from(a[c * 4 + r]));
            }
        }
    }
    // Exercise each largest-component branch and near-cancelling column dot products.
    for signs in [[1.0, -1.0, -1.0], [-1.0, 1.0, -1.0], [-1.0, -1.0, 1.0]] {
        let mut a = identity;
        for (k, sign) in [0, 5, 10].into_iter().zip(signs) {
            a[k] = sign;
        }
        let scene = import(&matrix_doc(a), "m.bin", &[]).unwrap();
        let m = scene.hierarchy.nodes[0].transform.to_matrix().unwrap();
        for (r, row) in m.m.iter().enumerate() {
            for (c, value) in row.iter().enumerate() {
                assert_eq!(*value, a[c * 4 + r]);
            }
        }
    }
    let mut a = identity;
    let u = std::f32::consts::FRAC_1_SQRT_2;
    a[0] = u;
    a[1] = u;
    a[4] = -u;
    a[5] = u;
    let scene = import(&matrix_doc(a), "m.bin", &[]).unwrap();
    let m = scene.hierarchy.nodes[0].transform.to_matrix().unwrap();
    for (r, row) in m.m.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            assert!((f64::from(*value) - f64::from(a[c * 4 + r])).abs() <= 1e-6);
        }
    }
    a[0] = f32::MAX;
    a[1] = f32::MAX;
    a[4] = -f32::MAX;
    a[5] = f32::MAX;
    assert_eq!(
        import(&matrix_doc(a), "m.bin", &[]).unwrap_err(),
        GltfError::Invalid("nodes[0].matrix".into())
    );
}

#[test]
fn gltf_refuses_unsupported_features() {
    let text = hierarchy_text();
    let bin = fixture("hierarchy.bin");
    for key in [
        "animations",
        "skins",
        "cameras",
        "materials",
        "extensionsUsed",
    ] {
        let edited = edit(
            &text,
            "\"scene\": 0,",
            &format!("\"scene\": 0,\n  \"{key}\": [],"),
        );
        refuses(&edited, &bin, GltfError::Unsupported(key.into()));
    }
    for (from, to, path) in [
        (
            r#"{"name": "left", "#,
            r#"{"name": "left", "camera": 0, "#,
            "nodes[1].camera",
        ),
        (
            r#""mode": 4}"#,
            r#""mode": 4, "material": 0}"#,
            "meshes[0].primitives[0].material",
        ),
        (
            r#""mode": 4"#,
            r#""mode": 1"#,
            "meshes[0].primitives[0].mode=1",
        ),
        (
            r#"{"POSITION": 0}"#,
            r#"{"POSITION": 0, "NORMAL": 0}"#,
            "meshes[0].primitives[0].attributes.NORMAL",
        ),
        (
            r#""type": "VEC3", "#,
            r#""type": "VEC3", "sparse": {}, "#,
            "accessors[0].sparse",
        ),
        (
            r#""mode": 4}]"#,
            r#""mode": 4}, {"attributes": {"POSITION": 0}, "indices": 1}]"#,
            "meshes[0].primitives.len=2",
        ),
        (
            r#"[{"nodes": [0]}]"#,
            r#"[{"nodes": [0]}, {"nodes": [0]}]"#,
            "scenes.len=2",
        ),
        (
            r#""byteLength": 36, "#,
            r#""byteLength": 36, "byteStride": 24, "#,
            "bufferViews[0].byteStride=24",
        ),
        (
            r#""count": 3, "type": "SCALAR""#,
            r#""count": 3, "type": "SCALAR", "normalized": true"#,
            "accessors[1].normalized=true",
        ),
        (
            r#""indices": 1, "#,
            "",
            "meshes[0].primitives[0] without indices",
        ),
        (
            r#"{"bufferView": 0, "byteOffset": 0, "componentType": 5126"#,
            r#"{"byteOffset": 0, "componentType": 5126"#,
            "accessors[0] without bufferView",
        ),
        (
            r#"{"uri": "hierarchy.bin", "#,
            "{",
            "buffers[0] without uri",
        ),
        (
            r#"{"uri": "hierarchy.bin", "byteLength": 44}"#,
            r#"{"uri": "hierarchy.bin", "byteLength": 44}, {"uri": "hierarchy.bin", "byteLength": 44}"#,
            "buffers.len=2",
        ),
        (
            r#""version": "2.0""#,
            r#""version": "1.0""#,
            "asset.version=1.0",
        ),
    ] {
        refuses(
            &edit(&text, from, to),
            &bin,
            GltfError::Unsupported(path.into()),
        );
    }
}

#[test]
fn gltf_refuses_bad_buffer_uris_and_lengths() {
    let text = hierarchy_text();
    let bin = fixture("hierarchy.bin");
    for uri in [
        "data:application/octet-stream;base64,AAAA",
        "../hierarchy.bin",
        "sub/hierarchy.bin",
        "C:hierarchy.bin",
        "hierarchy%2Ebin",
        "other.bin",
        r"sub\hierarchy.bin",
        "a\r.bin",
        "a\n.bin",
    ] {
        let escaped = uri
            .replace('\\', "\\\\")
            .replace('\r', "\\r")
            .replace('\n', "\\n");
        refuses(
            &edit(&text, "hierarchy.bin", &escaped),
            &bin,
            GltfError::BufferUri(uri.into()),
        );
    }
    for uri in [
        "../hierarchy.bin",
        "",
        ".hidden.bin",
        "no.bin/",
        "C:x.bin",
        "a\\b.bin",
        "a%.bin",
        "a\r.bin",
        "a\n.bin",
        "\u{e9}.bin",
        "wrong.BIN",
        "a..bin",
    ] {
        assert_eq!(
            import(&text, uri, &bin).unwrap_err(),
            GltfError::BufferUri(uri.into())
        );
    }
    let too_long = format!("{}.bin", "a".repeat(252));
    assert_eq!(
        import(&text, &too_long, &bin).unwrap_err(),
        GltfError::BufferUri(too_long)
    );
    refuses(
        &text,
        &bin[..43],
        GltfError::BinLength {
            declared: 44,
            actual: 43,
        },
    );
    let mut longer = bin;
    longer.extend([0; 4]);
    refuses(
        &text,
        &longer,
        GltfError::BinLength {
            declared: 44,
            actual: 48,
        },
    );
    assert_eq!(
        import(&minimal("{}"), "m.bin", &[0; 4]).unwrap_err(),
        GltfError::BinLength {
            declared: 0,
            actual: 4,
        }
    );
    let uri = format!("{}.bin", "a".repeat(251));
    assert!(import(&edit(&text, "hierarchy.bin", &uri), &uri, &longer[..44]).is_ok());
}

#[test]
fn gltf_refuses_out_of_range_accessors() {
    let text = hierarchy_text();
    let bin = fixture("hierarchy.bin");
    for count in ["4", "4294967295"] {
        let edited = edit(
            &text,
            r#""count": 3, "type": "VEC3""#,
            &format!(r#""count": {count}, "type": "VEC3""#),
        );
        refuses(&edited, &bin, GltfError::OutOfRange("accessors[0]".into()));
    }
    let edited = edit(
        &text,
        r#""count": 3, "type": "VEC3""#,
        r#""count": 9007199254740993, "type": "VEC3""#,
    );
    refuses(
        &edited,
        &bin,
        GltfError::Invalid("accessors[0].count".into()),
    );
    for (from, to, expected) in [
        (
            r#""byteOffset": 0, "componentType": 5126"#,
            r#""byteOffset": 4, "componentType": 5126"#,
            GltfError::OutOfRange("accessors[0]".into()),
        ),
        (
            r#""byteOffset": 36, "byteLength": 6"#,
            r#""byteOffset": 40, "byteLength": 6"#,
            GltfError::OutOfRange("bufferViews[1]".into()),
        ),
        (
            r#""byteOffset": 36, "byteLength": 6"#,
            r#""byteOffset": 37, "byteLength": 6"#,
            bad_accessor(1, "misaligned"),
        ),
        (
            r#""count": 3, "type": "SCALAR""#,
            r#""count": 2, "type": "SCALAR""#,
            bad_accessor(1, "index count not a multiple of 3"),
        ),
        (
            r#""componentType": 5123"#,
            r#""componentType": 5126"#,
            bad_accessor(
                1,
                "index accessor must be SCALAR UNSIGNED_BYTE, UNSIGNED_SHORT or UNSIGNED_INT",
            ),
        ),
        (
            r#""componentType": 5126"#,
            r#""componentType": 5123"#,
            bad_accessor(0, "position accessor must be VEC3 FLOAT"),
        ),
        (
            r#""count": 3, "type": "VEC3""#,
            r#""count": 0, "type": "VEC3""#,
            bad_accessor(0, "count is zero"),
        ),
        (
            r#""POSITION": 0"#,
            r#""POSITION": 2"#,
            GltfError::BadIndex {
                kind: "accessor",
                index: 2,
            },
        ),
        (
            r#""bufferView": 0"#,
            r#""bufferView": 2"#,
            GltfError::BadIndex {
                kind: "bufferView",
                index: 2,
            },
        ),
        (
            r#""buffer": 0"#,
            r#""buffer": 1"#,
            GltfError::BadIndex {
                kind: "buffer",
                index: 1,
            },
        ),
    ] {
        refuses(&edit(&text, from, to), &bin, expected);
    }
    let mut invalid_index = bin.clone();
    invalid_index[36] = 3;
    refuses(
        &text,
        &invalid_index,
        bad_accessor(1, "index >= vertex count"),
    );
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid_position = bin.clone();
        invalid_position[..4].copy_from_slice(&value.to_le_bytes());
        refuses(
            &text,
            &invalid_position,
            bad_accessor(0, "non-finite position"),
        );
    }
    // UNSIGNED_BYTE has the same widened values, with a shorter accessor span.
    let edited = edit(
        &text,
        r#""componentType": 5123"#,
        r#""componentType": 5121"#,
    );
    let mut bytes = bin;
    bytes[36..39].copy_from_slice(&[1, 2, 0]);
    assert_eq!(
        import(&edited, "hierarchy.bin", &bytes).unwrap().meshes[0].indices,
        [1, 2, 0]
    );
    for value in [f32::from_bits(1), f32::MAX, -0.0] {
        bytes[..4].copy_from_slice(&value.to_le_bytes());
        let scene = import(&edited, "hierarchy.bin", &bytes).unwrap();
        assert_eq!(scene.meshes[0].positions[0][0].to_bits(), value.to_bits());
    }
    let text = edit(
        &text,
        r#""byteLength": 36, "#,
        r#""byteLength": 36, "byteStride": 12, "#,
    );
    assert!(import(&text, "hierarchy.bin", &fixture("hierarchy.bin")).is_ok());
    let mut bytes = fixture("instances.bin");
    bytes[48..52].copy_from_slice(&65536_u32.to_le_bytes());
    assert_eq!(
        import_gltf(&fixture("instances.gltf"), "instances.bin", &bytes).unwrap_err(),
        bad_accessor(1, "index >= vertex count")
    );
}

#[test]
fn gltf_refuses_malformed_structure() {
    assert_eq!(
        import_gltf(&[0xff], "m.bin", &[]).unwrap_err(),
        GltfError::Utf8
    );
    let json_error = scene::json::read("{").unwrap_err();
    assert_eq!(
        import("{", "m.bin", &[]).unwrap_err(),
        GltfError::Json(json_error)
    );
    assert_eq!(
        import("[]", "m.bin", &[]).unwrap_err(),
        GltfError::Invalid("document".into())
    );
    let text = hierarchy_text();
    let bin = fixture("hierarchy.bin");
    for (from, to, expected) in [
        (
            "  \"asset\": {\"version\": \"2.0\", \"generator\": \"broadsword forge fixture\"},\n",
            "",
            GltfError::Invalid("asset".into()),
        ),
        (
            r#""scene": 0,"#,
            "\"scene\": 0,\n  \"scene\": 0,",
            GltfError::Invalid("scene duplicate".into()),
        ),
        (
            r#""name": "left", "mesh": 0"#,
            r#""name": "left", "mesh": 0.5"#,
            GltfError::Invalid("nodes[1].mesh".into()),
        ),
        (
            r#""name": "left", "mesh": 0"#,
            r#""name": "left", "mesh": 1"#,
            GltfError::BadIndex {
                kind: "mesh",
                index: 1,
            },
        ),
        (
            r#""children": [1, 2]"#,
            r#""children": [1, 9]"#,
            GltfError::BadIndex {
                kind: "node",
                index: 9,
            },
        ),
        (
            r#""children": [1, 2]"#,
            r#""children": [1, 2, 2]"#,
            GltfError::Invalid("nodes[2] has two parents".into()),
        ),
        (
            r#""children": [1, 2]"#,
            r#""children": [1]"#,
            GltfError::Invalid("nodes[2] is a root not in the scene".into()),
        ),
        (
            r#""rotation": [0.5, 0.5, 0.5, 0.5]"#,
            r#""rotation": [0.0, 0.0, 0.0, 0.0]"#,
            GltfError::Invalid("nodes[0].rotation".into()),
        ),
        (
            r#""translation": [1.0, 2.0, 3.0]"#,
            r#""translation": [1.0, 2.0]"#,
            GltfError::Invalid("nodes[0].translation".into()),
        ),
        (
            r#""translation": [1.0, 2.0, 3.0]"#,
            r#""translation": [1e39, 2.0, 3.0]"#,
            GltfError::Invalid("nodes[0].translation".into()),
        ),
        (
            r#""scene": 0"#,
            r#""scene": 1"#,
            GltfError::BadIndex {
                kind: "scene",
                index: 1,
            },
        ),
        (
            r#"[{"nodes": [0]}]"#,
            r#"[{"nodes": [0, 1]}]"#,
            GltfError::Invalid("scenes[0].nodes[1] has a parent".into()),
        ),
    ] {
        refuses(&edit(&text, from, to), &bin, expected);
    }
    let cycle = edit(&text, r#""children": [1, 2], "#, "");
    let cycle = edit(
        &cycle,
        r#"{"name": "left", "#,
        r#"{"name": "left", "children": [2], "#,
    );
    let cycle = edit(
        &cycle,
        r#"{"name": "right", "#,
        r#"{"name": "right", "children": [1], "#,
    );
    refuses(&cycle, &bin, GltfError::Hierarchy(HierarchyError::Cycle));
    for value in ["-1", "0.5", "4294967296", "1e300", "null", "true", "\"0\""] {
        let edited = edit(
            &text,
            r#""count": 3, "type": "VEC3""#,
            &format!(r#""count": {value}, "type": "VEC3""#),
        );
        refuses(
            &edited,
            &bin,
            GltfError::Invalid("accessors[0].count".into()),
        );
    }
    for (from, to, path) in [
        (
            r#""version": "2.0""#,
            r#""version": "2.0", "version": "2.0""#,
            "asset.version duplicate",
        ),
        (
            r#""mesh": 0"#,
            r#""mesh": 0, "mesh": 0"#,
            "nodes[1].mesh duplicate",
        ),
        (
            r#""POSITION": 0"#,
            r#""POSITION": 0, "POSITION": 0"#,
            "meshes[0].primitives[0].attributes.POSITION duplicate",
        ),
        (
            r#""count": 3"#,
            r#""count": 3, "count": 3"#,
            "accessors[0].count duplicate",
        ),
        (r#""name": "root""#, r#""name": 0"#, "nodes[0].name"),
        (
            r#""target": 34962"#,
            r#""target": 0"#,
            "bufferViews[0].target",
        ),
    ] {
        refuses(
            &edit(&text, from, to),
            &bin,
            GltfError::Invalid(path.into()),
        );
    }
}
