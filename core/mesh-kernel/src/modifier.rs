//! Ordered modifiers; the non-destructive rule lives only in `Stack::evaluate`,
//! which clones the untouched base before applying operations, without caching.
//! Changes to the plan: evaluate returns Result<Mesh, ModifierError>; the copied
//! 0.6 undo shape has no groups or poisoning. Attributes are not propagated.
//! Boolean operations retain M11's exact-input limit: rounded cuts are refused.

use crate::attr::AttrStore;
use crate::boolean::{difference, intersection, union, BooleanError};
use crate::halfedge::Mesh;
use crate::remesh::{remesh, RemeshError};
use crate::subdiv::{try_catmull_clark, PolyMesh, SubdivError};
use crate::undo::{Command, CommandError};

/// Largest accepted subdivision level (each level multiplies faces by 4 or more).
pub const MAX_SUBDIV_LEVELS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoolOp {
    Union,
    Intersection,
    Difference,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Modifier {
    Subdivide {
        levels: usize,
    },
    Remesh {
        target: f64,
        iterations: usize,
    },
    /// Current mesh op tool; difference means current minus tool.
    Boolean {
        op: BoolOp,
        tool: Mesh,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Base {
    pub mesh: Mesh,
    pub attrs: AttrStore,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StackError {
    IndexOutOfRange { index: usize, len: usize },
    KindMismatch { index: usize },
    InvalidSetting { reason: &'static str },
}

#[derive(Clone, Debug, PartialEq)]
pub enum ModifierFailure {
    Subdivide(SubdivError),
    Remesh(RemeshError),
    Boolean(BooleanError),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ModifierError {
    pub index: usize,
    pub failure: ModifierFailure,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stack {
    modifiers: Vec<Modifier>,
}

impl Modifier {
    fn validate(&self) -> Result<(), StackError> {
        match self {
            Self::Subdivide { levels } if *levels > MAX_SUBDIV_LEVELS => {
                Err(StackError::InvalidSetting {
                    reason: "subdivision level exceeds MAX_SUBDIV_LEVELS",
                })
            }
            Self::Remesh { target, .. } if !target.is_finite() || *target <= 0.0 => {
                Err(StackError::InvalidSetting {
                    reason: "remesh target must be finite and positive",
                })
            }
            _ => Ok(()),
        }
    }
}

impl Stack {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.modifiers.len()
    }

    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty()
    }

    pub fn modifiers(&self) -> &[Modifier] {
        &self.modifiers
    }

    fn check_index(&self, index: usize) -> Result<(), StackError> {
        if index >= self.len() {
            return Err(StackError::IndexOutOfRange {
                index,
                len: self.len(),
            });
        }
        Ok(())
    }

    /// Insert at an index in 0..=len.
    pub fn insert(&mut self, index: usize, modifier: Modifier) -> Result<(), StackError> {
        if index > self.len() {
            return Err(StackError::IndexOutOfRange {
                index,
                len: self.len(),
            });
        }
        modifier.validate()?;
        self.modifiers.insert(index, modifier);
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<Modifier, StackError> {
        self.check_index(index)?;
        Ok(self.modifiers.remove(index))
    }

    /// Remove at from, insert at to; both indices are in 0..len.
    pub fn move_modifier(&mut self, from: usize, to: usize) -> Result<(), StackError> {
        self.check_index(from)?;
        self.check_index(to)?;
        let modifier = self.modifiers.remove(from);
        self.modifiers.insert(to, modifier);
        Ok(())
    }

    /// Replace with the same kind only, returning the old modifier.
    pub fn replace(&mut self, index: usize, modifier: Modifier) -> Result<Modifier, StackError> {
        self.check_index(index)?;
        if std::mem::discriminant(&self.modifiers[index]) != std::mem::discriminant(&modifier) {
            return Err(StackError::KindMismatch { index });
        }
        modifier.validate()?;
        Ok(std::mem::replace(&mut self.modifiers[index], modifier))
    }

    pub fn evaluate(&self, base: &Base) -> Result<Mesh, ModifierError> {
        let mut current = base.mesh.clone();
        for (index, modifier) in self.modifiers.iter().enumerate() {
            current = match modifier {
                Modifier::Subdivide { levels } => {
                    try_catmull_clark(&PolyMesh::from_mesh(&current), *levels)
                        .and_then(|poly| poly.to_mesh())
                        .map_err(ModifierFailure::Subdivide)
                }
                Modifier::Remesh { target, iterations } => {
                    remesh(&current, *target, *iterations).map_err(ModifierFailure::Remesh)
                }
                Modifier::Boolean { op, tool } => match op {
                    BoolOp::Union => union(&current, tool),
                    BoolOp::Intersection => intersection(&current, tool),
                    BoolOp::Difference => difference(&current, tool),
                }
                .map_err(ModifierFailure::Boolean),
            }
            .map_err(|failure| ModifierError { index, failure })?;
        }
        Ok(current)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = (self.len() as u64).to_le_bytes().to_vec();
        for modifier in &self.modifiers {
            match modifier {
                Modifier::Subdivide { levels } => {
                    bytes.push(0);
                    bytes.extend_from_slice(&(*levels as u64).to_le_bytes());
                }
                Modifier::Remesh { target, iterations } => {
                    bytes.push(1);
                    bytes.extend_from_slice(&target.to_le_bytes());
                    bytes.extend_from_slice(&(*iterations as u64).to_le_bytes());
                }
                Modifier::Boolean { op, tool } => {
                    bytes.push(2);
                    bytes.push(match op {
                        BoolOp::Union => 0,
                        BoolOp::Intersection => 1,
                        BoolOp::Difference => 2,
                    });
                    let tool = tool.to_bytes();
                    bytes.extend_from_slice(&(tool.len() as u64).to_le_bytes());
                    bytes.extend_from_slice(&tool);
                }
            }
        }
        bytes
    }
}

fn command_error(error: StackError) -> CommandError {
    CommandError::new(format!("{error:?}"))
}

pub struct AddModifier {
    index: usize,
    modifier: Modifier,
}

impl AddModifier {
    pub fn new(index: usize, modifier: Modifier) -> Self {
        Self { index, modifier }
    }
}

impl Command<Stack> for AddModifier {
    fn apply(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        doc.insert(self.index, self.modifier.clone())
            .map_err(command_error)
    }
    fn undo(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        doc.remove(self.index).map(|_| ()).map_err(command_error)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct RemoveModifier {
    index: usize,
    removed: Option<Modifier>,
}

impl RemoveModifier {
    pub fn new(index: usize) -> Self {
        Self {
            index,
            removed: None,
        }
    }
}

impl Command<Stack> for RemoveModifier {
    fn apply(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        self.removed = Some(doc.remove(self.index).map_err(command_error)?);
        Ok(())
    }
    fn undo(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        let modifier = self
            .removed
            .as_ref()
            .ok_or_else(|| CommandError::new("remove not applied"))?;
        doc.insert(self.index, modifier.clone())
            .map_err(command_error)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct MoveModifier {
    from: usize,
    to: usize,
}

impl MoveModifier {
    pub fn new(from: usize, to: usize) -> Self {
        Self { from, to }
    }
}

impl Command<Stack> for MoveModifier {
    fn apply(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        doc.move_modifier(self.from, self.to).map_err(command_error)
    }
    fn undo(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        doc.move_modifier(self.to, self.from).map_err(command_error)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct SetModifier {
    index: usize,
    modifier: Modifier,
    old: Option<Modifier>,
}

impl SetModifier {
    pub fn new(index: usize, modifier: Modifier) -> Self {
        Self {
            index,
            modifier,
            old: None,
        }
    }
}

impl Command<Stack> for SetModifier {
    fn apply(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        self.old = Some(
            doc.replace(self.index, self.modifier.clone())
                .map_err(command_error)?,
        );
        Ok(())
    }
    fn undo(&mut self, doc: &mut Stack) -> Result<(), CommandError> {
        let old = self
            .old
            .as_ref()
            .ok_or_else(|| CommandError::new("set not applied"))?;
        doc.replace(self.index, old.clone())
            .map(|_| ())
            .map_err(command_error)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
