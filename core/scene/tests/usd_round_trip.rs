use scene::hierarchy::{HierarchyError, Node};
use scene::layer::Layer;
use scene::math::{Quat, Vec3};
use scene::scene::{Mesh, Scene};
use scene::transform::Transform;
use scene::usd::{compare_scenes, import_usda, UsdError, MAX_DEPTH};
use scene::usd_write::{export_usda, ExportError};
use std::path::Path;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/usd/{name}"));
    std::fs::read_to_string(path).unwrap()
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

fn mesh_scene() -> Scene {
    let mut scene = scene(&[None]);
    scene.hierarchy.nodes[0].mesh = Some(0);
    scene.meshes.push(Mesh {
        positions: vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
        indices: vec![0, 1, 2],
    });
    scene
}

fn round_trip(scene: &Scene) -> String {
    let text = export_usda(scene).unwrap();
    let imported = import_usda(&text).unwrap();
    assert_eq!(compare_scenes(scene, &imported), Ok(()));
    assert_eq!(export_usda(&imported).unwrap(), text);
    text
}

fn refuses(scene: &Scene, error: ExportError) {
    assert_eq!(export_usda(scene), Err(error));
}

#[test]
fn usd_round_trip_fixtures() {
    for (name, count) in [("hierarchy.usda", 5), ("transforms.usda", 3)] {
        let scene = import_usda(&fixture(name)).unwrap();
        assert_eq!(scene.hierarchy.nodes.len(), count, "{name}");
        round_trip(&scene);
    }
}

#[test]
fn usd_round_trip_detects_corruption() {
    let scene = import_usda(&fixture("hierarchy.usda")).unwrap();
    let text = export_usda(&scene).unwrap();
    let from = "    float3 xformOp:translate = (1, 2, 3)\n";
    let to = "    float3 xformOp:translate = (1, 2, 3.5)\n";
    assert_eq!(text.matches(from).count(), 1);
    let edited = import_usda(&text.replacen(from, to, 1)).unwrap();
    assert_eq!(
        compare_scenes(&scene, &edited),
        Err(UsdError::Mismatch {
            what: "node 0 translation".into(),
        })
    );
}

#[test]
fn usd_export_canonical_text() {
    let imported = import_usda(&fixture("hierarchy.usda")).unwrap();
    let expected = r#"#usda 1.0

def Xform "n0"
{
    float3 xformOp:translate = (1, 2, 3)
    quatf xformOp:orient = (1, 0, 0, 0)
    float3 xformOp:scale = (1, 1, 1)
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]

    def Xform "n1"
    {
        float3 xformOp:translate = (0.5, 0, -0.25)
        quatf xformOp:orient = (0.70710677, 0, 0, 0.70710677)
        float3 xformOp:scale = (1, 1, 1)
        uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]

        def Mesh "n2"
        {
            float3 xformOp:translate = (0, 0, 0)
            quatf xformOp:orient = (1, 0, 0, 0)
            float3 xformOp:scale = (2, 2, 2)
            uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
            int[] faceVertexCounts = [3]
            int[] faceVertexIndices = [0, 1, 2]
            point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]
        }
    }

    def Xform "n3"
    {
        float3 xformOp:translate = (0, 0, 0)
        quatf xformOp:orient = (1, 0, 0, 0)
        float3 xformOp:scale = (1, 1, 1)
        uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
    }
}

def Xform "n4"
{
    float3 xformOp:translate = (0, 0, 0)
    quatf xformOp:orient = (1, 0, 0, 0)
    float3 xformOp:scale = (1, 1, 1)
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
}
"#;
    assert_eq!(export_usda(&imported).unwrap(), expected);
    let mut one = scene(&[None]);
    one.hierarchy.nodes[0].transform.translation = Vec3::new(-0.0, 1e-7, 3.0e10);
    let text = round_trip(&one);
    assert!(text.contains("    float3 xformOp:translate = (-0, 0.0000001, 30000000000)\n"));

    assert_eq!(round_trip(&Scene::default()), "#usda 1.0\n");
    let mut empty_mesh = mesh_scene();
    empty_mesh.meshes[0] = Mesh::default();
    let text = round_trip(&empty_mesh);
    for attribute in [
        "int[] faceVertexCounts = []",
        "int[] faceVertexIndices = []",
        "point3f[] points = []",
    ] {
        assert!(text.contains(&format!("    {attribute}\n")));
    }
    // Display/parse must preserve all component bits, including signed zero and
    // the smallest subnormal, smallest normal, and largest finite values.
    for value in [
        -0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        f32::MAX,
        -f32::MAX,
    ] {
        let mut edge = mesh_scene();
        edge.hierarchy.nodes[0].transform.translation = Vec3::new(value, 0.0, 0.0);
        edge.hierarchy.nodes[0].transform.scale = Vec3::new(value, 1.0, 1.0);
        edge.meshes[0].positions[0] = [value, -value, 0.0];
        round_trip(&edge);
    }
    one.hierarchy.nodes[0].transform.rotation = Quat::new(-0.0, 0.0, -0.0, -1.0).unwrap();
    round_trip(&one);
    let mut nested_meshes = mesh_scene();
    nested_meshes.hierarchy.nodes.push(Node {
        parent: Some(0),
        mesh: Some(1),
        transform: Transform::default(),
    });
    nested_meshes.meshes.push(Mesh::default());
    round_trip(&nested_meshes);
}

#[test]
fn usd_export_refuses_what_cannot_round_trip() {
    let mut layered = scene(&[None]);
    layered.layers.push(Layer {
        id: 0,
        name: "a".into(),
        visible: true,
    });
    refuses(&layered, ExportError::Layers);
    layered.instances.push(0, 0, 0.0, 0.0).unwrap();
    layered.hierarchy.nodes[0].parent = Some(5);
    refuses(&layered, ExportError::Layers);
    layered.layers.clear();
    refuses(&layered, ExportError::Instances);
    let mut instanced = scene(&[None]);
    instanced.instances.push(0, 0, 0.0, 0.0).unwrap();
    refuses(&instanced, ExportError::Instances);

    refuses(
        &scene(&[None, Some(5)]),
        ExportError::Hierarchy(HierarchyError::InvalidParent { node: 1, parent: 5 }),
    );
    refuses(
        &scene(&[Some(1), Some(0)]),
        ExportError::Hierarchy(HierarchyError::Cycle),
    );
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = scene(&[None]);
        invalid.hierarchy.nodes[0].transform.translation.x = value;
        refuses(
            &invalid,
            ExportError::Hierarchy(HierarchyError::InvalidTransform { node: 0 }),
        );
        invalid.hierarchy.nodes[0].transform.translation.x = 0.0;
        invalid.hierarchy.nodes[0].transform.scale.y = value;
        refuses(
            &invalid,
            ExportError::Hierarchy(HierarchyError::InvalidTransform { node: 0 }),
        );
    }
    refuses(
        &scene(&[None, None, Some(0)]),
        ExportError::NodeOrder { node: 2 },
    );
    refuses(&scene(&[Some(1), None]), ExportError::NodeOrder { node: 1 });
    let mut shared = mesh_scene();
    shared.hierarchy.nodes.push(Node {
        parent: None,
        mesh: Some(0),
        transform: Transform::default(),
    });
    refuses(&shared, ExportError::MeshOrder { node: 1 });
    let mut unused = scene(&[None]);
    unused.meshes.push(Mesh::default());
    refuses(&unused, ExportError::MeshCount { used: 0, meshes: 1 });
    let mut missing = scene(&[None]);
    missing.hierarchy.nodes[0].mesh = Some(0);
    refuses(&missing, ExportError::MeshCount { used: 1, meshes: 0 });
    missing.hierarchy.nodes[0].mesh = Some(u32::MAX);
    refuses(&missing, ExportError::MeshOrder { node: 0 });
    for indices in [vec![0, 1], vec![0, 1, 3], vec![0, 1, u32::MAX]] {
        let mut invalid = mesh_scene();
        invalid.meshes[0].indices = indices;
        refuses(&invalid, ExportError::MeshInvalid { mesh: 0 });
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = mesh_scene();
        invalid.meshes[0].positions[0][2] = value;
        refuses(&invalid, ExportError::MeshInvalid { mesh: 0 });
    }

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
    let mut rotation = scene(&[None]);
    rotation.hierarchy.nodes[0].transform.rotation = q;
    refuses(&rotation, ExportError::RotationNotFixed { node: 0 });
    rotation.hierarchy.nodes[0].transform.rotation = Quat::IDENTITY;
    round_trip(&rotation);

    // Preserve the specified precedence even when several checks could fail.
    let mut mixed = scene(&[None, None, Some(0)]);
    mixed.hierarchy.nodes[0].mesh = Some(1);
    refuses(&mixed, ExportError::NodeOrder { node: 2 });
    mixed.hierarchy.nodes[2].parent = None;
    refuses(&mixed, ExportError::MeshOrder { node: 0 });
    mixed.hierarchy.nodes[0].mesh = Some(0);
    mixed.meshes.push(Mesh {
        positions: vec![[f32::NAN; 3]],
        indices: vec![],
    });
    mixed.hierarchy.nodes[0].transform.rotation = q;
    mixed.meshes.push(Mesh::default());
    refuses(&mixed, ExportError::MeshCount { used: 1, meshes: 2 });
    mixed.meshes.truncate(1);
    refuses(&mixed, ExportError::MeshInvalid { mesh: 0 });
    mixed.meshes[0].positions.clear();
    refuses(&mixed, ExportError::RotationNotFixed { node: 0 });

    let mut ordered = mesh_scene();
    ordered.hierarchy.nodes.push(Node {
        parent: None,
        mesh: Some(1),
        transform: Transform::default(),
    });
    ordered.meshes.push(ordered.meshes[0].clone());
    for mesh in &mut ordered.meshes {
        mesh.indices.truncate(2);
    }
    refuses(&ordered, ExportError::MeshInvalid { mesh: 0 });
    ordered.meshes[0].indices.push(2);
    refuses(&ordered, ExportError::MeshInvalid { mesh: 1 });
    ordered.meshes[1].indices.push(2);
    for node in &mut ordered.hierarchy.nodes {
        node.transform.rotation = q;
    }
    refuses(&ordered, ExportError::RotationNotFixed { node: 0 });
    ordered.hierarchy.nodes[0].transform.rotation = Quat::IDENTITY;
    refuses(&ordered, ExportError::RotationNotFixed { node: 1 });

    let parents: Vec<_> = (0..MAX_DEPTH).map(|i| i.checked_sub(1)).collect();
    let mut chain = scene(&parents);
    round_trip(&chain);
    chain.hierarchy.nodes.push(Node {
        parent: Some(MAX_DEPTH - 1),
        mesh: None,
        transform: Transform::default(),
    });
    refuses(&chain, ExportError::TooDeep { node: MAX_DEPTH });

    // A wide scene is not a deep one: traversal remains iterative and ordered.
    let wide = scene(&vec![None; MAX_DEPTH * MAX_DEPTH]);
    round_trip(&wide);
}
