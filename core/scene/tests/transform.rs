use scene::hierarchy::{Hierarchy, HierarchyError, Node};
use scene::math::{Mat4, Quat, Vec3};
use scene::transform::Transform;

fn product(a: [[f64; 4]; 4], b: [[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, value) in row.iter_mut().enumerate() {
            for (k, right) in b.iter().enumerate() {
                *value += a[r][k] * right[c];
            }
        }
    }
    out
}

fn assert_reference(actual: Mat4, expected: [[f64; 4]; 4]) {
    for (r, row) in expected.iter().enumerate() {
        for (c, reference) in row.iter().enumerate() {
            let error = (f64::from(actual.m[r][c]) - reference).abs();
            // Relative plus absolute tolerance for accumulated f32 rounding.
            let tolerance = 2.0e-5 * reference.abs().max(1.0);
            assert!(error <= tolerance, "[{r}][{c}]: actual={}, reference={reference}, error={error}, tolerance={tolerance}", actual.m[r][c]);
        }
    }
}

#[test]
fn hierarchy_world_matrices_match_reference() {
    for seed in [1_u32, 17, 9281] {
        let mut state = seed;
        let mut hierarchy = Hierarchy::default();
        let mut locals = Vec::new();
        for index in 0..64 {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            // Parents have larger arena indices, exercising non-topological storage.
            let parent = if index == 63 || state % 7 == 0 {
                None
            } else {
                Some(index + 1 + state as usize % (63 - index))
            };
            let translation = Vec3::new((state % 11) as f32 - 5.0, 0.5, -0.25);
            let scale = Vec3::new(0.75, 1.0, 1.25);
            // Exact half-turns: the independent reference uses their known diagonals.
            let (rotation, diagonal) = match state % 3 {
                0 => (Quat::new(1.0, 0.0, 0.0, 0.0).unwrap(), [1.0, -1.0, -1.0]),
                1 => (Quat::new(0.0, 1.0, 0.0, 0.0).unwrap(), [-1.0, 1.0, -1.0]),
                _ => (Quat::new(0.0, 0.0, 1.0, 0.0).unwrap(), [-1.0, -1.0, 1.0]),
            };
            hierarchy.nodes.push(Node {
                parent,
                mesh: Some(index as u32),
                transform: Transform {
                    translation,
                    rotation,
                    scale,
                },
            });
            locals.push([
                [diagonal[0] * 0.75, 0.0, 0.0, f64::from(translation.x)],
                [0.0, diagonal[1], 0.0, 0.5],
                [0.0, 0.0, diagonal[2] * 1.25, -0.25],
                [0.0, 0.0, 0.0, 1.0],
            ]);
        }
        let actual = hierarchy.world_matrices().unwrap();
        for (index, matrix) in actual.into_iter().enumerate() {
            let mut expected = locals[index];
            let mut parent = hierarchy.nodes[index].parent;
            while let Some(index) = parent {
                expected = product(locals[index], expected);
                parent = hierarchy.nodes[index].parent;
            }
            assert_reference(matrix, expected);
        }
    }
}

#[test]
fn hierarchy_rejects_cycle() {
    let mut hierarchy = Hierarchy::default();
    for parent in [Some(1), Some(2), Some(0)] {
        hierarchy.nodes.push(Node {
            parent,
            mesh: None,
            transform: Transform::default(),
        });
    }
    assert_eq!(hierarchy.world_matrices(), Err(HierarchyError::Cycle));
    hierarchy.nodes[0].parent = Some(0);
    assert_eq!(hierarchy.world_matrices(), Err(HierarchyError::Cycle));
}

#[test]
fn hierarchy_validates_and_handles_empty() {
    let mut hierarchy = Hierarchy::default();
    assert_eq!(hierarchy.world_matrices(), Ok(Vec::new()));
    hierarchy.nodes.push(Node {
        parent: Some(1),
        mesh: None,
        transform: Transform::default(),
    });
    assert_eq!(
        hierarchy.world_matrices(),
        Err(HierarchyError::InvalidParent { node: 0, parent: 1 })
    );
    hierarchy.nodes[0].parent = None;
    hierarchy.nodes[0].transform.scale.x = f32::NAN;
    assert_eq!(
        hierarchy.world_matrices(),
        Err(HierarchyError::InvalidTransform { node: 0 })
    );
    hierarchy.nodes[0].transform.scale.x = f32::MAX;
    hierarchy.nodes.push(Node {
        parent: Some(0),
        mesh: None,
        transform: hierarchy.nodes[0].transform,
    });
    assert_eq!(
        hierarchy.world_matrices(),
        Err(HierarchyError::InvalidTransform { node: 1 })
    );
}

#[test]
fn quaternion_and_trs_edges() {
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(Quat::new(invalid, 0.0, 0.0, 1.0).is_none());
        assert!(Mat4::from_trs(
            Vec3::new(invalid, 0.0, 0.0),
            Quat::IDENTITY,
            Vec3::new(1.0, 1.0, 1.0)
        )
        .is_none());
    }
    assert!(Quat::new(0.0, 0.0, 0.0, 0.0).is_none());
    for magnitude in [f32::from_bits(1), f32::MIN_POSITIVE, 1.0, f32::MAX] {
        let rotation = Quat::new(0.0, magnitude, 0.0, 0.0).unwrap();
        let matrix =
            Mat4::from_trs(Vec3::default(), rotation, Vec3::new(magnitude, 1.0, 1.0)).unwrap();
        assert_eq!(matrix.m[0][0], -magnitude);
        assert_eq!(matrix.m[2][2], -1.0);
    }
    let t = Vec3::new(3.0, -2.0, 5.0);
    let q = Quat::new(0.0, 0.0, 1.0, 1.0).unwrap();
    let s = Vec3::new(2.0, 3.0, -4.0);
    let forward = Mat4::from_trs(t, q, s).unwrap();
    // A quarter-turn about Z: scale (1, 2, 3), rotate, then translate.
    let point = forward.transform_point3(Vec3::new(1.0, 2.0, 3.0));
    for (actual, expected) in [(point.x, -3.0_f64), (point.y, 0.0), (point.z, -7.0)] {
        assert!((f64::from(actual) - expected).abs() <= 2.0e-5);
    }
    let inverse = Mat4::inverse_trs(t, q, s).unwrap();
    assert_reference(
        forward.mul(&inverse),
        [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    );
    assert!(Mat4::inverse_trs(t, q, Vec3::new(0.0, 1.0, 1.0)).is_none());
    assert!(Mat4::inverse_trs(
        Vec3::default(),
        Quat::IDENTITY,
        Vec3::new(f32::from_bits(1), 1.0, 1.0)
    )
    .is_none());
}

#[test]
fn quaternion_matrix_matches_independent_rodrigues_reference() {
    // Reference matrices from the Rodrigues formula in Python (f64), not from the quaternion formula under test.
    // 90 degrees about the axis (1, 2, 3) and 30 degrees about z.
    let cases: [([f32; 4], [[f64; 3]; 3]); 2] = [
        (
            [
                0.188_982_24,
                0.377_964_47,
                0.566_946_7,
                std::f32::consts::FRAC_1_SQRT_2,
            ],
            [
                [0.07142857142857148, -0.6589265828801303, 0.7488081981105631],
                [0.9446408685944161, 0.28571428571428575, 0.16131018665900415],
                [-0.3202367695391345, 0.6958326704838529, 0.6428571428571429],
            ],
        ),
        (
            [0.0, 0.0, 0.258_819_05, 0.965_925_8],
            [
                [0.8660254037844387, -0.49999999999999994, 0.0],
                [0.49999999999999994, 0.8660254037844387, 0.0],
                [0.0, 0.0, 1.0],
            ],
        ),
    ];
    let mut matrices = Vec::new();
    for (q, reference) in cases {
        let rotation = Quat::new(q[0], q[1], q[2], q[3]).unwrap();
        let matrix = Mat4::from_trs(Vec3::default(), rotation, Vec3::new(1.0, 1.0, 1.0)).unwrap();
        for (r, row) in reference.iter().enumerate() {
            for (c, expected) in row.iter().enumerate() {
                let error = (f64::from(matrix.m[r][c]) - expected).abs();
                // The matrix and quaternion are stored as f32, so 1e-12 is unreachable; 1e-6 is about 10 f32 ulps
                // and far below the error of any wrong sign or term (at least 0.1 for these rotations).
                assert!(
                    error <= 1.0e-6,
                    "[{r}][{c}]: {} vs {expected}",
                    matrix.m[r][c]
                );
            }
        }
        matrices.push(matrix);
    }
    // A parent turned 90 degrees about (1, 2, 3) with a child turned 30 degrees about z composes as parent * child.
    let mut hierarchy = Hierarchy::default();
    for (index, (q, _)) in cases.iter().enumerate() {
        hierarchy.nodes.push(Node {
            parent: if index == 1 { Some(0) } else { None },
            mesh: None,
            transform: Transform {
                translation: Vec3::default(),
                rotation: Quat::new(q[0], q[1], q[2], q[3]).unwrap(),
                scale: Vec3::new(1.0, 1.0, 1.0),
            },
        });
    }
    let world = hierarchy.world_matrices().unwrap();
    let product = matrices[0].mul(&matrices[1]);
    for r in 0..4 {
        for c in 0..4 {
            assert!((world[1].m[r][c] - product.m[r][c]).abs() <= 1.0e-6);
        }
    }
}

#[test]
fn quat_components_returns_normalized_xyzw() {
    assert_eq!(
        Quat::IDENTITY.components().map(f32::to_bits),
        [0, 0, 0, 0x3f80_0000]
    );
    assert_eq!(
        Quat::new(0.0, 0.0, 0.0, 2.0)
            .unwrap()
            .components()
            .map(f32::to_bits),
        [0, 0, 0, 0x3f80_0000]
    );
    assert_eq!(
        Quat::new(-0.0, 0.0, 0.0, 1.0).unwrap().components()[0].to_bits(),
        0x8000_0000
    );
    let expected = [1.0_f64, 2.0, 3.0, 4.0].map(|v| ((v / 30_f64.sqrt()) as f32).to_bits());
    assert_eq!(
        Quat::new(1.0, 2.0, 3.0, 4.0)
            .unwrap()
            .components()
            .map(f32::to_bits),
        expected
    );
    assert_eq!(
        Quat::new(0.0, 0.0, 0.0, -1.0).unwrap().components()[3],
        -1.0
    );
}
