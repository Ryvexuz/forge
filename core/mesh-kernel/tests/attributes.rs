use mesh_kernel::attr::{
    compute_face_normals, compute_vertex_normals, AttrError, AttrStore, Attribute, Domain,
};
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::vec3::Vec3;

#[test]
fn normals_match_reference() {
    let cube = mesh_kernel::gen::closed_box_with_triangles(12).unwrap();
    let mesh = Mesh::from_triangles(
        cube.vertices
            .iter()
            .map(|p| Vec3::new(p[0], p[1], p[2]))
            .collect(),
        &cube.triangles,
    )
    .unwrap();
    let axes = [
        Vec3::new(0.0, 0.0, -1.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
    ];
    let normals = compute_face_normals(&mesh).unwrap();
    assert_eq!(normals.len(), 12);
    for (i, normal) in normals.iter().enumerate() {
        assert_eq!(*normal, axes[i / 2]);
    }

    let phi = (1.0 + 5.0_f64.sqrt()) / 2.0;
    let octahedron = vec![
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(-1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(0.0, -1.0, 0.0),
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.0, 0.0, -1.0),
    ];
    let icosahedron = vec![
        Vec3::new(-1.0, phi, 0.0),
        Vec3::new(1.0, phi, 0.0),
        Vec3::new(-1.0, -phi, 0.0),
        Vec3::new(1.0, -phi, 0.0),
        Vec3::new(0.0, -1.0, phi),
        Vec3::new(0.0, 1.0, phi),
        Vec3::new(0.0, -1.0, -phi),
        Vec3::new(0.0, 1.0, -phi),
        Vec3::new(phi, 0.0, -1.0),
        Vec3::new(phi, 0.0, 1.0),
        Vec3::new(-phi, 0.0, -1.0),
        Vec3::new(-phi, 0.0, 1.0),
    ];
    // Independently identify regular-polyhedron faces by their edge length.
    for vertices in [octahedron, icosahedron] {
        let edge_squared = if vertices.len() == 6 { 2.0 } else { 4.0 };
        let mut faces = Vec::new();
        for a in 0..vertices.len() {
            for b in a + 1..vertices.len() {
                for c in b + 1..vertices.len() {
                    if [(a, b), (b, c), (c, a)].iter().all(|&(i, j)| {
                        let d = vertices[i] - vertices[j];
                        (d.dot(d) - edge_squared).abs() <= 1e-12
                    }) {
                        let outward = (vertices[b] - vertices[a])
                            .cross(vertices[c] - vertices[a])
                            .dot(vertices[a]);
                        faces.push(if outward > 0.0 { [a, b, c] } else { [a, c, b] });
                    }
                }
            }
        }
        assert_eq!(faces.len(), if vertices.len() == 6 { 8 } else { 20 });
        let mesh = Mesh::from_triangles(vertices, &faces).unwrap();
        let normals = compute_vertex_normals(&mesh).unwrap();
        assert_eq!(normals.len(), mesh.vertices.len());
        for (i, (normal, position)) in normals.iter().zip(&mesh.vertices).enumerate() {
            // Independent analytic radial reference; do not call normalized().
            let length = position.dot(*position).sqrt();
            for (actual, expected) in [
                (normal.x, position.x / length),
                (normal.y, position.y / length),
                (normal.z, position.z / length),
            ] {
                assert!(
                    (actual - expected).abs() <= 1e-12,
                    "vertex {i}: actual={actual}, reference={expected}"
                );
            }
        }
    }
}

#[test]
fn attributes_round_trip() {
    let uv = Attribute::Uv(vec![[0.0, -0.0], [0.25, 1.0], [1.0, 0.5]]);
    let mut original = AttrStore::default();
    original.insert(Domain::Corner, "uv", uv.clone()).unwrap();
    let mut cloned = original.clone();
    cloned
        .insert(Domain::Corner, "uv", Attribute::Uv(vec![]))
        .unwrap();
    cloned.insert(Domain::Corner, "uv", uv.clone()).unwrap();
    assert_eq!(cloned.get(Domain::Corner, "uv"), Some(&uv));
    let mut expected = Vec::new();
    expected.extend_from_slice(&1_u64.to_le_bytes());
    expected.push(1);
    expected.extend_from_slice(&2_u64.to_le_bytes());
    expected.extend_from_slice(b"uv");
    expected.push(1);
    expected.extend_from_slice(&3_u64.to_le_bytes());
    for v in [0.0_f64, -0.0, 0.25, 1.0, 1.0, 0.5] {
        expected.extend_from_slice(&v.to_le_bytes());
    }
    assert_eq!(original.to_bytes(), expected);
    assert_eq!(cloned.to_bytes(), expected);
    let before = cloned.to_bytes();
    assert_eq!(
        cloned.insert(Domain::Corner, "uv", Attribute::Uv(vec![[f64::NAN, 0.0]])),
        Err(AttrError::NonFinite)
    );
    assert_eq!(cloned.to_bytes(), before);
    cloned
        .insert(Domain::Vertex, "uv", Attribute::Uv(vec![[0.5, 0.5]]))
        .unwrap();
    assert_eq!(cloned.get(Domain::Corner, "uv"), Some(&uv));
    assert_ne!(cloned.to_bytes(), before);
}

#[test]
fn normal_edges_and_invalid_meshes() {
    for scale in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        let mesh = Mesh::from_triangles(
            vec![
                Vec3::default(),
                Vec3::new(scale, 0.0, 0.0),
                Vec3::new(0.0, scale, 0.0),
            ],
            &[[0, 1, 2]],
        )
        .unwrap();
        // Exact analytic reference, valid at every positive finite scale.
        assert_eq!(
            compute_face_normals(&mesh).unwrap(),
            vec![Vec3::new(0.0, 0.0, 1.0)]
        );
        let mut broken = mesh.clone();
        broken.halfedges[0].next = usize::MAX;
        assert_eq!(
            compute_vertex_normals(&broken),
            Err(AttrError::InvalidTopology)
        );
        broken = mesh;
        broken.vertices[0].x = f64::INFINITY;
        assert_eq!(compute_face_normals(&broken), Err(AttrError::NonFinite));
    }
    let mesh = Mesh::from_triangles(
        vec![Vec3::default(), Vec3::default(), Vec3::default()],
        &[[0, 1, 2]],
    )
    .unwrap();
    assert_eq!(compute_face_normals(&mesh), Err(AttrError::DegenerateFace));
    assert!(compute_vertex_normals(&Mesh::default()).unwrap().is_empty());

    let overflow = Mesh::from_triangles(
        vec![
            Vec3::new(-f64::MAX, 0.0, 0.0),
            Vec3::new(f64::MAX, 0.0, 0.0),
            Vec3::new(0.0, f64::MAX, 0.0),
        ],
        &[[0, 1, 2]],
    )
    .unwrap();
    assert_eq!(
        compute_face_normals(&overflow).unwrap(),
        vec![Vec3::new(0.0, 0.0, 1.0)]
    );
    // Adjacent representable coordinates: exact differences are one ulp.
    let next = f64::from_bits(1.0_f64.to_bits() + 1);
    let translated = Mesh::from_triangles(
        vec![
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(next, 1.0, 0.0),
            Vec3::new(1.0, next, 0.0),
        ],
        &[[0, 1, 2], [0, 2, 1]],
    )
    .unwrap();
    assert_eq!(
        compute_face_normals(&translated).unwrap(),
        vec![Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, -1.0)]
    );
    assert_eq!(
        compute_vertex_normals(&translated),
        Err(AttrError::CancelledNormal)
    );
}
