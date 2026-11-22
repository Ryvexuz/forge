use mesh_kernel::gen::icosphere;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::remesh::{remesh, RemeshError};
use mesh_kernel::vec3::Vec3;

fn sphere(level: u32) -> Mesh {
    let fixture = icosphere(level).unwrap();
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

fn valence_energy(mesh: &Mesh) -> i64 {
    let mut degree = vec![0_i64; mesh.vertices.len()];
    for h in &mesh.halfedges {
        degree[h.origin] += 1;
    }
    degree.iter().map(|d| (d - 6).pow(2)).sum()
}

#[test]
fn remesh_edge_lengths_within_band() {
    let input = sphere(6);
    let target = 0.0225;
    let output = remesh(&input, target, 5).unwrap();
    assert!(output.is_watertight());
    assert_eq!(output.euler_characteristic(), 2);
    let energy = valence_energy(&output);
    println!("valence_squared_error={energy}");
    // Measured at L=0.0225 on icosphere(6): 13900 with the flip pass, 16616
    // without it. A triangulated sphere needs at least 12, which the greedy
    // single flip pass does not reach, so the bound sits between the two.
    assert!(energy <= 15000, "valence_squared_error={energy}");
    let mut minimum = f64::INFINITY;
    let mut maximum = 0.0_f64;
    for h in &output.halfedges {
        let length =
            (output.vertices[h.origin] - output.vertices[output.halfedges[h.next].origin]).length();
        minimum = minimum.min(length / target);
        maximum = maximum.max(length / target);
    }
    println!("edge_length_over_target: minimum={minimum:.17} maximum={maximum:.17}");
    assert!(minimum >= 0.6, "minimum/L={minimum}");
    assert!(maximum <= 1.5, "maximum/L={maximum}");
}

#[test]
fn rejects_invalid_inputs_without_changing_them() {
    let input = sphere(0);
    let saved = input.clone();
    for target in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(remesh(&input, target, 1), Err(RemeshError::InvalidTarget));
    }
    assert_eq!(
        remesh(&input, f64::from_bits(1), 1),
        Err(RemeshError::NumericalRange)
    );
    assert_eq!(
        remesh(&Mesh::default(), 1.0, 1),
        Err(RemeshError::InvalidMesh)
    );
    assert_eq!(input, saved);
}

#[test]
fn zero_sweeps_preserve_geometry_and_topology() {
    let input = sphere(0);
    assert_eq!(remesh(&input, 1.0, 0).unwrap(), input);
}
