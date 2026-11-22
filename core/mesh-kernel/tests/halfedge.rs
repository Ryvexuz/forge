use mesh_kernel::gen::{self, closed_box_with_triangles, icosphere, jittered_closed_mesh};
use mesh_kernel::halfedge::{Mesh, MeshError};
use mesh_kernel::vec3::Vec3;

fn convert(fixture: &gen::Mesh) -> Mesh {
    Mesh::from_triangles(
        fixture
            .vertices
            .iter()
            .map(|&[x, y, z]| Vec3::new(x, y, z))
            .collect(),
        &fixture.triangles,
    )
    .unwrap()
}

fn invariants(mesh: &Mesh) -> bool {
    if !mesh.is_watertight() {
        return false;
    }
    for (index, edge) in mesh.halfedges.iter().enumerate() {
        let Some(twin) = edge.twin else {
            return false;
        };
        if mesh.halfedges[twin].twin != Some(index) {
            return false;
        }
        let second = &mesh.halfedges[edge.next];
        let third = &mesh.halfedges[second.next];
        if third.next != index || second.face != edge.face || third.face != edge.face {
            return false;
        }
    }
    // Independently enumerate each outgoing set, then walk twin.next.
    let mut fans = vec![Vec::new(); mesh.vertices.len()];
    for (index, edge) in mesh.halfedges.iter().enumerate() {
        fans[edge.origin].push(index);
    }
    for (vertex, outgoing) in fans.iter().enumerate() {
        if outgoing.is_empty() {
            return false;
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut current = outgoing[0];
        while seen.insert(current) {
            if mesh.halfedges[current].origin != vertex {
                return false;
            }
            current = mesh.halfedges[mesh.halfedges[current].twin.unwrap()].next;
        }
        if current != outgoing[0] || seen.len() != outgoing.len() {
            return false;
        }
    }
    true
}

#[test]
fn halfedge_round_trip_from_triangles() {
    for fixture in [
        closed_box_with_triangles(12).unwrap(),
        closed_box_with_triangles(100).unwrap(),
        icosphere(1).unwrap(),
    ] {
        let (positions, triangles) = convert(&fixture).to_triangles();
        let positions: Vec<_> = positions.iter().map(|v| [v.x, v.y, v.z]).collect();
        assert_eq!(positions, fixture.vertices);
        assert_eq!(triangles, fixture.triangles);
    }
}

#[test]
fn halfedge_invariants_random_meshes() {
    for seed in 0..1000 {
        let fixture = jittered_closed_mesh(seed).unwrap();
        assert!(invariants(&convert(&fixture)), "seed {seed}");
    }
}

#[test]
fn generated_box_is_watertight() {
    let mesh = convert(&closed_box_with_triangles(12).unwrap());
    assert!(mesh.is_watertight());
    assert_eq!(mesh.euler_characteristic(), 2);
    assert!(!Mesh::default().is_watertight());
}

#[test]
fn torus_euler_characteristic() {
    // Periodic 4 by 4 triangulated grid embedded as a torus.
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let side = 4;
    for i in 0..side {
        let u = std::f64::consts::TAU * i as f64 / side as f64;
        for j in 0..side {
            let v = std::f64::consts::TAU * j as f64 / side as f64;
            vertices.push(Vec3::new(
                (2.0 + v.cos()) * u.cos(),
                (2.0 + v.cos()) * u.sin(),
                v.sin(),
            ));
            let a = i * side + j;
            let b = ((i + 1) % side) * side + j;
            let c = ((i + 1) % side) * side + (j + 1) % side;
            let d = i * side + (j + 1) % side;
            triangles.extend_from_slice(&[[a, b, c], [a, c, d]]);
        }
    }
    let mesh = Mesh::from_triangles(vertices, &triangles).unwrap();
    assert!(mesh.is_watertight());
    assert_eq!(mesh.euler_characteristic(), 0);
}

#[test]
fn rejects_bad_topology_and_detects_twin_mutation() {
    let fixture = closed_box_with_triangles(12).unwrap();
    let mut mesh = convert(&fixture);
    let original = mesh.clone();
    let a = mesh.halfedges[0].twin;
    let b = mesh.halfedges[1].twin;
    mesh.halfedges[0].twin = b;
    mesh.halfedges[1].twin = a;
    assert!(!invariants(&mesh), "swapped twins must fail m4's predicate");
    mesh.halfedges[0].twin = a;
    mesh.halfedges[1].twin = b;
    assert_eq!(mesh, original);
    assert!(invariants(&mesh));

    let mut open = fixture.clone();
    open.triangles.pop();
    assert!(!convert(&open).is_watertight());
    let mut reversed = fixture.clone();
    reversed.triangles[0].swap(0, 1);
    assert!(!convert(&reversed).is_watertight());
    let mut duplicate = fixture.clone();
    duplicate.triangles.push(duplicate.triangles[0]);
    assert!(!convert(&duplicate).is_watertight());

    // Two closed shells touching at one indexed vertex have two face fans.
    let mut pinched = fixture.clone();
    pinched.vertices.extend_from_slice(&fixture.vertices[1..]);
    for triangle in &fixture.triangles {
        pinched
            .triangles
            .push(triangle.map(|i| if i == 0 { 0 } else { i + 7 }));
    }
    assert!(!convert(&pinched).is_watertight());
    let mut corrupt = original;
    corrupt.halfedges[0].next = usize::MAX;
    assert!(!corrupt.is_watertight());
}

#[test]
fn input_validation_and_bytes() {
    let points = vec![Vec3::default(); 3];
    assert_eq!(
        Mesh::from_triangles(points.clone(), &[[0, 1, 3]]),
        Err(MeshError::InvalidIndex)
    );
    assert_eq!(
        Mesh::from_triangles(points.clone(), &[[0, 0, 1]]),
        Err(MeshError::RepeatedVertex)
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            Mesh::from_triangles(vec![Vec3::new(bad, 0.0, 0.0)], &[]),
            Err(MeshError::NonFinitePosition)
        );
    }
    let mesh = Mesh::from_triangles(points, &[[0, 1, 2]]).unwrap();
    let mut expected = Vec::new();
    expected.extend_from_slice(&3_u64.to_le_bytes());
    expected.extend_from_slice(&1_u64.to_le_bytes());
    for _ in 0..9 {
        expected.extend_from_slice(&0_f64.to_le_bytes());
    }
    for i in 0..3_u64 {
        expected.extend_from_slice(&i.to_le_bytes());
    }
    assert_eq!(mesh.to_bytes(), expected);
}

#[test]
fn vector_arithmetic_and_extreme_normalization() {
    let x = Vec3::new(1.0, 0.0, 0.0);
    let y = Vec3::new(0.0, 1.0, 0.0);
    assert_eq!(x.dot(y), 0.0);
    assert_eq!(x.cross(y), Vec3::new(0.0, 0.0, 1.0));
    assert_eq!((x + y) - y, x);
    assert_eq!(-x, x * -1.0);
    assert_eq!(2.0 * x, Vec3::new(2.0, 0.0, 0.0));
    assert_eq!(Vec3::new(3.0, 4.0, 0.0).length(), 5.0);
    assert_eq!(Vec3::default().normalized(), Vec3::default());
    // Exact axis references require no rounded higher-precision approximation.
    for magnitude in [f64::from_bits(1), f64::MIN_POSITIVE, 1.0, f64::MAX] {
        assert_eq!(Vec3::new(magnitude, 0.0, 0.0).normalized(), x);
        assert_eq!(Vec3::new(magnitude, 0.0, 0.0).length(), magnitude);
    }
    assert_eq!(Vec3::new(1.0, 1.0, 0.0).dot(Vec3::new(1.0, -1.0, 0.0)), 0.0);
}
