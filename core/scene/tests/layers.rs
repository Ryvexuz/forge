use scene::hierarchy::Node;
use scene::layer::Layer;
use scene::scene::{Mesh, Scene};
use scene::transform::Transform;

#[test]
fn layer_visibility_filters_instances() {
    let mut scene = Scene {
        layers: vec![
            Layer {
                id: 42,
                name: "foreground".into(),
                visible: true,
            },
            Layer {
                id: 7,
                name: "background".into(),
                visible: true,
            },
            Layer {
                id: 99,
                name: "details".into(),
                visible: true,
            },
        ],
        ..Scene::default()
    };
    scene.meshes.push(Mesh {
        positions: vec![[0.0, 0.0, 0.0]],
        indices: vec![0],
    });
    scene.hierarchy.nodes.push(Node {
        parent: None,
        mesh: Some(0),
        transform: Transform::default(),
    });
    for layer in [7, 42, 99, 7, 99, 42] {
        scene.instances.push(layer, 0, 0.0, 0.0).unwrap();
    }

    assert_eq!(scene.visible_instances(), vec![0, 1, 2, 3, 4, 5]);
    scene.layers[1].visible = false;
    assert_eq!(scene.visible_instances(), vec![1, 2, 4, 5]);
    scene.layers[1].visible = true;
    assert_eq!(scene.visible_instances(), vec![0, 1, 2, 3, 4, 5]);
    assert_eq!(scene.instances.layers(), &[7, 42, 99, 7, 99, 42]);
    assert_eq!(scene.hierarchy.nodes[0].mesh, Some(0));
    assert_eq!(scene.meshes[0].indices, vec![0]);
}

#[test]
fn visibility_handles_empty_unknown_and_duplicate_layer_ids() {
    let mut scene = Scene::default();
    assert!(scene.visible_instances().is_empty());
    for layer in [0, u32::MAX, 0] {
        scene.instances.push(layer, 0, 0.0, 0.0).unwrap();
    }
    assert!(scene.visible_instances().is_empty());

    scene.layers.push(Layer {
        id: u32::MAX,
        name: String::new(),
        visible: false,
    });
    assert!(scene.visible_instances().is_empty());
    scene.layers.push(Layer {
        id: u32::MAX,
        name: "visible duplicate".into(),
        visible: true,
    });
    assert_eq!(scene.visible_instances(), vec![1]);
    scene.layers[0].visible = true;
    assert_eq!(scene.visible_instances(), vec![1]);
    scene.layers[0].visible = false;
    scene.layers[1].visible = false;
    assert!(scene.visible_instances().is_empty());
}
