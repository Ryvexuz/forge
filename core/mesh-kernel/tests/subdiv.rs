use mesh_kernel::halfedge::Mesh;
use mesh_kernel::subdiv::{catmull_clark, try_catmull_clark, PolyMesh};
use mesh_kernel::vec3::Vec3;
use std::collections::BTreeSet;

fn cube() -> PolyMesh {
    PolyMesh {
        vertices: [
            [-1.0, -1.0, -1.0],
            [1.0, -1.0, -1.0],
            [1.0, 1.0, -1.0],
            [-1.0, 1.0, -1.0],
            [-1.0, -1.0, 1.0],
            [1.0, -1.0, 1.0],
            [1.0, 1.0, 1.0],
            [-1.0, 1.0, 1.0],
        ]
        .map(|[x, y, z]| Vec3::new(x, y, z))
        .to_vec(),
        faces: vec![
            vec![0, 3, 2, 1],
            vec![4, 5, 6, 7],
            vec![0, 1, 5, 4],
            vec![3, 7, 6, 2],
            vec![0, 4, 7, 3],
            vec![1, 2, 6, 5],
        ],
    }
}

fn counts(mesh: &PolyMesh, expected: (usize, usize, usize)) {
    let mut edges = BTreeSet::new();
    for face in &mesh.faces {
        for i in 0..face.len() {
            let (a, b) = (face[i], face[(i + 1) % face.len()]);
            edges.insert((a.min(b), a.max(b)));
        }
    }
    assert_eq!(
        (mesh.vertices.len(), edges.len(), mesh.faces.len()),
        expected
    );
    assert_eq!(
        mesh.vertices.len() as i128 - edges.len() as i128 + mesh.faces.len() as i128,
        2
    );
    assert!(mesh.to_mesh().unwrap().is_watertight());
}

#[test]
fn catmull_clark_cube_counts() {
    let original = cube();
    counts(&original, (8, 12, 6));
    assert_eq!(catmull_clark(&original, 0), original);
    let one = catmull_clark(&original, 1);
    counts(&one, (26, 48, 24));
    counts(&catmull_clark(&original, 2), (98, 192, 96));
    // At a cube corner F=P/3, R=2P/3, n=3, hence P'=5P/9.
    // Removing the F term yields 4P/9 and fails this position assertion,
    // even though all topology counts remain unchanged.
    for (p, q) in original.vertices.iter().zip(&one.vertices) {
        assert_ne!(p, q);
        for (a, b) in [(p.x, q.x), (p.y, q.y), (p.z, q.z)] {
            assert!((b - a * (5.0 / 9.0)).abs() <= 16.0 * f64::EPSILON);
        }
    }
    let triangles = original.to_mesh().unwrap();
    let triangle_cube = PolyMesh::from_mesh(&triangles);
    counts(&triangle_cube, (8, 18, 12));
    counts(&catmull_clark(&triangle_cube, 1), (38, 72, 36));
    let (vertices, faces) = triangles.to_triangles();
    assert_eq!(
        triangle_cube.to_mesh().unwrap(),
        Mesh::from_triangles(vertices, &faces).unwrap()
    );
}

#[test]
fn invalid_input_is_rejected_even_at_zero_levels() {
    let mut mesh = cube();
    mesh.vertices[0].x = f64::NAN;
    assert!(try_catmull_clark(&mesh, 0).is_err());
    mesh = cube();
    mesh.faces[0][0] = usize::MAX;
    assert!(try_catmull_clark(&mesh, 1).is_err());
    mesh = cube();
    mesh.faces[0].clear();
    assert!(try_catmull_clark(&mesh, 1).is_err());
    mesh = cube();
    mesh.faces.pop();
    assert!(try_catmull_clark(&mesh, 1).is_err());
    assert!(try_catmull_clark(&PolyMesh::default(), 0).is_err());
}

#[test]
fn extreme_finite_coordinates_remain_finite() {
    for scale in [f64::MAX, f64::MIN_POSITIVE, f64::from_bits(1)] {
        let mut mesh = cube();
        for p in &mut mesh.vertices {
            *p = *p * scale;
        }
        let result = try_catmull_clark(&mesh, 1).unwrap();
        for p in result.vertices {
            for c in [p.x, p.y, p.z] {
                assert!(c.is_finite());
                assert!(c.abs() <= scale);
            }
        }
    }
}
