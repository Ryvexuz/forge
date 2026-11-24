//! M12a hard pairs use quarter-grid boxes and exactly representable cuts,
//! so touching, offset-touching, partly coplanar and contained-touching need
//! no rounding. The rotated pair and `boolean_hard_pairs_watertight_signed_volume`
//! (m16) wait for M11b and M12b.
//! If a pair fails, stop and report the error or numbers; do not change source
//! code or weaken expectations to make it pass.

use mesh_kernel::boolean::{difference, intersection, union, BooleanError};
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};

const RELATIVE_VOLUME_ERROR: f64 = 1e-9;

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

fn checked_result(
    result: Result<Mesh, BooleanError>,
    label: &str,
    volume: f64,
    euler: Option<i128>,
) -> Mesh {
    let mesh = result.unwrap_or_else(|error| panic!("{label}: {error}"));
    assert!(mesh.is_watertight(), "{label}: watertight");
    // Keep the copied assertion unchanged while adding pair/operation context.
    std::panic::catch_unwind(|| assert_volume(&mesh, volume)).unwrap_or_else(|_| {
        panic!(
            "{label}: signed volume: actual={}, analytic={volume}",
            signed_volume(&mesh)
        )
    });
    assert_eq!(components(&mesh), 1, "{label}: components");
    if let Some(expected) = euler {
        assert_eq!(mesh.euler_characteristic(), expected, "{label}: Euler");
    }
    mesh
}

fn checked_empty(result: Result<Mesh, BooleanError>, label: &str) {
    let mesh = result.unwrap_or_else(|error| panic!("{label}: {error}"));
    assert!(mesh.faces.is_empty(), "{label}: expected no faces");
}

#[test]
fn boolean_hard_touching_pair() {
    let a = grid_box(4, 1.0, [0.0; 3]);
    let b = grid_box(4, 1.0, [1.0, 0.0, 0.0]);
    let merged = checked_result(union(&a, &b), "touching: union(A, B)", 2.0, Some(2));
    assert_eq!(merged.faces.len(), 320, "touching: union(A, B): triangles");
    checked_empty(intersection(&a, &b), "touching: intersection(A, B)");
    let remainder = checked_result(difference(&a, &b), "touching: difference(A, B)", 1.0, None);
    assert_eq!(
        remainder.faces.len(),
        192,
        "touching: difference(A, B): triangles"
    );
}

#[test]
fn boolean_hard_touching_offset_pair() {
    let a = grid_box(4, 1.0, [0.0; 3]);
    let b = grid_box(4, 1.0, [1.0, 0.25, 0.25]);
    checked_result(union(&a, &b), "touching-offset: union(A, B)", 2.0, Some(2));
    checked_empty(intersection(&a, &b), "touching-offset: intersection(A, B)");
    checked_result(
        difference(&a, &b),
        "touching-offset: difference(A, B)",
        1.0,
        None,
    );
}

#[test]
fn boolean_hard_partly_coplanar_pair() {
    let a = grid_box(4, 1.0, [0.0; 3]);
    let b = grid_box(4, 1.0, [0.5, 0.0, 0.25]);
    // Analytic overlap: 0.5 * 1 * 0.75 = 0.375.
    checked_result(
        union(&a, &b),
        "partly-coplanar: union(A, B)",
        1.625,
        Some(2),
    );
    checked_result(
        intersection(&a, &b),
        "partly-coplanar: intersection(A, B)",
        0.375,
        Some(2),
    );
    checked_result(
        difference(&a, &b),
        "partly-coplanar: difference(A, B)",
        0.625,
        Some(2),
    );
    checked_result(
        difference(&b, &a),
        "partly-coplanar: difference(B, A)",
        0.625,
        Some(2),
    );
}

#[test]
fn boolean_hard_contained_touching_pair() {
    let a = grid_box(4, 1.0, [0.0; 3]);
    let c = grid_box(2, 0.5, [0.25; 3]);
    checked_result(union(&a, &c), "contained-touching: union(A, C)", 1.0, None);
    checked_result(
        intersection(&a, &c),
        "contained-touching: intersection(A, C)",
        0.125,
        None,
    );
    checked_result(
        difference(&a, &c),
        "contained-touching: difference(A, C)",
        0.875,
        Some(2),
    );
    assert_eq!(
        difference(&c, &a),
        Err(BooleanError::EmptyResult),
        "contained-touching: difference(C, A)"
    );
}
