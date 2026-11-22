//! Face classification by positive-x ray parity. Exact predicates and a
//! symbolic transverse perturbation avoid tolerances and double-counted edges.

use crate::halfedge::Mesh;
use crate::predicates::{orient2d, orient3d};

type Point = [f64; 3];
type Triangle = [Point; 3];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Side {
    Inside,
    Outside,
    OnBoundarySame,
    OnBoundaryOpposite,
}

/// Classify each face of `mesh` relative to `solid`, in face order.
///
/// Inputs must have valid triangle topology and finite coordinates (as produced
/// by `Mesh::from_triangles`), and nondegenerate faces. `solid` must be closed.
/// Faces must already be split at intersections: classification samples the
/// rounded centroid, not the entire unsplit triangle. A face is on the boundary
/// when its three vertices are exactly coplanar with a solid face containing
/// the centroid. Boundary orientation is meaningful for coplanar faces, not for transverse contacts.
pub fn classify_faces(mesh: &Mesh, solid: &Mesh) -> Vec<Side> {
    let targets = triangles(solid);
    triangles(mesh)
        .into_iter()
        .map(|face| {
            let point = std::array::from_fn(|axis| {
                let values = face.map(|p| p[axis]);
                if values[0] == values[1] && values[1] == values[2] {
                    return values[0];
                }
                let scale = values.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
                if scale == 0.0 {
                    return 0.0;
                }
                // Normalize before summing, including for subnormal inputs.
                let mean = values.iter().map(|v| v / scale).sum::<f64>() / 3.0;
                mean.clamp(-1.0, 1.0) * scale
            });
            classify(point, face, &targets)
        })
        .collect()
}

fn triangles(mesh: &Mesh) -> Vec<Triangle> {
    let (vertices, indices) = mesh.to_triangles();
    indices
        .into_iter()
        .map(|t| t.map(|i| [vertices[i].x, vertices[i].y, vertices[i].z]))
        .collect()
}

fn project(p: Point, axis: usize) -> [f64; 2] {
    [p[(axis + 1) % 3], p[(axis + 2) % 3]]
}

fn orientation(t: Triangle, axis: usize) -> i8 {
    orient2d(
        project(t[0], axis),
        project(t[1], axis),
        project(t[2], axis),
    )
    .expect("finite mesh coordinates")
}

fn edge_sign(a: Point, b: Point, p: Point, axis: usize, perturb: bool) -> i8 {
    let [a, b, p] = [a, b, p].map(|v| project(v, axis));
    let sign = orient2d(a, b, p).expect("finite mesh coordinates");
    if sign != 0 || !perturb {
        return sign;
    }
    // p -> p + (epsilon, epsilon^2): first nonzero coefficient wins.
    if a[1] != b[1] {
        if a[1] > b[1] {
            1
        } else {
            -1
        }
    } else if b[0] > a[0] {
        1
    } else if b[0] < a[0] {
        -1
    } else {
        0
    }
}

fn covers(t: Triangle, p: Point, axis: usize, sign: i8, perturb: bool) -> bool {
    (0..3).all(|i| {
        let edge = edge_sign(t[i], t[(i + 1) % 3], p, axis, perturb);
        edge == sign || (!perturb && edge == 0)
    })
}

fn classify(p: Point, face: Triangle, targets: &[Triangle]) -> Side {
    let mut odd = false;
    for &t in targets {
        // Coplanar means all three face vertices lie exactly on the solid face's
        // plane; the rounded sample point is only used for coverage and parity.
        let coplanar = face
            .iter()
            .all(|&v| orient3d(t[0], t[1], t[2], v).expect("finite mesh coordinates") == 0);
        let plane = if coplanar {
            0
        } else {
            orient3d(t[0], t[1], t[2], p).expect("finite mesh coordinates")
        };
        if coplanar {
            for axis in 0..3 {
                let sign = orientation(t, axis);
                if sign != 0 && covers(t, p, axis, sign, false) {
                    return if orientation(face, axis) == sign {
                        Side::OnBoundarySame
                    } else {
                        Side::OnBoundaryOpposite
                    };
                }
            }
        }
        let sign = orientation(t, 0);
        if sign != 0 && plane == sign && covers(t, p, 0, sign, true) {
            odd = !odd;
        }
    }
    if odd {
        Side::Inside
    } else {
        Side::Outside
    }
}
