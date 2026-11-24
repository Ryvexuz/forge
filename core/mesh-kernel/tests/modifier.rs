use mesh_kernel::attr::{compute_vertex_normals, AttrStore, Attribute, Domain};
use mesh_kernel::gen::SeededRng;
use mesh_kernel::halfedge::Mesh;
use mesh_kernel::modifier::*;
use mesh_kernel::remesh::RemeshError;
use mesh_kernel::undo::{Command, CommandError, History, UndoError};
use mesh_kernel::vec3::Vec3;
use std::collections::BTreeMap;

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
        (actual - expected).abs() <= expected.abs() * 1e-9,
        "signed volume: actual={actual}, analytic={expected}"
    );
}

// Copied from tests/boolean.rs: positive k and dyadic size, offset and spacing.
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

fn evaluation_stack(tool: &Mesh, levels: usize) -> Stack {
    let mut stack = Stack::new();
    for modifier in [
        Modifier::Boolean {
            op: BoolOp::Union,
            tool: tool.clone(),
        },
        Modifier::Subdivide { levels },
        rem(0.5, 2),
    ] {
        stack.insert(stack.len(), modifier).unwrap();
    }
    stack
}

fn rem(target: f64, iterations: usize) -> Modifier {
    Modifier::Remesh { target, iterations }
}

#[test]
fn modifier_stack_leaves_base_untouched() {
    let mesh = grid_box(2, 2.0, [0.0; 3]);
    let mut attrs = AttrStore::default();
    let normals = compute_vertex_normals(&mesh).unwrap();
    attrs
        .insert(Domain::Vertex, "N", Attribute::Normal(normals))
        .unwrap();
    let uv = mesh
        .halfedges
        .iter()
        .map(|h| {
            let p = mesh.vertices[h.origin];
            [p.x, p.y]
        })
        .collect();
    attrs
        .insert(Domain::Corner, "uv", Attribute::Uv(uv))
        .unwrap();
    let base = Base { mesh, attrs };
    let mesh_bytes = base.mesh.to_bytes();
    let attr_bytes = base.attrs.to_bytes();
    assert_eq!(Stack::new().evaluate(&base).unwrap().to_bytes(), mesh_bytes);
    let tool = grid_box(2, 1.0, [0.875, 0.625, 0.375]);
    let mut stack = evaluation_stack(&tool, 1);
    let mut results = Vec::new();
    for _ in 0..3 {
        let result = stack.evaluate(&base).unwrap();
        assert!(result.is_watertight());
        results.push(result.to_bytes());
        assert_eq!(base.mesh.to_bytes(), mesh_bytes);
        assert_eq!(base.attrs.to_bytes(), attr_bytes);
    }
    assert_eq!(results[0], results[1]);
    assert_eq!(results[1], results[2]);
    stack.replace(1, Modifier::Subdivide { levels: 2 }).unwrap();
    let changed = stack.evaluate(&base).unwrap();
    assert!(changed.is_watertight());
    assert_ne!(changed.to_bytes(), results[0]);
    let fresh = evaluation_stack(&tool, 2).evaluate(&base).unwrap();
    assert_eq!(changed.to_bytes(), fresh.to_bytes());
    assert_eq!(base.mesh.to_bytes(), mesh_bytes);
    assert_eq!(base.attrs.to_bytes(), attr_bytes);
}

fn random_modifier(rng: &mut SeededRng, tool: &Mesh, kind: u64) -> Modifier {
    let operations = [BoolOp::Union, BoolOp::Intersection, BoolOp::Difference];
    match kind {
        0 => Modifier::Subdivide {
            levels: (rng.next_u64() % 4) as usize,
        },
        1 => Modifier::Remesh {
            target: [0.25, 0.5, 1.0][(rng.next_u64() % 3) as usize],
            iterations: (rng.next_u64() % 4) as usize,
        },
        _ => Modifier::Boolean {
            op: operations[(rng.next_u64() % 3) as usize],
            tool: tool.clone(),
        },
    }
}

#[test]
fn modifier_stack_10000_edits_undo_to_start() {
    let mut history = History::<Stack>::new(10_000).unwrap();
    let mut rng = SeededRng::new(0x2314);
    let tool = grid_box(1, 1.0, [0.25; 3]);
    let mut stack = Stack::new();
    stack.insert(0, Modifier::Subdivide { levels: 1 }).unwrap();
    let mut snapshots = vec![stack.to_bytes()];
    let mut counts = [0; 4];
    for _ in 0..10_000 {
        let mut kind = (rng.next_u64() % 4) as usize;
        let len = stack.len();
        if kind == 0 && len >= 8 {
            kind = 1;
        }
        if len == 0 || (kind == 2 && len < 2) {
            kind = 0;
        }
        let cmd: Box<dyn Command<Stack>> = match kind {
            0 => {
                let index = (rng.next_u64() % (len + 1) as u64) as usize;
                let variant = rng.next_u64() % 3;
                let modifier = random_modifier(&mut rng, &tool, variant);
                Box::new(AddModifier::new(index, modifier))
            }
            1 => Box::new(RemoveModifier::new((rng.next_u64() % len as u64) as usize)),
            2 => Box::new(MoveModifier::new(
                (rng.next_u64() % len as u64) as usize,
                (rng.next_u64() % len as u64) as usize,
            )),
            _ => {
                let index = (rng.next_u64() % len as u64) as usize;
                let variant = match stack.modifiers()[index] {
                    Modifier::Subdivide { .. } => 0,
                    Modifier::Remesh { .. } => 1,
                    Modifier::Boolean { .. } => 2,
                };
                let modifier = random_modifier(&mut rng, &tool, variant);
                Box::new(SetModifier::new(index, modifier))
            }
        };
        history.execute(&mut stack, cmd).unwrap();
        counts[kind] += 1;
        snapshots.push(stack.to_bytes());
    }
    println!("edit-kind counts [add, remove, move, set]: {counts:?}");
    assert!(counts.iter().all(|&count| count >= 1_000), "{counts:?}");
    assert_eq!(history.undo_len(), 10_000);
    assert_eq!(history.dropped(), 0);
    for k in 1..=10_000 {
        assert_eq!(history.undo(&mut stack), Ok(true));
        assert_eq!(stack.to_bytes(), snapshots[10_000 - k], "undo {k}");
    }
    assert_eq!(history.undo(&mut stack), Ok(false));
    assert_eq!(stack.to_bytes(), snapshots[0]);
    for (k, snapshot) in snapshots.iter().enumerate().skip(1) {
        assert_eq!(history.redo(&mut stack), Ok(true));
        assert_eq!(&stack.to_bytes(), snapshot, "redo {k}");
    }
    assert_eq!(history.redo(&mut stack), Ok(false));
}

fn refused(stack: &Stack, before: &[u8], result: Result<(), StackError>, expected: StackError) {
    assert_eq!(result, Err(expected));
    assert_eq!(stack.to_bytes(), before);
}

#[test]
fn stack_and_history_refuse_bad_edits() {
    assert!(matches!(
        History::<Stack>::new(0),
        Err(UndoError::InvalidCap)
    ));
    let mut stack = Stack::new();
    let sub = Modifier::Subdivide { levels: 1 };
    let before = stack.to_bytes();
    let result = stack.insert(2, sub.clone());
    refused(&stack, &before, result, bad_index(2, 0));
    assert_eq!(stack.remove(0), Err(bad_index(0, 0)));
    assert_eq!(stack.to_bytes(), before);
    stack.insert(0, sub.clone()).unwrap();
    stack.insert(1, sub.clone()).unwrap();
    let before = stack.to_bytes();
    let result = stack.move_modifier(0, 5);
    refused(&stack, &before, result, bad_index(5, 2));
    let result = stack.move_modifier(9, 5);
    refused(&stack, &before, result, bad_index(9, 2));
    assert_eq!(
        stack.replace(0, rem(0.5, 1)),
        Err(StackError::KindMismatch { index: 0 })
    );
    assert_eq!(stack.to_bytes(), before);
    for invalid in [
        Modifier::Subdivide { levels: 5 },
        rem(0.0, 1),
        rem(-1.0, 1),
        rem(f64::NAN, 1),
        rem(f64::INFINITY, 1),
        rem(f64::NEG_INFINITY, 1),
    ] {
        assert!(matches!(
            stack.insert(0, invalid),
            Err(StackError::InvalidSetting { .. })
        ));
        assert_eq!(stack.to_bytes(), before);
    }
    assert!(matches!(
        stack.replace(0, Modifier::Subdivide { levels: 5 }),
        Err(StackError::InvalidSetting { .. })
    ));
    assert_eq!(stack.to_bytes(), before);
    let mut remesh_stack = Stack::new();
    remesh_stack.insert(0, rem(f64::from_bits(1), 0)).unwrap();
    for target in [0.0, f64::NAN, f64::INFINITY] {
        let before = remesh_stack.to_bytes();
        assert!(matches!(
            remesh_stack.replace(0, rem(target, 0)),
            Err(StackError::InvalidSetting { .. })
        ));
        assert_eq!(remesh_stack.to_bytes(), before);
    }
    remesh_stack.replace(0, rem(f64::MAX, 0)).unwrap();
    let mut stack = Stack::new();
    let before = stack.to_bytes();
    let mut history = History::new(2).unwrap();
    let invalid = Box::new(AddModifier::new(9, sub.clone()));
    assert!(matches!(
        history.execute(&mut stack, invalid),
        Err(UndoError::ApplyFailed(_))
    ));
    assert_eq!(history.undo_len(), 0);
    assert_eq!(history.redo_len(), 0);
    assert_eq!(stack.to_bytes(), before);
    for _ in 0..3 {
        let add = Box::new(AddModifier::new(0, sub.clone()));
        history.execute(&mut stack, add).unwrap();
    }
    assert_eq!(history.undo_len(), 2);
    assert_eq!(history.dropped(), 1);
    assert_eq!(history.undo(&mut stack), Ok(true));
    let add = Box::new(AddModifier::new(0, sub));
    history.execute(&mut stack, add).unwrap();
    assert_eq!(history.redo_len(), 0);
    let mesh = Mesh::from_triangles(
        vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ],
        &[[0, 1, 2]],
    )
    .unwrap();
    let base = Base {
        mesh,
        attrs: AttrStore::default(),
    };
    let mut stack = Stack::new();
    stack.insert(0, rem(0.5, 1)).unwrap();
    assert_eq!(
        stack.evaluate(&base),
        Err(ModifierError {
            index: 0,
            failure: ModifierFailure::Remesh(RemeshError::InvalidMesh),
        })
    );
    stack.insert(0, Modifier::Subdivide { levels: 0 }).unwrap();
    assert!(matches!(
        stack.evaluate(&base),
        Err(ModifierError {
            index: 0,
            failure: ModifierFailure::Subdivide(_),
        })
    ));
    check_failed_history_calls();
}

// A sentinel in the existing document requests a failure, without mutation.
struct Fallible;

impl Command<Vec<i64>> for Fallible {
    fn apply(&mut self, doc: &mut Vec<i64>) -> Result<(), CommandError> {
        if doc.first() == Some(&-1) {
            return Err(CommandError::new("apply sentinel"));
        }
        doc.push(7);
        Ok(())
    }
    fn undo(&mut self, doc: &mut Vec<i64>) -> Result<(), CommandError> {
        if doc.first() == Some(&-2) {
            return Err(CommandError::new("undo sentinel"));
        }
        let _ = doc.pop();
        Ok(())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

fn check_failed_history_calls() {
    let mut history = History::new(2).unwrap();
    let mut doc = vec![0];
    history.execute(&mut doc, Box::new(Fallible)).unwrap();
    doc[0] = -2;
    assert_eq!(
        history.undo(&mut doc),
        Err(UndoError::UndoFailed(CommandError::new("undo sentinel")))
    );
    assert_eq!(doc, vec![-2, 7]);
    assert_eq!((history.undo_len(), history.redo_len()), (1, 0));
    doc[0] = 0;
    assert_eq!(history.undo(&mut doc), Ok(true));
    doc[0] = -1;
    assert_eq!(
        history.redo(&mut doc),
        Err(UndoError::RedoFailed(CommandError::new("apply sentinel")))
    );
    assert!(matches!(
        history.execute(&mut doc, Box::new(Fallible)),
        Err(UndoError::ApplyFailed(_))
    ));
    assert_eq!(doc, vec![-1]);
    assert_eq!((history.undo_len(), history.redo_len()), (0, 1));
    assert_eq!(history.dropped(), 0);
    doc[0] = 0;
    assert_eq!(history.redo(&mut doc), Ok(true));
    assert_eq!(doc, vec![0, 7]);
}

fn bad_index(index: usize, len: usize) -> StackError {
    StackError::IndexOutOfRange { index, len }
}

struct Push(Vec<i64>);

impl Command<Vec<i64>> for Push {
    fn apply(&mut self, doc: &mut Vec<i64>) -> Result<(), CommandError> {
        doc.extend_from_slice(&self.0);
        Ok(())
    }
    fn undo(&mut self, doc: &mut Vec<i64>) -> Result<(), CommandError> {
        if !doc.ends_with(&self.0) {
            return Err(CommandError::new("push suffix missing"));
        }
        doc.truncate(doc.len() - self.0.len());
        Ok(())
    }
    fn merge(&mut self, next: &dyn Command<Vec<i64>>) -> bool {
        if let Some(next) = next.as_any().downcast_ref::<Self>() {
            self.0.extend_from_slice(&next.0);
            true
        } else {
            false
        }
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn history_merges_only_without_barrier() {
    let mut history = History::new(10).unwrap();
    let mut doc = Vec::new();
    history.execute(&mut doc, Box::new(Push(vec![1]))).unwrap();
    history.execute(&mut doc, Box::new(Push(vec![2]))).unwrap();
    assert_eq!(history.undo_len(), 1);
    assert_eq!(history.undo(&mut doc), Ok(true));
    assert!(doc.is_empty());
    assert_eq!(history.redo(&mut doc), Ok(true));
    assert_eq!(doc, vec![1, 2]);
    history.execute(&mut doc, Box::new(Push(vec![3]))).unwrap();
    assert_eq!(history.undo_len(), 2);
    assert_eq!(history.undo(&mut doc), Ok(true));
    history.execute(&mut doc, Box::new(Push(vec![4]))).unwrap();
    assert_eq!(history.undo_len(), 2);
    assert_eq!(history.redo_len(), 0);
    history.break_merge();
    history.execute(&mut doc, Box::new(Push(vec![5]))).unwrap();
    assert_eq!(history.undo_len(), 3);
    assert_eq!(doc, vec![1, 2, 4, 5]);
}
