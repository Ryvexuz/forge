use mesh_kernel::broadphase::candidate_pairs;
use mesh_kernel::gen;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::vec3::Vec3;

fn convert(fixture: gen::Mesh, offset: f64) -> Mesh {
    Mesh::from_triangles(
        fixture
            .vertices
            .into_iter()
            .map(|[x, y, z]| Vec3::new(x + offset, y + offset, z + offset))
            .collect(),
        &fixture.triangles,
    )
    .unwrap()
}

// Independent oracle: compare each axis directly from the original triangles,
// without calling the implementation's Aabb or its cell mapping.
fn brute_force(a: &Mesh, b: &Mesh) -> Vec<(u32, u32)> {
    let (av, at) = a.to_triangles();
    let (bv, bt) = b.to_triangles();
    let axis = |v: Vec3, k| [v.x, v.y, v.z][k];
    let mut result = Vec::new();
    for (i, ta) in at.iter().enumerate() {
        for (j, tb) in bt.iter().enumerate() {
            if (0..3).all(|k| {
                let amin = ta
                    .iter()
                    .map(|&v| axis(av[v], k))
                    .fold(f64::INFINITY, f64::min);
                let amax = ta
                    .iter()
                    .map(|&v| axis(av[v], k))
                    .fold(f64::NEG_INFINITY, f64::max);
                let bmin = tb
                    .iter()
                    .map(|&v| axis(bv[v], k))
                    .fold(f64::INFINITY, f64::min);
                let bmax = tb
                    .iter()
                    .map(|&v| axis(bv[v], k))
                    .fold(f64::NEG_INFINITY, f64::max);
                amin <= bmax && bmin <= amax
            }) {
                result.push((i as u32, j as u32));
            }
        }
    }
    result
}

#[test]
fn broadphase_pairs_match_brute_force() {
    for seed in 0..4 {
        let a = convert(gen::jittered_closed_mesh(seed).unwrap(), 0.0);
        let b = convert(gen::jittered_closed_mesh(seed + 10).unwrap(), 0.5);
        let expected = brute_force(&a, &b);
        assert!(!expected.is_empty(), "seed={seed}");
        assert_eq!(candidate_pairs(&a, &b), expected, "seed={seed}");
    }
    // Identical triangles cross multiple grid cells. Mutating the x-cell loop
    // to skip its first cell loses the pair at the minimum corner of this box.
    let a = convert(gen::closed_box_with_triangles(12).unwrap(), 0.0);
    assert_eq!(candidate_pairs(&a, &a), brute_force(&a, &a));
}

#[test]
fn broadphase_100k_pair_count_bounded() {
    // Boxes touch at a corner, so the bound is non-vacuous, without requiring
    // quadratic output for coincident surfaces. Bound: fewer than 1,000,000.
    let a = convert(gen::closed_box_with_triangles(100_000).unwrap(), 0.0);
    let b = convert(gen::closed_box_with_triangles(100_000).unwrap(), 2.0);
    let pairs = candidate_pairs(&a, &b);
    println!("100k touching-box candidate count={}", pairs.len());
    assert!(!pairs.is_empty());
    assert!(pairs.len() < 1_000_000);
}

#[test]
fn extreme_coordinates_and_empty_meshes() {
    assert!(candidate_pairs(&Mesh::default(), &Mesh::default()).is_empty());
    for value in [0.0, f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let mesh = Mesh::from_triangles(
            vec![
                Vec3::new(-value, 0.0, 0.0),
                Vec3::new(value, 0.0, 0.0),
                Vec3::new(0.0, value, 0.0),
            ],
            &[[0, 1, 2]],
        )
        .unwrap();
        assert_eq!(candidate_pairs(&mesh, &mesh), vec![(0, 0)]);
    }
}

#[test]
#[should_panic(expected = "NonFinitePosition")]
fn rejects_mutated_nonfinite_mesh() {
    let mut mesh = convert(gen::closed_box_with_triangles(12).unwrap(), 0.0);
    mesh.vertices[0].x = f64::NAN;
    candidate_pairs(&mesh, &mesh);
}
