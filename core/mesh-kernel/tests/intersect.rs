use mesh_kernel::gen::SeededRng;
use mesh_kernel::intersect::{intersect_triangles, Intersection, IntersectionError};
use mesh_kernel::vec3::Vec3;

fn triangle(points: [[f64; 3]; 3]) -> [Vec3; 3] {
    points.map(|[x, y, z]| Vec3::new(x, y, z))
}

fn segment(start: [f64; 3], end: [f64; 3]) -> Intersection {
    let [start, end, _] = triangle([start, end, end]);
    Intersection::Segment(start, end)
}

fn assert_both(a: [Vec3; 3], b: [Vec3; 3], expected: Intersection) {
    for first in [a, [a[1], a[2], a[0]], [a[2], a[1], a[0]]] {
        for second in [b, [b[2], b[0], b[1]], [b[1], b[0], b[2]]] {
            assert_eq!(intersect_triangles(first, second), Ok(expected.clone()));
            assert_eq!(intersect_triangles(second, first), Ok(expected.clone()));
        }
    }
}

// Independent separating-axis oracle for the small integer xy fixtures.
// Homogeneous i128 determinants are exact here, without production predicates,
// translated floating-point subtraction, or constructed intersection points.
fn integer_area(a: Vec3, b: Vec3, c: Vec3) -> i128 {
    let [ax, ay, bx, by, cx, cy] =
        [a.x, a.y, b.x, b.y, c.x, c.y].map(|coordinate| coordinate as i128);
    ax * by + bx * cy + cx * ay - ay * bx - by * cx - cy * ax
}

fn integer_coplanar_overlap(a: [Vec3; 3], b: [Vec3; 3]) -> bool {
    for (first, second) in [(a, b), (b, a)] {
        assert_ne!(
            integer_area(first[0], first[1], first[2]),
            0,
            "oracle requires nondegenerate triangles"
        );
        for i in 0..3 {
            let u = first[i];
            let v = first[(i + 1) % 3];
            let normal = [v.y as i128 - u.y as i128, u.x as i128 - v.x as i128];
            let project = |p: Vec3| p.x as i128 * normal[0] + p.y as i128 * normal[1];
            let left = first.map(project);
            let right = second.map(project);
            let left_min = left.iter().min().expect("three projections");
            let left_max = left.iter().max().expect("three projections");
            let right_min = right.iter().min().expect("three projections");
            let right_max = right.iter().max().expect("three projections");
            if left_max < right_min || right_max < left_min {
                return false;
            }
        }
    }
    true
}

#[test]
fn intersect_crossing_triangles_segment() {
    // At x=1, the first triangle covers y in [0,3]. At z=0, the
    // second triangle covers y in [0,2]. All endpoints are exact dyadics.
    let a = triangle([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]]);
    let b = triangle([[1.0, -1.0, -1.0], [1.0, 3.0, -1.0], [1.0, 1.0, 1.0]]);
    assert_both(a, b, segment([1.0, 0.0, 0.0], [1.0, 2.0, 0.0]));

    // The plane sections exist, but their intervals are disjoint.
    let separate = triangle([[1.0, 5.0, -1.0], [1.0, 9.0, -1.0], [1.0, 7.0, 1.0]]);
    assert_both(a, separate, Intersection::None);
    let parallel = a.map(|v| Vec3::new(v.x, v.y, 1.0));
    assert_both(a, parallel, Intersection::None);
}

#[test]
fn intersect_coplanar_overlap() {
    let a = triangle([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]]);
    let b = triangle([[1.0, 1.0, 0.0], [5.0, 1.0, 0.0], [1.0, 5.0, 0.0]]);
    assert_both(
        a,
        b,
        Intersection::CoplanarOverlap(vec![
            Vec3::new(1.0, 1.0, 0.0),
            Vec3::new(1.0, 3.0, 0.0),
            Vec3::new(3.0, 1.0, 0.0),
        ]),
    );
    assert_both(a, a, Intersection::CoplanarOverlap(vec![a[0], a[2], a[1]]));
    let contained = triangle([[1.0, 1.0, 0.0], [2.0, 1.0, 0.0], [1.0, 2.0, 0.0]]);
    assert_both(
        a,
        contained,
        Intersection::CoplanarOverlap(vec![contained[0], contained[2], contained[1]]),
    );
    let separate = triangle([[5.0, 5.0, 0.0], [6.0, 5.0, 0.0], [5.0, 6.0, 0.0]]);
    assert_both(a, separate, Intersection::None);
}

#[test]
fn intersect_coplanar_six_corners_and_all_projections() {
    // Exact rational line intersections: x=3/4, 3/2, 5/2, 13/4.
    let a = [[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [2.0, 4.0, 0.0]];
    let b = [[0.0, 3.0, 0.0], [4.0, 3.0, 0.0], [2.0, -1.0, 0.0]];
    let expected = [
        [0.75, 1.5, 0.0],
        [1.5, 0.0, 0.0],
        [2.5, 0.0, 0.0],
        [3.25, 1.5, 0.0],
        [2.5, 3.0, 0.0],
        [1.5, 3.0, 0.0],
    ];
    for permutation in [[0, 1, 2], [0, 2, 1], [2, 0, 1]] {
        let permute = |p: [f64; 3]| permutation.map(|i| p[i]);
        // Different projections change the canonical boundary ordering.
        let result = intersect_triangles(triangle(a.map(permute)), triangle(b.map(permute)))
            .expect("nondegenerate finite fixtures");
        let Intersection::CoplanarOverlap(mut actual) = result else {
            panic!("expected an overlap polygon, got {result:?}");
        };
        let mut reference: Vec<_> = expected.map(permute).map(|p| triangle([p; 3])[0]).into();
        let order = |p: &Vec3, q: &Vec3| {
            p.x.total_cmp(&q.x)
                .then(p.y.total_cmp(&q.y))
                .then(p.z.total_cmp(&q.z))
        };
        actual.sort_by(order);
        reference.sort_by(order);
        assert_eq!(actual, reference);
    }
}

#[test]
fn intersect_touching_at_vertex_and_edge() {
    let a = triangle([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 4.0, 0.0]]);
    for b in [
        triangle([[0.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, -1.0, 0.0]]),
        triangle([[0.0, 0.0, 0.0], [-1.0, 0.0, 1.0], [0.0, -1.0, 1.0]]),
    ] {
        assert_both(a, b, segment([0.0; 3], [0.0; 3]));
    }
    for b in [
        triangle([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, -4.0, 0.0]]),
        triangle([[0.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 4.0]]),
    ] {
        assert_both(a, b, segment([0.0; 3], [4.0, 0.0, 0.0]));
    }
    // Partial collinear overlap, with neither shared edge fully contained.
    let b = triangle([[2.0, 0.0, 0.0], [6.0, 0.0, 0.0], [6.0, -4.0, 0.0]]);
    assert_both(a, b, segment([2.0, 0.0, 0.0], [4.0, 0.0, 0.0]));

    // Mutation witness: (1+u)(1-u)-1 = -u^2 exactly, u=2^-52.
    // Replacing orientation2's exact predicate with the plain determinant
    // mislabels this valid touching triangle as degenerate, failing assert_both.
    let up = 1.0 + f64::EPSILON;
    let below = 1.0 - f64::EPSILON;
    assert_eq!(up * below - 1.0, 0.0, "float mutation must lose the sign");
    // Put the origin last in canonical vertex order, so the mutated
    // translated determinant actually evaluates the cancelling products.
    let thin = triangle([[0.0; 3], [-up, -1.0, 0.0], [-1.0, -below, 0.0]]);
    let touching = triangle([[0.0; 3], [1.0, 0.0, 1.0], [0.0, 1.0, 1.0]]);
    assert_both(thin, touching, segment([0.0; 3], [0.0; 3]));
    for exponent in [-1000, -256, 256, 1000] {
        let scale = f64::from_bits(((1023 + exponent) as u64) << 52);
        let up = f64::from_bits(scale.to_bits() + 1);
        let below = f64::from_bits(scale.to_bits() - 2);
        let thin = triangle([[0.0; 3], [-up, -scale, 0.0], [-scale, -below, 0.0]]);
        assert_both(thin, touching, segment([0.0; 3], [0.0; 3]));
    }
}

#[test]
fn intersect_seeded_swap_symmetry() {
    let mut rng = SeededRng::new(0x4d38);
    let mut counts = [0; 3];
    let mut valid = 0;
    for sample in 0..256 {
        let mut make = || {
            triangle(std::array::from_fn(|_| {
                std::array::from_fn(|axis| {
                    if sample % 2 == 0 && axis == 2 {
                        0.0
                    } else {
                        (rng.next_u64() % 17) as f64 - 8.0
                    }
                })
            }))
        };
        let a = make();
        let b = make();
        let result = intersect_triangles(a, b);
        assert_eq!(result, intersect_triangles(b, a), "sample={sample}");
        assert_eq!(
            result,
            intersect_triangles([a[2], a[0], a[1]], [b[1], b[0], b[2]]),
            "sample={sample}: vertex permutation"
        );
        match result {
            Ok(intersection) => {
                if sample % 2 == 0 {
                    let expected_overlap = integer_coplanar_overlap(a, b);
                    assert_eq!(
                        !matches!(intersection, Intersection::None),
                        expected_overlap,
                        "sample={sample}: independent integer separating-axis reference"
                    );
                }
                valid += 1;
                counts[match intersection {
                    Intersection::None => 0,
                    Intersection::Segment(_, _) => 1,
                    Intersection::CoplanarOverlap(_) => 2,
                }] += 1;
            }
            Err(IntersectionError::DegenerateTriangle) => {
                if sample % 2 == 0 {
                    assert!(
                        integer_area(a[0], a[1], a[2]) == 0 || integer_area(b[0], b[1], b[2]) == 0,
                        "sample={sample}: spurious degeneracy"
                    );
                }
            }
            Err(error) => panic!("sample={sample}: unexpected error {error}"),
        }
    }
    eprintln!("seed=0x4d38 valid={valid} none/segment/polygon={counts:?}");
    assert!(valid > 0);
    assert!(counts.into_iter().all(|count| count > 0));
}

#[test]
fn intersect_finite_extremes_and_cancellation() {
    // Independent integer/dyadic reference: the first triangle's x=0
    // section is [-scale,scale], the second's z=0 section is [-extent,extent].
    // Here extent <= scale, so the latter is the exact intersection.
    for (scale, extent) in [
        (f64::from_bits(1), f64::from_bits(1)),
        (f64::from_bits(4), f64::from_bits(1)),
        (f64::MIN_POSITIVE, f64::from_bits(1_u64 << 50)),
        (4.0, 1.0),
        (f64::MAX, f64::MAX / 4.0),
    ] {
        let a = triangle([
            [-scale, -scale, 0.0],
            [scale, -scale, 0.0],
            [0.0, scale, 0.0],
        ]);
        let b = triangle([
            [0.0, -extent, -scale],
            [0.0, -extent, scale],
            [0.0, extent, 0.0],
        ]);
        assert_both(a, b, segment([0.0, -extent, 0.0], [0.0, extent, 0.0]));
    }

    // Products overflow and opposite-sign coordinate subtraction overflows,
    // yet the midpoint of this vertical edge is exactly the origin.
    let m = f64::MAX;
    let a = triangle([[-m, -m, 0.0], [m, -m, 0.0], [0.0, m, 0.0]]);
    let b = triangle([[0.0, 0.0, -m], [0.0, 0.0, m], [0.0, m, m]]);
    assert_both(a, b, segment([0.0; 3], [0.0, m / 2.0, 0.0]));
}

#[test]
fn intersect_rational_rounding_matches_integer_reference() {
    // On z=0, the crossing edges meet y=1/(height+1) and y=1.
    // Small integers and their sums convert exactly;
    // binary64 division independently supplies correctly rounded references.
    let a = triangle([[-2.0, -2.0, 0.0], [2.0, -2.0, 0.0], [0.0, 2.0, 0.0]]);
    for height in [2, 3, 7, 31, 127] {
        let h = f64::from(height);
        let b = triangle([[0.0, 0.0, -1.0], [0.0, 1.0, h], [0.0, 1.0, -h]]);
        assert_both(a, b, segment([0.0, 1.0 / (h + 1.0), 0.0], [0.0, 1.0, 0.0]));
    }
}

#[test]
fn intersect_coplanar_rounding_does_not_change_topology() {
    // The exact overlap is (0,0), (scale/2,scale/2), (scale,0).
    // For the smallest subnormal, ties-to-even rounds the middle corner to
    // (0,0), but the exact intersection is still a polygon, not a segment.
    for (scale, half) in [
        (f64::from_bits(1), 0.0),
        (f64::from_bits(4), f64::from_bits(2)),
        (f64::MIN_POSITIVE, f64::from_bits(1_u64 << 51)),
        (1.0, 0.5),
        (f64::MAX, f64::MAX / 2.0),
    ] {
        let a = triangle([[0.0; 3], [scale, 0.0, 0.0], [0.0, scale, 0.0]]);
        let b = triangle([[0.0; 3], [scale, 0.0, 0.0], [scale, scale, 0.0]]);
        assert_both(
            a,
            b,
            Intersection::CoplanarOverlap(vec![
                Vec3::default(),
                Vec3::new(half, half, 0.0),
                Vec3::new(scale, 0.0, 0.0),
            ]),
        );
    }
}

#[test]
fn intersect_midpoints_round_ties_even_and_preserve_cancellation() {
    let a = triangle([
        [-f64::MAX, -f64::MAX, 0.0],
        [f64::MAX, -f64::MAX, 0.0],
        [0.0, f64::MAX, 0.0],
    ]);
    let tiny = f64::from_bits(1);
    let next = f64::from_bits(1.0_f64.to_bits() + 1);
    let next_next = f64::from_bits(1.0_f64.to_bits() + 2);
    let previous_maximum = f64::from_bits(f64::MAX.to_bits() - 1);
    // Exact dyadic averages, independently rounded by the ties-to-even rule.
    // The last two cases retain residuals after nearly complete cancellation.
    for (left, right, expected) in [
        (1.0, next, 1.0),
        (next, next_next, next_next),
        (0.0, tiny, 0.0),
        (tiny, f64::from_bits(2), f64::from_bits(2)),
        (-tiny, 0.0, 0.0),
        (-next_next, -next, -next_next),
        (1.0, -1.0 + f64::EPSILON, f64::EPSILON / 2.0),
        (
            f64::MAX,
            -previous_maximum,
            (f64::MAX - previous_maximum) / 2.0,
        ),
    ] {
        let b = triangle([[left, 0.0, -1.0], [right, 0.0, 1.0], [left, 1.0, -1.0]]);
        assert_both(a, b, segment([expected, 0.0, 0.0], [expected, 0.5, 0.0]));
    }
}

#[test]
fn intersect_rejects_nonfinite_and_degenerate_inputs() {
    let good = triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for vertex in 0..3 {
            for axis in 0..3 {
                let mut points = [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
                points[vertex][axis] = invalid;
                let bad = triangle(points);
                assert_eq!(
                    intersect_triangles(bad, good),
                    Err(IntersectionError::NonFiniteCoordinate)
                );
                assert_eq!(
                    intersect_triangles(good, bad),
                    Err(IntersectionError::NonFiniteCoordinate)
                );
            }
        }
    }
    for bad in [
        [Vec3::default(); 3],
        triangle([[0.0; 3], [1.0, 1.0, 1.0], [2.0, 2.0, 2.0]]),
        triangle([[-f64::MAX, 0.0, 0.0], [0.0; 3], [f64::MAX, 0.0, 0.0]]),
    ] {
        assert_eq!(
            intersect_triangles(bad, good),
            Err(IntersectionError::DegenerateTriangle)
        );
        assert_eq!(
            intersect_triangles(good, bad),
            Err(IntersectionError::DegenerateTriangle)
        );
    }
    let signed_zero = good.map(|v| Vec3::new(v.x, v.y, -0.0));
    assert_both(
        good,
        signed_zero,
        Intersection::CoplanarOverlap(vec![good[0], good[2], good[1]]),
    );
}
