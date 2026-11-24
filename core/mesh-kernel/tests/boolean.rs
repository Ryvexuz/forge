//! M11c plan-named volume and watertight tests use 100,000 gen triangles per box.
//! The dyadic_768 tests and remaining tests retain the exact dyadic fixtures.

use mesh_kernel::boolean::{difference, intersection, union, watertight_gate, BooleanError, Operand};
use mesh_kernel::gen;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;
use std::time::Instant;

const RELATIVE_VOLUME_ERROR: f64 = 1e-9;
type BooleanOp = fn(&Mesh, &Mesh) -> Result<Mesh, BooleanError>;
const OPERATIONS: [(&str, BooleanOp); 3] = [
    ("union", union),
    ("intersection", intersection),
    ("difference", difference),
];

fn signed_volume(mesh: &Mesh) -> f64 {
    let (vertices, triangles) = mesh.to_triangles();
    triangles
        .into_iter()
        .map(|[a, b, c]| vertices[a].dot(vertices[b].cross(vertices[c])) / 6.0)
        .sum()
}

fn assert_volume(mesh: &Mesh, expected: f64) {
    let actual = signed_volume(mesh);
    assert!(
        (actual - expected).abs() <= expected.abs() * RELATIVE_VOLUME_ERROR,
        "signed volume: actual={actual}, analytic={expected}"
    );
}

// Test-only inputs below have positive k and dyadic size, offset and spacing.
fn grid_box(k: usize, size: f64, offset: [f64; 3]) -> Mesh {
    assert!(k > 0 && size.is_finite() && size > 0.0);
    assert!(offset.iter().all(|v| v.is_finite()));
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut indices = BTreeMap::new();
    for axis in 0..3 {
        for plane in [0, k] {
            let mut normal = [0.0; 3];
            normal[axis] = if plane == 0 { -1.0 } else { 1.0 };
            let normal = Vec3::new(normal[0], normal[1], normal[2]);
            for i in 0..k {
                for j in 0..k {
                    let cell = [[i, j], [i + 1, j], [i + 1, j + 1], [i, j + 1]];
                    let corners = cell.map(|[u, v]| {
                        let mut lattice = [0; 3];
                        lattice[axis] = plane;
                        lattice[(axis + 1) % 3] = u;
                        lattice[(axis + 2) % 3] = v;
                        *indices.entry(lattice).or_insert_with(|| {
                            let xyz = std::array::from_fn::<_, 3, _>(|d| {
                                offset[d] - size / 2.0 + size * lattice[d] as f64 / k as f64
                            });
                            let index = vertices.len();
                            vertices.push(Vec3::new(xyz[0], xyz[1], xyz[2]));
                            index
                        })
                    });
                    for mut face in [
                        [corners[0], corners[1], corners[2]],
                        [corners[0], corners[2], corners[3]],
                    ] {
                        let [a, b, c] = face.map(|v| vertices[v]);
                        if (b - a).cross(c - a).dot(normal) < 0.0 {
                            face.swap(1, 2);
                        }
                        triangles.push(face);
                    }
                }
            }
        }
    }
    let mesh = Mesh::from_triangles(vertices, &triangles).unwrap();
    assert_eq!(mesh.faces.len(), 12 * k * k);
    assert!(mesh.is_watertight());
    assert_volume(&mesh, size.powi(3));
    mesh
}

fn overlapping_boxes() -> (Mesh, Mesh) {
    (
        grid_box(8, 2.0, [0.0; 3]),
        grid_box(8, 2.0, [0.875, 0.625, 0.375]),
    )
}

fn root(parents: &mut [usize], mut i: usize) -> usize {
    while parents[i] != i {
        parents[i] = parents[parents[i]];
        i = parents[i];
    }
    i
}

// Independent face union-find, joining only across shared undirected edges.
fn components(mesh: &Mesh) -> usize {
    let (_, faces) = mesh.to_triangles();
    let mut parents: Vec<_> = (0..faces.len()).collect();
    let mut edges = BTreeMap::new();
    for (face, [a, b, c]) in faces.into_iter().enumerate() {
        for (u, v) in [(a, b), (b, c), (c, a)] {
            if let Some(other) = edges.insert((u.min(v), u.max(v)), face) {
                let left = root(&mut parents, face);
                let right = root(&mut parents, other);
                parents[left] = right;
            }
        }
    }
    (0..parents.len())
        .map(|i| root(&mut parents, i))
        .collect::<BTreeSet<_>>()
        .len()
}

const OFFSET: [f64; 3] = [0.3, 0.2, 0.1];

fn plan_results() -> &'static [(&'static str, Mesh); 3] {
    static RESULTS: OnceLock<[(&str, Mesh); 3]> = OnceLock::new();
    RESULTS.get_or_init(|| {
        let convert = |fixture: gen::Mesh, offset: [f64; 3]| {
            Mesh::from_triangles(
                fixture
                    .vertices
                    .into_iter()
                    .map(|[x, y, z]| Vec3::new(x + offset[0], y + offset[1], z + offset[2]))
                    .collect(),
                &fixture.triangles,
            )
            .unwrap()
        };
        let a = convert(gen::closed_box_with_triangles(100_000).unwrap(), [0.0; 3]);
        let b = convert(gen::closed_box_with_triangles(100_000).unwrap(), OFFSET);
        for (name, input) in [("A", &a), ("B", &b)] {
            assert_eq!(input.faces.len(), 100_000, "{name}: input faces");
            assert!(input.is_watertight(), "{name}: input watertight");
        }
        OPERATIONS.map(|(name, operation)| {
            let start = Instant::now();
            let result = operation(&a, &b).unwrap_or_else(|e| panic!("{name}: {e}"));
            println!(
                "m11c {name} tris={} elapsed={:?}",
                result.faces.len(),
                start.elapsed()
            );
            (name, result)
        })
    })
}

#[test]
fn boolean_union_volume_reference() {
    let overlap = 1.7 * 1.8 * 1.9;
    let expected = [8.0 + 8.0 - overlap, overlap, 8.0 - overlap];
    for ((name, result), expected) in plan_results().iter().zip(expected) {
        // Keep the operation visible if the unchanged signed-volume assertion fails.
        println!("m9 {name}: expected={expected}");
        assert_volume(result, expected);
    }
}

#[test]
fn boolean_result_watertight() {
    for (name, result) in plan_results() {
        assert!(result.is_watertight(), "{name}");
        assert_eq!(result.euler_characteristic(), 2, "{name}: genus zero");
        assert_eq!(components(result), 1, "{name}");
        for p in &result.vertices {
            assert!((-1.0..=1.0 + OFFSET[0]).contains(&p.x), "{name}: x");
            assert!((-1.0..=1.0 + OFFSET[1]).contains(&p.y), "{name}: y");
            assert!((-1.0..=1.0 + OFFSET[2]).contains(&p.z), "{name}: z");
        }
    }
}

#[test]
fn boolean_union_volume_reference_dyadic_768() {
    let (a, b) = overlapping_boxes();
    // Closed-form box overlap is (9/8)(11/8)(13/8) = 1287/512.
    let expected = [6905.0 / 512.0, 1287.0 / 512.0, 2809.0 / 512.0];
    for ((name, operation), expected) in OPERATIONS.into_iter().zip(expected) {
        let result = operation(&a, &b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_volume(&result, expected);
    }
}

#[test]
fn boolean_result_watertight_dyadic_768() {
    let (a, b) = overlapping_boxes();
    for (name, operation) in OPERATIONS {
        let result = operation(&a, &b).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(result.is_watertight(), "{name}");
        assert_eq!(result.euler_characteristic(), 2, "{name}: genus zero");
        assert_eq!(components(&result), 1, "{name}");
        for p in result.vertices {
            assert!((-1.0..=15.0 / 8.0).contains(&p.x), "{name}: x");
            assert!((-1.0..=13.0 / 8.0).contains(&p.y), "{name}: y");
            assert!((-1.0..=11.0 / 8.0).contains(&p.z), "{name}: z");
        }
    }
}

#[test]
fn boolean_contained_and_disjoint_pairs() {
    let a = grid_box(8, 2.0, [0.0; 3]);
    let c = grid_box(4, 1.0, [0.125; 3]);
    let d = grid_box(8, 2.0, [4.0, 0.125, 0.125]);
    let ac_union = union(&a, &c).unwrap();
    assert_volume(&ac_union, 8.0);
    assert_eq!(ac_union.faces.len(), 768);
    assert_eq!(components(&ac_union), 1);
    let ac_intersection = intersection(&a, &c).unwrap();
    assert_volume(&ac_intersection, 1.0);
    assert_eq!(ac_intersection.faces.len(), 192);
    let ac_difference = difference(&a, &c).unwrap();
    assert!(ac_difference.is_watertight());
    assert_volume(&ac_difference, 7.0);
    assert_eq!(components(&ac_difference), 2, "outer and reversed inner shell");
    assert_eq!(ac_difference.euler_characteristic(), 4);
    assert_eq!(difference(&c, &a), Err(BooleanError::EmptyResult));
    let ad_union = union(&a, &d).unwrap();
    assert!(ad_union.is_watertight());
    assert_volume(&ad_union, 16.0);
    assert_eq!(components(&ad_union), 2, "disjoint shells");
    let empty = intersection(&a, &d).unwrap();
    assert!(empty.faces.is_empty(), "genuinely empty intersection");
    assert!(empty.vertices.is_empty());
    assert_volume(&difference(&a, &d).unwrap(), 8.0);
}

#[test]
fn boolean_refuses_bad_inputs_and_gate_fails_closed() {
    let (a, b) = overlapping_boxes();
    let (vertices, triangles) = a.to_triangles();
    let open = Mesh::from_triangles(vertices, &triangles[..triangles.len() - 1]).unwrap();
    let mut unused = a.clone();
    unused.vertices.push(Vec3::new(0.0, 0.0, 0.0));
    let mut malformed = a.clone();
    malformed.faces[0] = usize::MAX;
    for (_, operation) in OPERATIONS {
        for invalid in [&open, &unused, &malformed, &Mesh::default()] {
            assert_eq!(
                operation(invalid, &b),
                Err(BooleanError::InputNotWatertight { operand: Operand::A })
            );
            assert_eq!(
                operation(&a, invalid),
                Err(BooleanError::InputNotWatertight { operand: Operand::B })
            );
        }
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for axis in 0..3 {
                let mut bad = b.clone();
                let mut xyz = [bad.vertices[0].x, bad.vertices[0].y, bad.vertices[0].z];
                xyz[axis] = value;
                bad.vertices[0] = Vec3::new(xyz[0], xyz[1], xyz[2]);
                assert_eq!(
                    operation(&a, &bad),
                    Err(BooleanError::NonFiniteInput { operand: Operand::B })
                );
                assert_eq!(
                    operation(&bad, &b),
                    Err(BooleanError::NonFiniteInput { operand: Operand::A })
                );
                // A's topology failure must precede even B's non-finite check.
                assert_eq!(
                    operation(&open, &bad),
                    Err(BooleanError::InputNotWatertight { operand: Operand::A })
                );
                bad.faces[0] = usize::MAX;
                assert_eq!(
                    operation(&bad, &b),
                    Err(BooleanError::NonFiniteInput { operand: Operand::A })
                );
            }
        }
    }
    assert_eq!(
        watertight_gate(Mesh::default()),
        Err(BooleanError::NotWatertight)
    );
    assert_eq!(watertight_gate(open), Err(BooleanError::NotWatertight));
    assert_eq!(watertight_gate(a.clone()).unwrap().to_bytes(), a.to_bytes());
}

fn canonical_key(p: Vec3) -> [u64; 3] {
    [p.x, p.y, p.z].map(|v| {
        if v == 0.0 {
            0.0_f64.to_bits()
        } else {
            v.to_bits()
        }
    })
}

#[test]
fn boolean_output_is_deterministic_and_compact() {
    let (a, b) = overlapping_boxes();
    let result = union(&a, &b).unwrap();
    assert_eq!(result.to_bytes(), union(&a, &b).unwrap().to_bytes());
    let (vertices, triangles) = result.to_triangles();
    let mut used = BTreeSet::new();
    for index in triangles.into_iter().flatten() {
        if used.insert(index) {
            assert_eq!(index, used.len() - 1, "first-use vertex numbering");
        }
    }
    assert_eq!(used, (0..vertices.len()).collect());
    let keys: BTreeSet<_> = vertices.iter().copied().map(canonical_key).collect();
    assert_eq!(keys.len(), vertices.len(), "exact canonical coordinate weld");

    let mut signed_zero_a = a.clone();
    for p in &mut signed_zero_a.vertices {
        if p.x == 0.0 {
            p.x = -0.0;
        }
        if p.y == 0.0 {
            p.y = -0.0;
        }
        if p.z == 0.0 {
            p.z = -0.0;
        }
    }
    assert_eq!(
        result.to_bytes(),
        union(&signed_zero_a, &b).unwrap().to_bytes()
    );
    for p in result.vertices {
        assert_eq!([p.x, p.y, p.z].map(f64::to_bits), canonical_key(p));
    }
}
