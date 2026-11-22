//! Isotropic remeshing of closed triangular surfaces.

use crate::halfedge::Mesh;
use crate::vec3::Vec3;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemeshError {
    InvalidTarget,
    InvalidMesh,
    NumericalRange,
}

impl fmt::Display for RemeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for RemeshError {}

type Edge = (usize, usize);
type Faces = Vec<[usize; 3]>;

fn key(a: usize, b: usize) -> Edge {
    (a.min(b), a.max(b))
}

fn edges(faces: &[[usize; 3]]) -> BTreeMap<Edge, Vec<usize>> {
    let mut result = BTreeMap::<Edge, Vec<usize>>::new();
    for (i, t) in faces.iter().enumerate() {
        for j in 0..3 {
            result.entry(key(t[j], t[(j + 1) % 3])).or_default().push(i);
        }
    }
    result
}

fn normal(p: &[Vec3], t: [usize; 3]) -> Vec3 {
    (p[t[1]] - p[t[0]]).cross(p[t[2]] - p[t[0]])
}

fn finite(p: Vec3) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

// Splitting a marked edge in each incident triangle preserves orientation,
// including when several edges of the same original triangle are marked.
fn split(p: &mut Vec<Vec3>, faces: &mut Faces) {
    let mut marked = BTreeMap::new();
    for ((a, b), _) in edges(faces) {
        if (p[a] - p[b]).length() > 4.0 / 3.0 {
            let index = p.len();
            p.push((p[a] + p[b]) * 0.5);
            marked.insert((a, b), index);
        }
    }
    let mut result = Vec::new();
    for &triangle in faces.iter() {
        let mut pieces = vec![triangle];
        for j in 0..3 {
            let edge = key(triangle[j], triangle[(j + 1) % 3]);
            let Some(&mid) = marked.get(&edge) else {
                continue;
            };
            let mut next = Vec::new();
            for t in pieces {
                if let Some(k) = (0..3).find(|&k| key(t[k], t[(k + 1) % 3]) == edge) {
                    next.push([t[k], mid, t[(k + 2) % 3]]);
                    next.push([mid, t[(k + 1) % 3], t[(k + 2) % 3]]);
                } else {
                    next.push(t);
                }
            }
            pieces = next;
        }
        result.extend(pieces);
    }
    *faces = result;
}

fn collapse(p: &mut [Vec3], faces: &mut Faces) {
    let edge_map = edges(faces);
    let mut neighbors = vec![BTreeSet::new(); p.len()];
    let mut incident = vec![Vec::new(); p.len()];
    for &(a, b) in edge_map.keys() {
        neighbors[a].insert(b);
        neighbors[b].insert(a);
    }
    for (i, t) in faces.iter().enumerate() {
        for &v in t {
            incident[v].push(i);
        }
    }
    let mut touched = BTreeSet::new();
    for (&(a, b), shared) in &edge_map {
        if shared.len() != 2
            || (p[a] - p[b]).length() >= 0.8
            || touched.contains(&a)
            || touched.contains(&b)
            || neighbors[a].intersection(&neighbors[b]).count() != 2
        {
            continue;
        }
        let affected: BTreeSet<_> = incident[a].iter().chain(&incident[b]).copied().collect();
        if affected
            .iter()
            .any(|&i| faces[i].iter().any(|v| touched.contains(v)))
        {
            continue;
        }
        let midpoint = (p[a] + p[b]) * 0.5;
        if neighbors[a]
            .union(&neighbors[b])
            .any(|&v| v != a && v != b && (p[v] - midpoint).length() > 4.0 / 3.0)
        {
            continue;
        }
        let safe = affected.iter().all(|&i| {
            let t = faces[i];
            if t.contains(&a) && t.contains(&b) {
                return true;
            }
            let q = t.map(|v| if v == a || v == b { midpoint } else { p[v] });
            let n = (q[1] - q[0]).cross(q[2] - q[0]);
            n.dot(normal(p, t)) > 0.0
        });
        if !safe {
            continue;
        }
        for i in affected {
            touched.extend(faces[i]);
            for v in &mut faces[i] {
                if *v == b {
                    *v = a;
                }
            }
        }
        p[a] = midpoint;
    }
    faces.retain(|t| t[0] != t[1] && t[1] != t[2] && t[2] != t[0]);
}

fn flip(p: &[Vec3], faces: &mut [[usize; 3]]) {
    let mut edge_map = edges(faces);
    let mut degree = vec![0_i64; p.len()];
    for &(a, b) in edge_map.keys() {
        degree[a] += 1;
        degree[b] += 1;
    }
    let candidates: Vec<_> = edge_map.iter().map(|(&e, f)| (e, f.clone())).collect();
    let mut used = BTreeSet::new();
    for ((a, b), pair) in candidates {
        if pair.len() != 2 || pair.iter().any(|f| used.contains(f)) {
            continue;
        }
        let (i, j) = (pair[0], pair[1]);
        let t = faces[i];
        let k = (0..3)
            .find(|&k| key(t[k], t[(k + 1) % 3]) == (a, b))
            .unwrap();
        let (u, v, c) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
        let Some(&d) = faces[j].iter().find(|&&v| v != a && v != b) else {
            continue;
        };
        if c == d || edge_map.contains_key(&key(c, d)) {
            continue;
        }
        let before = [a, b, c, d]
            .map(|v| (degree[v] - 6).pow(2))
            .iter()
            .sum::<i64>();
        let after = (degree[a] - 7).pow(2)
            + (degree[b] - 7).pow(2)
            + (degree[c] - 5).pow(2)
            + (degree[d] - 5).pow(2);
        let length = (p[c] - p[d]).length();
        let replacement = [[c, d, v], [d, c, u]];
        if after >= before
            || !(0.6..=4.0 / 3.0).contains(&length)
            || replacement
                .iter()
                .any(|&r| normal(p, r).dot(normal(p, faces[i]) + normal(p, faces[j])) <= 0.0)
        {
            continue;
        }
        faces[i] = replacement[0];
        faces[j] = replacement[1];
        degree[a] -= 1;
        degree[b] -= 1;
        degree[c] += 1;
        degree[d] += 1;
        edge_map.remove(&(a, b));
        edge_map.insert(key(c, d), vec![i, j]);
        used.extend([i, j]);
    }
}

fn relax(p: &mut [Vec3], faces: &[[usize; 3]]) {
    let mut normals = vec![Vec3::default(); p.len()];
    let mut neighbors = vec![BTreeSet::new(); p.len()];
    for &t in faces {
        let n = normal(p, t);
        for j in 0..3 {
            normals[t[j]] = normals[t[j]] + n;
            neighbors[t[j]].extend([t[(j + 1) % 3], t[(j + 2) % 3]]);
        }
    }
    let proposed: Vec<_> = p
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            if neighbors[i].is_empty() {
                return v;
            }
            let weight = 1.0 / neighbors[i].len() as f64;
            let centroid = neighbors[i]
                .iter()
                .fold(Vec3::default(), |s, &j| s + p[j] * weight);
            let n = normals[i].normalized();
            let displacement = centroid - v;
            v + (displacement - n * displacement.dot(n)) * 0.5
        })
        .collect();
    if proposed.iter().all(|&v| finite(v))
        && faces
            .iter()
            .all(|&t| normal(&proposed, t).dot(normal(p, t)) > 0.0)
    {
        p.copy_from_slice(&proposed);
    }
}

/// Run a caller-selected number of split, collapse, valence-flip and tangential
/// relaxation sweeps. Input is unchanged on failure. Only closed manifolds are
/// accepted. Coordinates are computed in target-length units; the numerical
/// range bound keeps fourth-degree orientation predicates away from overflow.
pub fn remesh(mesh: &Mesh, target: f64, iterations: usize) -> Result<Mesh, RemeshError> {
    if !target.is_finite() || target <= 0.0 {
        return Err(RemeshError::InvalidTarget);
    }
    if !mesh.is_watertight() {
        return Err(RemeshError::InvalidMesh);
    }
    let (mut p, mut faces) = mesh.to_triangles();
    let bound = f64::MAX.sqrt().sqrt() / 16.0;
    for v in &mut p {
        *v = Vec3::new(v.x / target, v.y / target, v.z / target);
        if !finite(*v) || v.x.abs().max(v.y.abs()).max(v.z.abs()) > bound {
            return Err(RemeshError::NumericalRange);
        }
    }
    if faces.iter().any(|&t| normal(&p, t).length() == 0.0) {
        return Err(RemeshError::NumericalRange);
    }
    if iterations == 0 {
        return Ok(mesh.clone());
    }
    for _ in 0..iterations {
        split(&mut p, &mut faces);
        collapse(&mut p, &mut faces);
        flip(&p, &mut faces);
        relax(&mut p, &faces);
    }
    let mut indices = vec![None; p.len()];
    let mut vertices = Vec::new();
    for t in &mut faces {
        for v in t {
            let index = *indices[*v].get_or_insert_with(|| {
                let index = vertices.len();
                vertices.push(p[*v] * target);
                index
            });
            *v = index;
        }
    }
    if vertices.iter().any(|&v| !finite(v)) {
        return Err(RemeshError::NumericalRange);
    }
    let result = Mesh::from_triangles(vertices, &faces).map_err(|_| RemeshError::InvalidMesh)?;
    if !result.is_watertight() {
        return Err(RemeshError::InvalidMesh);
    }
    Ok(result)
}
