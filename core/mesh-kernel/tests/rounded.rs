use mesh_kernel::boolean::{difference, intersection, union, BooleanError};
use mesh_kernel::broadphase::candidate_pairs;
use mesh_kernel::gen;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::intersect::{
    intersect_triangles, intersect_triangles_tagged, Feature, Intersection, IntersectionError,
    TaggedIntersection, TaggedPoint,
};
use mesh_kernel::predicates::orient2d;
use mesh_kernel::split::{split_faces_tagged, split_faces_with_seams, SplitError};
use mesh_kernel::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

const RELATIVE_VOLUME_ERROR: f64 = 1e-9;
const SEED: u64 = 0x724f_756e_6465_6432;
// Verifier baseline: if main accepts this pair, try n=1212, [0.3, 0.7, 0.1],
// then n=1212, [1.0 / 3.0, 0.2, 0.1], recording main's split refusal.
const OFFSET: [f64; 3] = [0.3, 0.2, 0.1];
const BASE_TRIANGLES: usize = 1200;
type FaceCuts = Vec<Vec<TaggedIntersection>>;
type BooleanOp = fn(&Mesh, &Mesh) -> Result<Mesh, BooleanError>;

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

fn fixtures() -> Vec<(&'static str, Mesh, Mesh)> {
    vec![
        (
            "overlapping",
            grid_box(8, 2.0, [0.0; 3]),
            grid_box(8, 2.0, [0.875, 0.625, 0.375]),
        ),
        (
            "contained",
            grid_box(8, 2.0, [0.0; 3]),
            grid_box(4, 1.0, [0.125; 3]),
        ),
        (
            "disjoint",
            grid_box(8, 2.0, [0.0; 3]),
            grid_box(8, 2.0, [4.0, 0.125, 0.125]),
        ),
        (
            "touching",
            grid_box(4, 1.0, [0.0; 3]),
            grid_box(4, 1.0, [1.0, 0.0, 0.0]),
        ),
        (
            "partly coplanar",
            grid_box(4, 1.0, [0.0; 3]),
            grid_box(4, 1.0, [0.5, 0.0, 0.25]),
        ),
    ]
}

fn convert(fixture: gen::Mesh, offset: [f64; 3]) -> Mesh {
    Mesh::from_triangles(
        fixture
            .vertices
            .into_iter()
            .map(|[x, y, z]| Vec3::new(x + offset[0], y + offset[1], z + offset[2]))
            .collect(),
        &fixture.triangles,
    )
    .unwrap()
}

fn boxes(n: usize) -> (Mesh, Mesh) {
    (
        convert(gen::closed_box_with_triangles(n).unwrap(), [0.0; 3]),
        convert(gen::closed_box_with_triangles(n).unwrap(), OFFSET),
    )
}

fn cuts(a: &Mesh, b: &Mesh) -> (FaceCuts, FaceCuts) {
    let (av, at) = a.to_triangles();
    let (bv, bt) = b.to_triangles();
    let mut ca = vec![Vec::new(); at.len()];
    let mut cb = vec![Vec::new(); bt.len()];
    for (fa, fb) in candidate_pairs(a, b) {
        let cut = intersect_triangles_tagged(
            at[fa as usize].map(|i| av[i]),
            bt[fb as usize].map(|i| bv[i]),
        )
        .unwrap();
        if cut != TaggedIntersection::None {
            ca[fa as usize].push(cut.clone());
            cb[fb as usize].push(cut.swapped());
        }
    }
    (ca, cb)
}

fn plain(cuts: &[Vec<TaggedIntersection>]) -> Vec<Vec<Intersection>> {
    cuts.iter()
        .map(|list| list.iter().map(TaggedIntersection::untagged).collect())
        .collect()
}

fn draw(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 33
}

fn pair(state: &mut u64, thirds: bool, case: usize) -> [[Vec3; 3]; 2] {
    let mut triangles = std::array::from_fn(|_| {
        std::array::from_fn(|_| {
            let p = std::array::from_fn::<_, 3, _>(|_| {
                if thirds {
                    ((draw(state) % 13) as i64 - 6) as f64 / 3.0
                } else {
                    (draw(state) % 33) as f64 / 8.0 - 2.0
                }
            });
            Vec3::new(p[0], p[1], p[2])
        })
    });
    match case % 4 {
        0 => triangles[1][0] = triangles[0][0],
        1 => {
            triangles[1][0] = triangles[0][0];
            triangles[1][1] = triangles[0][1];
        }
        2 => {
            for t in &mut triangles {
                for p in t {
                    p.z = 0.0;
                }
            }
        }
        _ => {}
    }
    for t in &mut triangles {
        for i in (1..3).rev() {
            t.swap(i, (draw(state) % (i as u64 + 1)) as usize);
        }
    }
    triangles
}

fn compare_paths(a: [Vec3; 3], b: [Vec3; 3], label: u64) {
    let tagged = intersect_triangles_tagged(a, b);
    assert_eq!(
        tagged
            .as_ref()
            .map(TaggedIntersection::untagged)
            .map_err(|e| *e),
        intersect_triangles(a, b),
        "seed={SEED} case={label}"
    );
    assert_eq!(
        intersect_triangles_tagged(b, a),
        tagged.map(|t| t.swapped()),
        "seed={SEED} case={label}"
    );
}

#[test]
fn tagged_intersection_matches_untagged() {
    let mut state = SEED;
    for i in 0..20_000 {
        let [a, b] = pair(&mut state, i % 2 == 0, i);
        compare_paths(a, b, i as u64);
    }
    let (a, b) = boxes(BASE_TRIANGLES);
    let (av, at) = a.to_triangles();
    let (bv, bt) = b.to_triangles();
    let candidates = candidate_pairs(&a, &b);
    assert!(!candidates.is_empty());
    for (fa, fb) in candidates {
        compare_paths(
            at[fa as usize].map(|i| av[i]),
            bt[fb as usize].map(|i| bv[i]),
            (u64::from(fa) << 32) | u64::from(fb),
        );
    }
    // Copied pairs from intersect_coplanar_rounding_does_not_change_topology.
    for (scale, half) in [
        (f64::from_bits(1), 0.0),
        (f64::from_bits(4), f64::from_bits(2)),
        (f64::MIN_POSITIVE, f64::from_bits(1_u64 << 51)),
        (1.0, 0.5),
        (f64::MAX, f64::MAX / 2.0),
    ] {
        let origin = Vec3::default();
        let corner = Vec3::new(scale, 0.0, 0.0);
        let a = [origin, corner, Vec3::new(0.0, scale, 0.0)];
        let b = [origin, corner, Vec3::new(scale, scale, 0.0)];
        compare_paths(a, b, scale.to_bits());
        assert_eq!(
            intersect_triangles_tagged(a, b).unwrap().untagged(),
            Intersection::CoplanarOverlap(vec![origin, Vec3::new(half, half, 0.0), corner])
        );
    }
}

fn xyz(p: Vec3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn bits(p: Vec3) -> [u64; 3] {
    xyz(p).map(f64::to_bits)
}
fn cross(a: [f64; 3], b: [f64; 3], p: [f64; 3], [u, v]: [usize; 2]) -> f64 {
    (b[u] - a[u]) * (p[v] - a[v]) - (b[v] - a[v]) * (p[u] - a[u])
}
fn tolerance(points: &[[f64; 3]]) -> f64 {
    let m = points.iter().flatten().map(|x| x.abs()).fold(0.0, f64::max);
    64.0 * f64::EPSILON * m * m
}

/// Scaled by 8, inputs are integers in [-16,16]. Exact cut denominators have
/// magnitude at most about 4e5; a nonzero exact cross is at least about 4e-8.
/// Rounding the point and evaluating cross moves it by at most about
/// 6 * EPSILON * m * m < tol < 1e-13. This gap of five orders of magnitude
/// separates on-edge from off-edge in every case, including coplanar pairs,
/// edges in the other plane, Face points and non-dyadic crossings; no division
/// reference can become 0/0 because this check uses no division.
#[test]
fn tags_are_exact_on_dyadic_inputs() {
    let mut state = SEED;
    let mut counts = [0; 4];
    for i in 0..20_000 {
        let triangles = pair(&mut state, false, i);
        let label = format!("seed={SEED} pair={i}");
        let cut = match intersect_triangles_tagged(triangles[0], triangles[1]) {
            Ok(cut) => cut,
            Err(IntersectionError::DegenerateTriangle) => continue,
            Err(error) => panic!("{label}: {error}"),
        };
        let points = match cut {
            TaggedIntersection::None => Vec::new(),
            TaggedIntersection::Segment(p, q) => vec![p, q],
            TaggedIntersection::CoplanarOverlap(poly) => poly,
        };
        for point in points {
            counts[0] += 1;
            for (argument, triangle) in triangles.into_iter().enumerate() {
                let p = xyz(point.point);
                let t = triangle.map(xyz);
                match point.on[argument] {
                    Feature::Vertex(k) => {
                        counts[1] += 1;
                        assert_eq!(bits(point.point), bits(triangle[k]), "{label}");
                    }
                    Feature::Edge(k, l) => {
                        counts[2] += 1;
                        assert!(k < l && l < 3, "{label}");
                        assert_ne!(bits(point.point), bits(triangle[k]), "{label}");
                        assert_ne!(bits(point.point), bits(triangle[l]), "{label}");
                        for (d, &coordinate) in p.iter().enumerate() {
                            let (a, b) = (t[k][d], t[l][d]);
                            assert!(a.min(b) <= coordinate && coordinate <= a.max(b), "{label}");
                        }
                        let tol = tolerance(&[t[k], t[l], p]);
                        for axes in [[0, 1], [0, 2], [1, 2]] {
                            assert!(cross(t[k], t[l], p, axes).abs() <= tol, "{label}");
                        }
                    }
                    Feature::Face => {
                        counts[3] += 1;
                        for (d, &coordinate) in p.iter().enumerate() {
                            let lo = t.iter().map(|v| v[d]).fold(f64::INFINITY, f64::min);
                            let hi = t.iter().map(|v| v[d]).fold(f64::NEG_INFINITY, f64::max);
                            assert!(lo <= coordinate && coordinate <= hi, "{label}");
                        }
                        let axes = [[0, 1], [0, 2], [1, 2]]
                            .into_iter()
                            .find(|&axes| {
                                let project = |v: [f64; 3]| axes.map(|d| v[d]);
                                orient2d(project(t[0]), project(t[1]), project(t[2])).unwrap() != 0
                            })
                            .unwrap();
                        for k in 0..3 {
                            let l = (k + 1) % 3;
                            let value = cross(t[k], t[l], p, axes).abs();
                            assert!(value > tolerance(&[t[k], t[l], p]), "{label} {point:?}");
                        }
                    }
                }
            }
        }
    }
    writeln!(
        std::io::stdout(),
        "points={} vertex={} edge={} face={}",
        counts[0], counts[1], counts[2], counts[3]
    )
    .unwrap();
    assert!(counts.into_iter().all(|n| n > 0));
}

#[test]
fn tagged_split_equals_untagged_on_exact_fixtures() {
    for (name, a, b) in fixtures() {
        let (ca, cb) = cuts(&a, &b);
        for (mesh, cuts) in [(&a, ca), (&b, cb)] {
            let (tagged, tags) = split_faces_tagged(mesh, &cuts).expect(name);
            let (untagged, seams) = split_faces_with_seams(mesh, &plain(&cuts)).expect(name);
            assert_eq!(tagged.to_bytes(), untagged.to_bytes(), "{name}");
            assert_eq!(tags, seams, "{name}");
        }
    }
}

fn extents(mesh: &Mesh) -> [[f64; 3]; 2] {
    let axis = |d| mesh.vertices.iter().map(move |&p| xyz(p)[d]);
    let lo = std::array::from_fn(|d| axis(d).fold(f64::INFINITY, f64::min));
    let hi = std::array::from_fn(|d| axis(d).fold(f64::NEG_INFINITY, f64::max));
    [lo, hi]
}

fn check_boolean(a: &Mesh, b: &Mesh, operations: &[(BooleanOp, f64)]) {
    for &(operation, expected) in operations {
        let result = operation(a, b).unwrap();
        assert!(result.is_watertight());
        // Count connected components by an independent traversal of face twins.
        let mut seen = BTreeSet::new();
        let mut components = 0;
        for start in 0..result.faces.len() {
            if seen.contains(&start) {
                continue;
            }
            components += 1;
            let mut stack = vec![start];
            while let Some(face) = stack.pop() {
                if !seen.insert(face) {
                    continue;
                }
                let mut h = result.faces[face];
                for _ in 0..3 {
                    stack.push(result.halfedges[result.halfedges[h].twin.unwrap()].face);
                    h = result.halfedges[h].next;
                }
            }
        }
        assert_eq!(components, 1);
        assert_volume(&result, expected);
    }
}

fn volumes(a: &Mesh, b: &Mesh) -> [f64; 3] {
    let [amin, amax] = extents(a);
    let [bmin, bmax] = extents(b);
    let va: f64 = (0..3).map(|d| amax[d] - amin[d]).product();
    let vb: f64 = (0..3).map(|d| bmax[d] - bmin[d]).product();
    let inter: f64 = (0..3)
        .map(|d| (amax[d].min(bmax[d]) - amin[d].max(bmin[d])).max(0.0))
        .product();
    [va + vb - inter, inter, va - inter]
}

#[test]
fn gen_boxes_boolean_watertight_with_rounded_cuts() {
    let (a, b) = boxes(BASE_TRIANGLES);
    let (ca, cb) = cuts(&a, &b);
    let refusals = [
        split_faces_with_seams(&a, &plain(&ca)).err(),
        split_faces_with_seams(&b, &plain(&cb)).err(),
    ];
    writeln!(std::io::stdout(), "untagged split refusals: {refusals:?}").unwrap();
    assert!(refusals.iter().any(Option::is_some), "pair must require tags");
    let [vu, vi, vd] = volumes(&a, &b);
    check_boolean(&a, &b, &[(union, vu), (intersection, vi), (difference, vd)]);
}

#[test]
fn gen_boxes_scale_to_12000() {
    let started = Instant::now();
    let (a, b) = boxes(12000);
    check_boolean(&a, &b, &[(union, volumes(&a, &b)[0])]);
    let elapsed = started.elapsed();
    writeln!(std::io::stdout(), "gen_boxes_scale_to_12000 elapsed={elapsed:?}").unwrap();
    assert!(
        elapsed <= std::time::Duration::from_secs(120),
        "scale time exceeded: {elapsed:?}"
    );
}

#[test]
fn tag_conflict_and_mismatch_are_refused() {
    let mesh = convert(gen::closed_box_with_triangles(12).unwrap(), [0.0; 3]);
    let p = Vec3::new(0.0, 0.0, -1.0);
    let run = |face: usize, point: Vec3, features: &[Feature]| {
        let mut cuts = vec![Vec::new(); mesh.faces.len()];
        for &feature in features {
            let p = TaggedPoint {
                point,
                on: [feature, Feature::Face],
            };
            cuts[face].push(TaggedIntersection::Segment(p, p));
        }
        split_faces_tagged(&mesh, &cuts)
    };
    assert_eq!(
        run(0, p, &[Feature::Edge(0, 1), Feature::Edge(1, 2)]),
        Err(SplitError::TagConflict { face: 0 })
    );
    assert_eq!(
        run(0, p, &[Feature::Vertex(0)]),
        Err(SplitError::TagMismatch { face: 0 })
    );
    let (split, _) = run(0, p, &[Feature::Edge(0, 1), Feature::Face]).unwrap();
    assert!(split.is_watertight());
    assert_eq!(split.vertices.len(), 9);
    assert_eq!(split.faces.len(), 14);
    assert_eq!(
        run(1, Vec3::new(1.0, -1.0, -1.0), &[Feature::Edge(0, 2)]),
        Err(SplitError::TagConflict { face: 0 })
    );
    for feature in [
        Feature::Vertex(usize::MAX),
        Feature::Edge(0, usize::MAX),
        Feature::Face,
    ] {
        assert_eq!(run(0, p, &[feature]), Err(SplitError::TagMismatch { face: 0 }));
    }
    let corner = mesh.vertices[0];
    assert_eq!(
        run(0, corner, &[Feature::Edge(1, 2)]),
        Err(SplitError::TagMismatch { face: 0 })
    );
    let normalized = run(0, corner, &[Feature::Edge(0, 1), Feature::Face])
        .unwrap()
        .0;
    assert_eq!(normalized.to_bytes(), mesh.to_bytes());
    for x in [f64::NAN, f64::NEG_INFINITY, f64::INFINITY] {
        let error = run(0, Vec3::new(x, 0.0, -1.0), &[Feature::Face]).unwrap_err();
        assert_eq!(error, SplitError::NonFiniteCut { face: 0 });
    }
}
