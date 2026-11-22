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
