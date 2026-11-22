//! Arena hierarchy with iterative, parent-first world evaluation.

use crate::math::Mat4;
use crate::transform::Transform;

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub parent: Option<usize>,
    pub mesh: Option<u32>,
    pub transform: Transform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HierarchyError {
    Cycle,
    InvalidParent { node: usize, parent: usize },
    InvalidTransform { node: usize },
}

#[derive(Clone, Debug, Default)]
pub struct Hierarchy {
    pub nodes: Vec<Node>,
}

impl Hierarchy {
    /// Returns matrices in arena order; errors never publish partial results.
    /// No recursion or depth limit is needed, including for child-before-parent arenas.
    pub fn world_matrices(&self) -> Result<Vec<Mat4>, HierarchyError> {
        let mut states = vec![0_u8; self.nodes.len()];
        let mut world = vec![Mat4::IDENTITY; self.nodes.len()];
        let mut path = Vec::new();
        for start in 0..self.nodes.len() {
            let mut next = Some(start);
            while let Some(index) = next {
                match states[index] {
                    2 => break,
                    1 => return Err(HierarchyError::Cycle),
                    _ => {}
                }
                states[index] = 1;
                path.push(index);
                next = self.nodes[index].parent;
                if let Some(parent) = next {
                    if parent >= self.nodes.len() {
                        return Err(HierarchyError::InvalidParent {
                            node: index,
                            parent,
                        });
                    }
                }
            }
            while let Some(index) = path.pop() {
                let node = &self.nodes[index];
                let local = node
                    .transform
                    .to_matrix()
                    .ok_or(HierarchyError::InvalidTransform { node: index })?;
                let matrix = match node.parent {
                    Some(parent) => world[parent].mul(&local),
                    None => local,
                };
                if !matrix.m.iter().flatten().all(|v| v.is_finite()) {
                    return Err(HierarchyError::InvalidTransform { node: index });
                }
                world[index] = matrix;
                states[index] = 2;
            }
        }
        Ok(world)
    }
}
