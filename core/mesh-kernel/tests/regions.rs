use mesh_kernel::boolean::{difference, intersection, union, watertight_gate, BooleanError, Operand};
use mesh_kernel::broadphase::candidate_pairs;
use mesh_kernel::classify::{classify_faces, classify_regions, face_regions, Side};
use mesh_kernel::halfedge::{Mesh, MeshError};
use mesh_kernel::intersect::{intersect_triangles, Intersection};
use mesh_kernel::split::{split_faces, split_faces_with_seams};
use mesh_kernel::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::time::Instant;

const RELATIVE_VOLUME_ERROR: f64 = 1e-9;
type FaceCuts = Vec<Vec<Intersection>>;
type SplitMesh = (Mesh, BTreeSet<[usize; 2]>);
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

fn cuts(a: &Mesh, b: &Mesh) -> Result<(FaceCuts, FaceCuts), BooleanError> {
    let (vertices_a, triangles_a) = a.to_triangles();
    let (vertices_b, triangles_b) = b.to_triangles();
    let mut cuts_a = vec![Vec::new(); a.faces.len()];
    let mut cuts_b = vec![Vec::new(); b.faces.len()];
    for (face_a, face_b) in candidate_pairs(a, b) {
        let cut = intersect_triangles(
            triangles_a[face_a as usize].map(|i| vertices_a[i]),
            triangles_b[face_b as usize].map(|i| vertices_b[i]),
        )
        .map_err(|error| BooleanError::Intersect {
            face_a,
            face_b,
            error,
        })?;
        if cut != Intersection::None {
            cuts_a[face_a as usize].push(cut.clone());
            cuts_b[face_b as usize].push(cut);
        }
    }
    Ok((cuts_a, cuts_b))
}

// Only a split refusal may omit a fixture from the side/region checks.
// The byte/error comparison below always exercises every fixture.
fn split_fixture(name: &str, a: &Mesh, b: &Mesh) -> Option<[SplitMesh; 2]> {
    let (cuts_a, cuts_b) = cuts(a, b).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut output = Vec::new();
    for (mesh, cuts, operand) in [(a, cuts_a, Operand::A), (b, cuts_b, Operand::B)] {
        match split_faces_with_seams(mesh, &cuts) {
            Ok(split) => output.push(split),
            Err(error) => {
                assert_eq!(split_faces(mesh, &cuts), Err(error));
                writeln!(
                    std::io::stdout(),
                    "split refusal: fixture={name}, operand={operand:?}, error={error}"
                )
                .expect("report split refusal");
                return None;
            }
        }
    }
    Some(output.try_into().unwrap())
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Union,
    Intersection,
    Difference,
}

fn keep(operation: Operation, operand: Operand, side: Side) -> Option<bool> {
    match (operation, operand, side) {
        (Operation::Union, Operand::A, Side::Outside | Side::OnBoundarySame)
        | (Operation::Union, Operand::B, Side::Outside)
        | (Operation::Intersection, Operand::A, Side::Inside | Side::OnBoundarySame)
        | (Operation::Intersection, Operand::B, Side::Inside)
        | (Operation::Difference, Operand::A, Side::Outside | Side::OnBoundaryOpposite) => {
            Some(false)
        }
        (Operation::Difference, Operand::B, Side::Inside) => Some(true),
        _ => None,
    }
}

fn old_compose(a: &Mesh, b: &Mesh, operation: Operation) -> Result<Mesh, BooleanError> {
    for (mesh, operand) in [(a, Operand::A), (b, Operand::B)] {
        if mesh
            .vertices
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
        {
            return Err(BooleanError::NonFiniteInput { operand });
        }
        if !mesh.is_watertight() {
            return Err(BooleanError::InputNotWatertight { operand });
        }
    }
    for mesh in [a, b] {
        u32::try_from(mesh.faces.len()).map_err(|_| BooleanError::Mesh(MeshError::CountOverflow))?;
    }
    let (cuts_a, cuts_b) = cuts(a, b)?;
    let split_a = split_faces(a, &cuts_a).map_err(|error| BooleanError::Split {
        operand: Operand::A,
        error,
    })?;
    let split_b = split_faces(b, &cuts_b).map_err(|error| BooleanError::Split {
        operand: Operand::B,
        error,
    })?;
    let sides_a = classify_faces(&split_a, b);
    let sides_b = classify_faces(&split_b, a);
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut indices = BTreeMap::new();
    for (mesh, sides, operand) in [
        (&split_a, sides_a, Operand::A),
        (&split_b, sides_b, Operand::B),
    ] {
        let (points, faces) = mesh.to_triangles();
        for (mut face, side) in faces.into_iter().zip(sides) {
            let Some(reverse) = keep(operation, operand, side) else {
                continue;
            };
            if reverse {
                face.swap(1, 2);
            }
            triangles.push(face.map(|i| {
                let p = points[i];
                let xyz = [p.x, p.y, p.z].map(|v| if v == 0.0 { 0.0 } else { v });
                *indices.entry(xyz.map(f64::to_bits)).or_insert_with(|| {
                    let index = vertices.len();
                    vertices.push(Vec3::new(xyz[0], xyz[1], xyz[2]));
                    index
                })
            }));
        }
    }
    if triangles.is_empty() {
        return match operation {
            Operation::Intersection => Ok(Mesh::default()),
            Operation::Union | Operation::Difference => Err(BooleanError::EmptyResult),
        };
    }
    let mesh = Mesh::from_triangles(vertices, &triangles).map_err(BooleanError::Mesh)?;
    watertight_gate(mesh)
}

fn edges(mesh: &Mesh) -> BTreeSet<[usize; 2]> {
    mesh.halfedges
        .iter()
        .map(|h| {
            let (a, b) = (h.origin, mesh.halfedges[h.next].origin);
            [a.min(b), a.max(b)]
        })
        .collect()
}

fn assert_regions(mesh: &Mesh, seams: &BTreeSet<[usize; 2]>, expected: usize) -> Vec<usize> {
    let regions = face_regions(mesh, seams);
    assert_eq!(regions.len(), mesh.faces.len());
    let unique: BTreeSet<_> = regions.iter().copied().collect();
    assert_eq!(unique, (0..expected).collect());
    let first: Vec<_> = (0..expected)
        .map(|id| regions.iter().position(|&r| r == id).unwrap())
        .collect();
    assert!(first.windows(2).all(|pair| pair[0] < pair[1]));
    println!("regions: faces={}, count={expected}", regions.len());
    regions
}

#[test]
fn region_sides_equal_per_face_sides() {
    let mut checked = 0;
    for (name, a, b) in fixtures() {
        let Some(splits) = split_fixture(name, &a, &b) else {
            continue;
        };
        for ((split, seams), other) in splits.iter().zip([&b, &a]) {
            assert!(seams.is_subset(&edges(split)), "{name}: recovered seams");
            assert_eq!(
                classify_regions(split, seams, other),
                classify_faces(split, other),
                "{name}"
            );
            checked += 1;
        }
    }
    println!("side oracle: checked operands={checked}");
    assert!(checked > 0, "no fixture reached the side oracle");
    assert!(classify_regions(&Mesh::default(), &BTreeSet::new(), &Mesh::default()).is_empty());
}

#[test]
fn region_boolean_output_unchanged() {
    let operations: [(Operation, BooleanOp); 3] = [
        (Operation::Union, union),
        (Operation::Intersection, intersection),
        (Operation::Difference, difference),
    ];
    for (name, a, b) in fixtures() {
        for (operation, compose) in operations {
            let expected = old_compose(&a, &b, operation).map(|mesh| mesh.to_bytes());
            let actual = compose(&a, &b).map(|mesh| mesh.to_bytes());
            assert_eq!(actual, expected, "{name}: {operation:?}");
        }
    }
}

#[test]
fn regions_follow_the_cut_curve() {
    assert!(face_regions(&Mesh::default(), &BTreeSet::new()).is_empty());
    // Three faces share edge 0-1 with the same direction, so no twins exist.
    // Face 1 is disconnected: numbering must follow lowest face, not root id.
    let open = Mesh::from_triangles(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 1.0, 0.0),
        ],
        &[[0, 1, 2], [2, 3, 4], [0, 1, 3], [0, 1, 4]],
    )
    .unwrap();
    assert_eq!(face_regions(&open, &BTreeSet::new()), vec![0, 1, 0, 0]);
    assert_eq!(
        face_regions(&open, &BTreeSet::from([[0, 1]])),
        vec![0, 1, 2, 3]
    );
    let mut checked = 0;
    for (name, a, b) in fixtures() {
        for mesh in [&a, &b] {
            assert_regions(mesh, &BTreeSet::new(), 1);
            assert_eq!(
                face_regions(mesh, &edges(mesh)),
                (0..mesh.faces.len()).collect::<Vec<_>>()
            );
        }
        if !matches!(name, "overlapping" | "contained" | "disjoint") {
            continue;
        }
        let Some(splits) = split_fixture(name, &a, &b) else {
            continue;
        };
        for ((split, seams), other) in splits.iter().zip([&b, &a]) {
            let expected = if name == "overlapping" { 2 } else { 1 };
            let regions = assert_regions(split, seams, expected);
            assert_regions(split, &BTreeSet::new(), 1);
            assert_eq!(
                face_regions(split, &edges(split)),
                (0..split.faces.len()).collect::<Vec<_>>()
            );
            if name == "overlapping" {
                let sides = classify_regions(split, seams, other);
                let second = regions.iter().position(|&r| r == 1).unwrap();
                assert_ne!(sides[0], sides[second], "{name}: inside/outside");
            }
            checked += 1;
        }
    }
    println!("cut-curve checks: checked operands={checked}");
    assert!(checked > 0, "no fixture reached the cut-curve checks");
}

#[test]
fn region_classification_scales_to_k32() {
    let started = Instant::now();
    let a = grid_box(32, 2.0, [0.0; 3]);
    let b = grid_box(32, 2.0, [0.875, 0.625, 0.375]);
    let result = union(&a, &b).unwrap();
    assert!(result.is_watertight());
    assert_volume(&result, 6905.0 / 512.0);
    let splits = split_fixture("k32 overlapping", &a, &b).expect("k32 split must succeed");
    for (split, seams) in &splits {
        assert_regions(split, seams, 2);
    }
    writeln!(
        std::io::stdout(),
        "region_classification_scales_to_k32 elapsed={:?}",
        started.elapsed()
    )
    .expect("report k32 elapsed time");
}
