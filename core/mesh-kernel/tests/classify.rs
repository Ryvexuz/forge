use mesh_kernel::classify::{classify_faces, Side};
use mesh_kernel::gen::closed_box_with_triangles;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::vec3::Vec3;

fn cube(scale: f64) -> Mesh {
    let fixture = closed_box_with_triangles(12).unwrap();
    Mesh::from_triangles(
        fixture
            .vertices
            .into_iter()
            .map(|p| Vec3::new(p[0] * scale, p[1] * scale, p[2] * scale))
            .collect(),
        &fixture.triangles,
    )
    .unwrap()
}

fn sample(x: f64, y: f64, z: f64) -> Mesh {
    // The symmetric triangle centers the sample around the requested point;
    // y=z=0 also exercises a projected box diagonal.
    Mesh::from_triangles(
        vec![
            Vec3::new(x, y - 0.125, z - 0.125),
            Vec3::new(x, y + 0.125, z - 0.125),
            Vec3::new(x, y, z + 0.25),
        ],
        &[[0, 1, 2]],
    )
    .unwrap()
}

#[test]
fn classify_points_in_box() {
    let solid = cube(1.0);
    for (x, y, z, expected) in [
        (0.0, 0.0, 0.0, Side::Inside),
        (0.5, 0.25, -0.25, Side::Inside),
        (2.0, 0.0, 0.0, Side::Outside),
        (-2.0, 0.0, 0.0, Side::Outside),
        (0.0, 2.0, 0.0, Side::Outside),
        (0.0, 0.0, -2.0, Side::Outside),
    ] {
        assert_eq!(classify_faces(&sample(x, y, z), &solid), vec![expected]);
    }
    assert_eq!(
        classify_faces(&sample(0.0, 0.0, 0.0), &Mesh::default()),
        vec![Side::Outside]
    );
    assert!(classify_faces(&Mesh::default(), &solid).is_empty());
}

#[test]
fn classify_coplanar_faces_same_and_opposite() {
    let solid = cube(1.0);
    assert_eq!(
        classify_faces(&solid, &solid),
        vec![Side::OnBoundarySame; 12]
    );
    let (vertices, mut triangles) = solid.to_triangles();
    for triangle in &mut triangles {
        triangle.swap(1, 2);
    }
    let reversed = Mesh::from_triangles(vertices, &triangles).unwrap();
    assert_eq!(
        classify_faces(&reversed, &solid),
        vec![Side::OnBoundaryOpposite; 12]
    );
    assert_eq!(
        classify_faces(&solid, &reversed),
        vec![Side::OnBoundaryOpposite; 12]
    );
}

#[test]
fn classify_contained_mesh_all_inside() {
    for scale in [f64::from_bits(16), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        let outer = cube(scale);
        let inner = cube(scale * 0.5);
        assert_eq!(classify_faces(&inner, &outer), vec![Side::Inside; 12]);
        assert_eq!(classify_faces(&outer, &inner), vec![Side::Outside; 12]);
    }
}
