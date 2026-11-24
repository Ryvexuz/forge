use mesh_kernel::gen::closed_box_with_triangles;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::intersect::{intersect_triangles, Intersection};
use mesh_kernel::split::{split_faces, SplitError};
use mesh_kernel::vec3::Vec3;
use std::collections::BTreeSet;

const Z25: [[Vec3; 3]; 2] = [
    [v(-2.0, -2.0, 0.25), v(2.0, -2.0, 0.25), v(2.0, 2.0, 0.25)],
    [v(-2.0, -2.0, 0.25), v(2.0, 2.0, 0.25), v(-2.0, 2.0, 0.25)],
];

const fn v(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3::new(x, y, z)
}

fn box_mesh(n: usize) -> Mesh {
    let generated = closed_box_with_triangles(n).unwrap();
    Mesh::from_triangles(
        generated
            .vertices
            .iter()
            .map(|&[x, y, z]| v(x, y, z))
            .collect(),
        &generated.triangles,
    )
    .unwrap()
}

fn cube() -> Mesh {
    box_mesh(12)
}

fn face_triangle(mesh: &Mesh, f: usize) -> [Vec3; 3] {
    mesh.to_triangles().1[f].map(|i| mesh.vertices[i])
}

fn cuts_against(mesh: &Mesh, plane: &[[Vec3; 3]]) -> Vec<Vec<Intersection>> {
    (0..mesh.faces.len())
        .map(|f| {
            plane
                .iter()
                .map(|&t| intersect_triangles(face_triangle(mesh, f), t).unwrap())
                .collect()
        })
        .collect()
}

fn area(mesh: &Mesh) -> f64 {
    (0..mesh.faces.len())
        .map(|f| {
            let [a, b, c] = face_triangle(mesh, f);
            0.5 * (b - a).cross(c - a).length()
        })
        .sum()
}

fn signed_volume(mesh: &Mesh) -> f64 {
    (0..mesh.faces.len())
        .map(|f| {
            let [a, b, c] = face_triangle(mesh, f);
            a.dot(b.cross(c)) / 6.0
        })
        .sum()
}

fn edges(mesh: &Mesh) -> BTreeSet<(usize, usize)> {
    mesh.halfedges
        .iter()
        .map(|h| {
            let b = mesh.halfedges[h.next].origin;
            (h.origin.min(b), h.origin.max(b))
        })
        .collect()
}

fn assert_segments(mesh: &Mesh, segments: &[(Vec3, Vec3)]) {
    let edges = edges(mesh);
    for &(a, b) in segments {
        let a = mesh.vertices.iter().position(|&p| p == a).unwrap();
        let b = mesh.vertices.iter().position(|&p| p == b).unwrap();
        assert!(edges.contains(&(a.min(b), a.max(b))), "missing {a}-{b}");
    }
}

fn on_face_zero(mesh: &Mesh, list: Vec<Intersection>) -> Vec<Vec<Intersection>> {
    let mut cuts = vec![vec![]; mesh.faces.len()];
    cuts[0] = list;
    cuts
}

fn segment(a: Vec3, b: Vec3) -> Intersection {
    Intersection::Segment(a, b)
}

fn relative(actual: f64, expected: f64) {
    assert!(actual.is_finite());
    assert!(
        (actual - expected).abs() <= 1e-12 * expected.abs(),
        "{actual} != {expected}"
    );
}

#[test]
fn split_cube_by_plane_is_watertight() {
    let input = cube();
    let out = split_faces(&input, &cuts_against(&input, &Z25)).unwrap();
    assert!(out.is_watertight());
    assert_eq!(out.euler_characteristic(), 2);
    assert_eq!((out.vertices.len(), out.faces.len()), (16, 28));
    for (a, b) in input.vertices.iter().zip(&out.vertices) {
        assert_eq!(
            [a.x, a.y, a.z].map(f64::to_bits),
            [b.x, b.y, b.z].map(f64::to_bits)
        );
    }
    for f in 0..out.faces.len() {
        let t = face_triangle(&out, f);
        assert!(!(t.iter().any(|p| p.z < 0.25) && t.iter().any(|p| p.z > 0.25)));
    }
    relative(signed_volume(&out), 8.0);
}

#[test]
fn split_keeps_total_area() {
    let input = cube();
    let tilted = [[v(-4.0, -4.0, 0.1), v(4.0, -4.0, 0.3), v(0.0, 4.0, -0.2)]];
    relative(area(&input), 24.0);
    for plane in [&Z25[..], &tilted[..]] {
        let out = split_faces(&input, &cuts_against(&input, plane)).unwrap();
        assert!(out.is_watertight());
        assert!(out.faces.len() > input.faces.len());
        relative(area(&out), 24.0);
        relative(signed_volume(&out), 8.0);
    }
}

#[test]
fn split_without_cuts_is_byte_identical() {
    for input in [cube(), box_mesh(100)] {
        for list in [vec![], vec![Intersection::None]] {
            let out = split_faces(&input, &vec![list; input.faces.len()]).unwrap();
            assert_eq!(out.to_bytes(), input.to_bytes());
        }
    }
    let input = cube();
    let p = v(0.5, -0.5, -1.0);
    let out = split_faces(&input, &on_face_zero(&input, vec![segment(p, p)])).unwrap();
    assert!(out.is_watertight());
    assert_eq!((out.vertices.len(), out.faces.len()), (9, 14));
    assert_eq!(out.to_triangles().1[3..14], input.to_triangles().1[1..12]);
    // Exact rational midpoint and determinant signs, even when floating-point
    // products would underflow or coordinate subtraction would overflow.
    for s in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX] {
        let mesh = Mesh::from_triangles(
            vec![v(-s, 0.0, 0.0), v(s, 0.0, 0.0), v(0.0, s, 0.0)],
            &[[0, 1, 2]],
        )
        .unwrap();
        let p = v(-0.0, 0.0, 0.0);
        let out = split_faces(&mesh, &[vec![segment(p, p)]]).unwrap();
        assert_eq!((out.vertices.len(), out.faces.len()), (4, 2));
        assert_segments(&out, &[(mesh.vertices[0], p), (mesh.vertices[1], p)]);
    }
}

#[test]
fn split_segments_along_an_edge_and_from_a_corner() {
    let input = cube();
    let [v0, v2, v1] = face_triangle(&input, 0);
    let out = split_faces(&input, &on_face_zero(&input, vec![segment(v0, v2)])).unwrap();
    assert_eq!(out.to_bytes(), input.to_bytes());
    let m = v(0.0, 0.0, -1.0);
    let out = split_faces(&input, &on_face_zero(&input, vec![segment(v0, m)])).unwrap();
    assert!(out.is_watertight());
    assert_eq!((out.vertices.len(), out.faces.len()), (9, 14));
    assert!(out.to_triangles().1[2..4].iter().all(|t| t.contains(&8)));
    let q = v(0.5, -0.25, -1.0);
    let out = split_faces(&input, &on_face_zero(&input, vec![segment(v1, q)])).unwrap();
    assert!(out.is_watertight());
    assert_eq!(out.vertices.len(), 9);
    assert!(edges(&out).contains(&(1, 8)));
}

#[test]
fn split_star_of_segments_meeting_inside_face() {
    let input = cube();
    let c = v(0.5, -0.5, -1.0);
    let n = v(0.0, -1.0, -1.0);
    let ends = [input.vertices[0], input.vertices[1], input.vertices[2], n];
    let pairs: Vec<_> = ends.iter().map(|&p| (c, p)).collect();
    let cuts = on_face_zero(&input, ends.iter().map(|&p| segment(c, p)).collect());
    let out = split_faces(&input, &cuts).unwrap();
    assert!(out.is_watertight());
    assert_eq!(out.vertices.len(), 10);
    // With n inserted before c, c->v0 needs a flip of n->v2.
    // Disabling constraint recovery leaves that edge absent.
    assert_segments(&out, &pairs);
    let n_id = out.vertices.iter().position(|&p| p == n).unwrap();
    assert!(out.to_triangles().1[7..9].iter().all(|t| t.contains(&n_id)));
}

fn square() -> Vec<Vec3> {
    vec![
        v(0.25, -0.75, -1.0),
        v(0.75, -0.75, -1.0),
        v(0.75, -0.25, -1.0),
        v(0.25, -0.25, -1.0),
    ]
}

#[test]
fn split_coplanar_overlap_boundary_becomes_edges() {
    let input = cube();
    let poly = square();
    let cuts = on_face_zero(&input, vec![Intersection::CoplanarOverlap(poly.clone())]);
    let out = split_faces(&input, &cuts).unwrap();
    assert!(out.is_watertight());
    assert_eq!(out.vertices.len(), 12);
    let pairs: Vec<_> = (0..poly.len())
        .map(|i| (poly[i], poly[(i + 1) % poly.len()]))
        .collect();
    assert_segments(&out, &pairs);
}

#[test]
fn split_refuses_bad_cuts() {
    let input = cube();
    assert_eq!(
        split_faces(&input, &vec![vec![]; 11]),
        Err(SplitError::CutCountMismatch {
            faces: 12,
            cuts: 11,
        })
    );
    let outside = v(0.0, 0.5, -1.0);
    let crossing = vec![
        segment(v(0.0, -0.75, -1.0), v(1.0, -0.75, -1.0)),
        segment(v(0.5, -1.0, -1.0), v(0.5, 0.0, -1.0)),
    ];
    for (list, expected) in [
        (
            vec![segment(outside, outside)],
            SplitError::PointOutsideFace { face: 0 },
        ),
        (crossing, SplitError::CrossingCuts { face: 0 }),
    ] {
        assert_eq!(
            split_faces(&input, &on_face_zero(&input, list)),
            Err(expected)
        );
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let p = v(bad, 0.0, -1.0);
        let list = vec![segment(outside, outside), segment(p, p)];
        assert_eq!(
            split_faces(&input, &on_face_zero(&input, list)),
            Err(SplitError::NonFiniteCut { face: 0 })
        );
    }
    let degenerate = Mesh::from_triangles(
        vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(2.0, 0.0, 0.0)],
        &[[0, 1, 2]],
    )
    .unwrap();
    assert_eq!(
        split_faces(&degenerate, &[vec![]]),
        Err(SplitError::DegenerateFace { face: 0 })
    );
    // The point is on edge 0-1 in face 0's xy chart, but strictly inside
    // face 1's xz chart. Even an empty cut list there must refuse it.
    let skew = Mesh::from_triangles(
        vec![
            v(0.0, 0.0, 0.0),
            v(1.0, 1.0, 1.0),
            v(1.0, 0.0, 0.0),
            v(0.0, 0.0, 1.0),
        ],
        &[[0, 1, 2], [1, 0, 3]],
    )
    .unwrap();
    let p = v(0.5, 0.5, 0.75);
    assert_eq!(
        split_faces(&skew, &[vec![segment(p, p)], vec![]]),
        Err(SplitError::InconsistentEdgePoint { face: 1 })
    );
}

#[test]
fn split_output_ignores_cut_order() {
    let input = cube();
    let mut cuts = cuts_against(&input, &Z25);
    let expected = split_faces(&input, &cuts).unwrap().to_bytes();
    assert_eq!(split_faces(&input, &cuts).unwrap().to_bytes(), expected);
    for list in &mut cuts {
        list.reverse();
        for cut in list {
            if let Intersection::Segment(a, b) = cut {
                std::mem::swap(a, b);
            }
        }
    }
    assert_eq!(split_faces(&input, &cuts).unwrap().to_bytes(), expected);
    let mut poly = square();
    let baseline = on_face_zero(&input, vec![Intersection::CoplanarOverlap(poly.clone())]);
    let expected = split_faces(&input, &baseline).unwrap().to_bytes();
    poly.rotate_left(1);
    poly.reverse();
    let changed = on_face_zero(&input, vec![Intersection::CoplanarOverlap(poly)]);
    assert_eq!(split_faces(&input, &changed).unwrap().to_bytes(), expected);
}
