//! Exact, deterministic constrained face splitting without coordinate construction.
//!
//! Intersection constructs exact rational points and rounds each coordinate to
//! nearest. Equal exact points therefore weld by equal coordinates across operands.
//! Rounding can break incidence, making exact on-segment and chart tests disagree
//! across a shared edge. `split_faces_tagged` uses incidence decided exactly before
//! rounding, without a geometric adjustment. Distinct exact points can round
//! together: intersection retains their tags; this splitter owns the merged vertex,
//! merges its tags by one rule, and refuses merges that would crack a shared edge.

use crate::halfedge::{Mesh, MeshError};
use crate::intersect::{Feature, Intersection, TaggedIntersection};
use crate::predicates::orient2d;
use crate::vec3::Vec3;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Bound::{Excluded, Unbounded};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitError {
    /// The number of cut lists differs from the number of faces.
    CutCountMismatch { faces: usize, cuts: usize },
    /// A cut coordinate is not finite.
    NonFiniteCut { face: usize },
    /// No axis projection of the face has nonzero area.
    DegenerateFace { face: usize },
    /// A point is outside its face and has no shared-edge incidence.
    PointOutsideFace { face: usize },
    /// A shared-edge point is strictly inside an incident face's chart.
    InconsistentEdgePoint { face: usize },
    /// Rounded points carry incompatible incidence on this face.
    TagConflict { face: usize },
    /// A tag disagrees with a local corner or the face's original boundary.
    TagMismatch { face: usize },
    /// Constraint interiors cross without a supplied vertex at the crossing.
    CrossingCuts { face: usize },
    /// The bounded flip queue could not recover a constraint.
    ConstraintNotRecovered { face: usize },
    /// An emitted triangle is degenerate or has the wrong winding.
    BadTriangle { face: usize },
    /// Mesh construction or input validation failed.
    Mesh(MeshError),
}

impl std::fmt::Display for SplitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for SplitError {}

/// Finite coordinates only; canonical zero makes equality and ordering agree.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point([f64; 3]);

impl Point {
    fn new(v: Vec3) -> Self {
        Self([v.x, v.y, v.z].map(|x| if x == 0.0 { 0.0 } else { x }))
    }

    fn vector(self) -> Vec3 {
        let [x, y, z] = self.0;
        Vec3::new(x, y, z)
    }

    fn compare(&self, other: &Self) -> Ordering {
        self.0
            .iter()
            .zip(other.0)
            .map(|(a, b)| a.total_cmp(&b))
            .find(|order| *order != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}

fn index(indices: &[(Point, usize)], p: Point) -> usize {
    let i = indices
        .binary_search_by(|(point, _)| point.compare(&p))
        .expect("every cut point has been numbered");
    indices[i].1
}

type Edge = (usize, usize);
type Triangle = [usize; 3];
type Adjacency = BTreeMap<Edge, Vec<(usize, usize)>>;

fn edge(a: usize, b: usize) -> Edge {
    (a.min(b), a.max(b))
}

fn triangle_edges(t: Triangle) -> [Edge; 3] {
    [edge(t[0], t[1]), edge(t[1], t[2]), edge(t[2], t[0])]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Location {
    Corner,
    Edge(Edge),
    Inside,
    Outside,
}

struct Chart<'a> {
    vertices: &'a [Vec3],
    axes: [usize; 2],
    sign: i8,
    face: usize,
}

impl Chart<'_> {
    fn project(&self, v: usize) -> [f64; 2] {
        let p = Point::new(self.vertices[v]);
        self.axes.map(|axis| p.0[axis])
    }

    fn turn(&self, a: usize, b: usize, c: usize) -> i8 {
        orient2d(self.project(a), self.project(b), self.project(c))
            .expect("all mesh and cut coordinates were checked finite")
    }

    fn on_segment(&self, p: usize, a: usize, b: usize) -> bool {
        let (p2, a2, b2) = (self.project(p), self.project(a), self.project(b));
        p2 != a2
            && p2 != b2
            && self.turn(a, b, p) == 0
            && (0..2).all(|i| a2[i].min(b2[i]) <= p2[i] && p2[i] <= a2[i].max(b2[i]))
    }

    fn classify(&self, t: Triangle, p: usize) -> Location {
        if t.iter().any(|&v| self.vertices[v] == self.vertices[p]) {
            return Location::Corner;
        }
        for (a, b) in triangle_edges(t) {
            if self.on_segment(p, a, b) {
                return Location::Edge(edge(a, b));
            }
        }
        if (0..3).all(|i| self.turn(t[i], t[(i + 1) % 3], p) == self.sign) {
            Location::Inside
        } else {
            Location::Outside
        }
    }

    fn crosses(&self, (a, b): Edge, (c, d): Edge) -> bool {
        self.turn(a, b, c) * self.turn(a, b, d) == -1
            && self.turn(c, d, a) * self.turn(c, d, b) == -1
    }

    fn validate(&self, t: Triangle) -> Result<(), SplitError> {
        if self.turn(t[0], t[1], t[2]) == self.sign {
            Ok(())
        } else {
            Err(SplitError::BadTriangle { face: self.face })
        }
    }

    fn wound(&self, mut t: Triangle) -> Triangle {
        if self.turn(t[0], t[1], t[2]) == -self.sign {
            t.swap(0, 1);
        }
        t
    }
}

// Do not call to_triangles on malformed public topology: it indexes unchecked.
fn input_triangles(mesh: &Mesh) -> Result<Vec<Triangle>, SplitError> {
    let invalid = SplitError::Mesh(MeshError::InvalidIndex);
    let count = mesh
        .faces
        .len()
        .checked_mul(3)
        .ok_or(SplitError::Mesh(MeshError::CountOverflow))?;
    if count != mesh.halfedges.len() {
        return Err(invalid);
    }
    let mut triangles = Vec::new();
    for (face, &start) in mesh.faces.iter().enumerate() {
        let mut current = start;
        let mut t = [0; 3];
        for v in &mut t {
            let h = mesh.halfedges.get(current).ok_or(invalid)?;
            if h.face != face || h.origin >= mesh.vertices.len() {
                return Err(invalid);
            }
            *v = h.origin;
            current = h.next;
        }
        if current != start {
            return Err(invalid);
        }
        if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
            return Err(SplitError::Mesh(MeshError::RepeatedVertex));
        }
        triangles.push(t);
    }
    Ok(triangles)
}

/// Re-triangulates faces so every cut segment becomes a chain of output edges.
///
/// M8 rounds constructed points to nearest f64. A point intended to be on an
/// edge can therefore land just off it. This function is exact on the received
/// values: it refuses inconsistent points with `PointOutsideFace`,
/// `InconsistentEdgePoint` or `BadTriangle`, rather than changing coordinates.
/// Use `split_faces_tagged` to preserve exact incidence of rounded cut points.
pub fn split_faces(mesh: &Mesh, cuts: &[Vec<Intersection>]) -> Result<Mesh, SplitError> {
    split_faces_with_seams(mesh, cuts).map(|(mesh, _)| mesh)
}

/// Like `split_faces`, also returning every recovered cut edge,
/// as `[min, max]` output vertex indices.
pub fn split_faces_with_seams(
    mesh: &Mesh,
    cuts: &[Vec<Intersection>],
) -> Result<(Mesh, BTreeSet<[usize; 2]>), SplitError> {
    split_body(mesh, cuts, None)
}

/// Like `split_faces_with_seams`; each point is placed by `on[0]` on this mesh.
pub fn split_faces_tagged(
    mesh: &Mesh,
    cuts: &[Vec<TaggedIntersection>],
) -> Result<(Mesh, BTreeSet<[usize; 2]>), SplitError> {
    let plain: Vec<Vec<_>> = cuts
        .iter()
        .map(|list| list.iter().map(TaggedIntersection::untagged).collect())
        .collect();
    split_body(mesh, &plain, Some(cuts))
}

fn merge_tags(
    tags: &[Feature],
    p: Point,
    t: Triangle,
    vertices: &[Vec3],
    face: usize,
) -> Result<Feature, SplitError> {
    let corner = t.iter().position(|&v| Point::new(vertices[v]) == p);
    let mut normalized = Vec::new();
    for &tag in tags {
        let tag = match tag {
            Feature::Vertex(k) if k >= 3 || corner != Some(k) => {
                return Err(SplitError::TagMismatch { face });
            }
            Feature::Edge(k, l)
                if k >= l || l >= 3 || corner.is_some_and(|m| m != k && m != l) =>
            {
                return Err(SplitError::TagMismatch { face });
            }
            Feature::Face | Feature::Edge(_, _) => corner.map_or(tag, Feature::Vertex),
            other => other,
        };
        normalized.push(tag);
    }
    let mut merged = normalized[0];
    for &tag in &normalized[1..] {
        merged = match (merged, tag) {
            (a, b) if a == b => a,
            (Feature::Face, e @ Feature::Edge(_, _))
            | (e @ Feature::Edge(_, _), Feature::Face) => e,
            _ => return Err(SplitError::TagConflict { face }),
        };
    }
    Ok(merged)
}

fn split_body(
    mesh: &Mesh,
    cuts: &[Vec<Intersection>],
    tagged: Option<&[Vec<TaggedIntersection>]>,
) -> Result<(Mesh, BTreeSet<[usize; 2]>), SplitError> {
    if cuts.len() != mesh.faces.len() {
        return Err(SplitError::CutCountMismatch {
            faces: mesh.faces.len(),
            cuts: cuts.len(),
        });
    }
    let input = input_triangles(mesh)?;
    if mesh
        .vertices
        .iter()
        .any(|v| !v.x.is_finite() || !v.y.is_finite() || !v.z.is_finite())
    {
        return Err(SplitError::Mesh(MeshError::NonFinitePosition));
    }
    let mut points = vec![Vec::new(); cuts.len()];
    let mut segments = vec![Vec::new(); cuts.len()];
    for (face, list) in cuts.iter().enumerate() {
        let mut add = |p: Vec3, q: Vec3| -> Result<(), SplitError> {
            if [p.x, p.y, p.z, q.x, q.y, q.z]
                .iter()
                .any(|x| !x.is_finite())
            {
                return Err(SplitError::NonFiniteCut { face });
            }
            let (p, q) = (Point::new(p), Point::new(q));
            points[face].extend([p, q]);
            if p != q {
                segments[face].push((p, q));
            }
            Ok(())
        };
        for cut in list {
            match cut {
                Intersection::None => {}
                Intersection::Segment(p, q) => add(*p, *q)?,
                Intersection::CoplanarOverlap(poly) => {
                    for (&p, &q) in poly.iter().zip(poly.iter().cycle().skip(1)) {
                        add(p, q)?;
                    }
                }
            }
        }
    }
    let mut vertices = mesh.vertices.clone();
    let mut indices: Vec<_> = vertices
        .iter()
        .enumerate()
        .map(|(i, &p)| (Point::new(p), i))
        .collect();
    indices.sort_by(|a, b| a.0.compare(&b.0));
    indices.dedup_by(|a, b| a.0 == b.0);
    let mut all: Vec<_> = points.iter().flatten().copied().collect();
    all.sort_by(Point::compare);
    all.dedup();
    all.retain(|p| indices.binary_search_by(|(q, _)| q.compare(p)).is_err());
    for p in all {
        indices.push((p, vertices.len()));
        vertices.push(p.vector());
    }
    indices.sort_by(|a, b| a.0.compare(&b.0));
    let mut own = vec![BTreeMap::new(); cuts.len()];
    let mut tagged_points = BTreeSet::new();
    // Validate and merge every face's tags before any chart or edge-point entry.
    if let Some(lists) = tagged {
        for (face, list) in lists.iter().enumerate() {
            let mut tags: BTreeMap<usize, Vec<Feature>> = BTreeMap::new();
            for cut in list {
                let endpoints;
                let points = match cut {
                    TaggedIntersection::None => &[][..],
                    TaggedIntersection::Segment(p, q) => {
                        endpoints = [*p, *q];
                        &endpoints[..]
                    }
                    TaggedIntersection::CoplanarOverlap(poly) => poly.as_slice(),
                };
                for p in points {
                    let i = index(&indices, Point::new(p.point));
                    tags.entry(i).or_default().push(p.on[0]);
                    tagged_points.insert(i);
                }
            }
            for (p, tags) in tags {
                own[face].insert(
                    p,
                    merge_tags(&tags, Point::new(vertices[p]), input[face], &vertices, face)?,
                );
            }
        }
    }
    let points: Vec<BTreeSet<usize>> = points
        .iter()
        .map(|set| set.iter().map(|&p| index(&indices, p)).collect())
        .collect();
    let segments: Vec<BTreeSet<Edge>> = segments
        .iter()
        .map(|list| {
            list.iter()
                .map(|&(a, b)| edge(index(&indices, a), index(&indices, b)))
                .collect()
        })
        .collect();
    let mut charts = Vec::new();
    let mut edge_points: BTreeMap<Edge, BTreeSet<usize>> = BTreeMap::new();
    for (face, &t) in input.iter().enumerate() {
        let mut chart = Chart {
            vertices: &vertices,
            axes: [0, 1],
            sign: 0,
            face,
        };
        for axes in [[0, 1], [0, 2], [1, 2]] {
            chart.axes = axes;
            chart.sign = chart.turn(t[0], t[1], t[2]);
            if chart.sign != 0 {
                break;
            }
        }
        if chart.sign == 0 {
            return Err(SplitError::DegenerateFace { face });
        }
        for &p in &points[face] {
            let location = match own[face].get(&p) {
                Some(Feature::Edge(k, l)) => Location::Edge(edge(t[*k], t[*l])),
                Some(_) => continue,
                None => chart.classify(t, p),
            };
            if let Location::Edge(e) = location {
                edge_points.entry(e).or_default().insert(p);
            }
        }
        charts.push(chart);
    }
    let mut output = Vec::new();
    let mut seams = BTreeSet::new();
    for (face, &t) in input.iter().enumerate() {
        let chart = &charts[face];
        let mut local_points = points[face].clone();
        let mut incidence = BTreeMap::new();
        // This is the single propagation site: every face reads every original
        // edge's points, even when that face's own cut list was empty.
        for e in triangle_edges(t) {
            if let Some(set) = edge_points.get(&e) {
                for &p in set {
                    if tagged_points.contains(&p) {
                        let other_edge = match own[face].get(&p) {
                            Some(Feature::Edge(k, l)) => Some(edge(t[*k], t[*l])),
                            _ => incidence.get(&p).copied(),
                        };
                        if t.iter()
                            .any(|&v| Point::new(vertices[v]) == Point::new(vertices[p]))
                            || other_edge.is_some_and(|other| other != e)
                        {
                            return Err(SplitError::TagConflict { face });
                        }
                    } else if chart.classify(t, p) == Location::Inside {
                        return Err(SplitError::InconsistentEdgePoint { face });
                    }
                    local_points.insert(p);
                    incidence.insert(p, e);
                }
            }
        }
        for &p in &local_points {
            if own[face].get(&p) == Some(&Feature::Face)
                && !incidence.contains_key(&p)
                && matches!(chart.classify(t, p), Location::Edge(_))
            {
                return Err(SplitError::TagMismatch { face });
            }
            if chart.classify(t, p) == Location::Outside && !incidence.contains_key(&p) {
                return Err(SplitError::PointOutsideFace { face });
            }
        }
        let mut local = vec![t];
        let mut chains: BTreeMap<_, _> = triangle_edges(t)
            .into_iter()
            .map(|(a, b)| ((a, b), vec![a, b]))
            .collect();
        for &p in &local_points {
            if matches!(own[face].get(&p), Some(Feature::Vertex(_)))
                || (!own[face].contains_key(&p) && chart.classify(t, p) == Location::Corner)
            {
                continue;
            }
            if let Some(e) = incidence.get(&p) {
                let chain = chains.get_mut(e).expect("incidence is on an original edge");
                chain.sort_by(|&a, &b| Point::new(vertices[a]).compare(&Point::new(vertices[b])));
                let key = Point::new(vertices[p]);
                let pair = chain.windows(2).find(|pair| {
                    Point::new(vertices[pair[0]]).compare(&key) == Ordering::Less
                        && key.compare(&Point::new(vertices[pair[1]])) == Ordering::Less
                });
                let pair = pair.ok_or(SplitError::BadTriangle { face })?;
                split_edge(&mut local, edge(pair[0], pair[1]), p, face)?;
                chain.push(p);
            } else {
                insert_inside(&mut local, p, chart)?;
            }
        }
        for &triangle in &local {
            chart.validate(triangle)?;
        }
        local_points.extend(t);
        let mut constraints = BTreeSet::new();
        for &(a, b) in &segments[face] {
            let mut chain = vec![a, b];
            chain.extend(
                local_points
                    .iter()
                    .copied()
                    .filter(|&p| chart.on_segment(p, a, b)),
            );
            chain.sort_by(|&a, &b| Point::new(vertices[a]).compare(&Point::new(vertices[b])));
            chain.dedup();
            constraints.extend(chain.windows(2).map(|pair| edge(pair[0], pair[1])));
        }
        for &a in &constraints {
            for &b in constraints.range((Excluded(a), Unbounded)) {
                if chart.crosses(a, b) {
                    return Err(SplitError::CrossingCuts { face });
                }
            }
        }
        recover(&mut local, &constraints, chart)?;
        seams.extend(constraints.iter().map(|&(a, b)| [a, b]));
        if local.len() == 1 && local[0] == t {
            output.push(t);
        } else {
            for triangle in local {
                chart.validate(triangle)?;
                output.push(triangle);
            }
        }
    }
    let mesh = Mesh::from_triangles(vertices, &output).map_err(SplitError::Mesh)?;
    Ok((mesh, seams))
}

fn split_edge(local: &mut Vec<Triangle>, e: Edge, p: usize, face: usize) -> Result<(), SplitError> {
    let mut found = false;
    for i in 0..local.len() {
        let t = local[i];
        for j in 0..3 {
            let (a, b, c) = (t[j], t[(j + 1) % 3], t[(j + 2) % 3]);
            if edge(a, b) == e {
                local[i] = [a, p, c];
                local.push([p, b, c]);
                found = true;
                break;
            }
        }
    }
    if found {
        Ok(())
    } else {
        Err(SplitError::BadTriangle { face })
    }
}

fn insert_inside(local: &mut Vec<Triangle>, p: usize, chart: &Chart<'_>) -> Result<(), SplitError> {
    for i in 0..local.len() {
        let [a, b, c] = local[i];
        let sides = [
            chart.turn(a, b, p),
            chart.turn(b, c, p),
            chart.turn(c, a, p),
        ];
        if sides.iter().all(|&s| s == chart.sign) {
            local[i] = [a, b, p];
            local.extend([[b, c, p], [c, a, p]]);
            return Ok(());
        }
        if sides.iter().all(|&s| s == 0 || s == chart.sign) {
            for (a, b) in triangle_edges([a, b, c]) {
                if chart.on_segment(p, a, b) {
                    return split_edge(local, edge(a, b), p, chart.face);
                }
            }
        }
    }
    Err(SplitError::BadTriangle { face: chart.face })
}

fn adjacency(local: &[Triangle]) -> Adjacency {
    let mut result = Adjacency::new();
    for (i, &t) in local.iter().enumerate() {
        for j in 0..3 {
            result
                .entry(edge(t[j], t[(j + 1) % 3]))
                .or_default()
                .push((i, t[(j + 2) % 3]));
        }
    }
    result
}

fn recover(
    local: &mut [Triangle],
    constraints: &BTreeSet<Edge>,
    chart: &Chart<'_>,
) -> Result<(), SplitError> {
    let error = SplitError::ConstraintNotRecovered { face: chart.face };
    let cap = local
        .len()
        .checked_mul(local.len())
        .and_then(|t| t.checked_add(16))
        .ok_or(error)?;
    let mut flips = 0;
    let mut protected = BTreeSet::new();
    for &target in constraints {
        let mut queue: VecDeque<_> = adjacency(local)
            .keys()
            .copied()
            .filter(|&e| chart.crosses(e, target))
            .collect();
        let mut stalled = 0;
        loop {
            let map = adjacency(local);
            if map.contains_key(&target) {
                protected.insert(target);
                break;
            }
            let e = queue.pop_front().ok_or(error)?;
            let Some(pair) = map.get(&e) else {
                continue;
            };
            let flippable = match pair.as_slice() {
                &[(_, c), (_, d)] => !protected.contains(&e) && chart.crosses(e, edge(c, d)),
                _ => false,
            };
            if !flippable {
                queue.push_back(e);
                stalled += 1;
                if stalled >= queue.len() {
                    return Err(error);
                }
                continue;
            }
            if flips >= cap {
                return Err(error);
            }
            let ((i, c), (j, d)) = (pair[0], pair[1]);
            local[i] = chart.wound([c, d, e.0]);
            local[j] = chart.wound([d, c, e.1]);
            flips += 1;
            stalled = 0;
            if chart.crosses(edge(c, d), target) {
                queue.push_back(edge(c, d));
            }
        }
    }
    Ok(())
}
