//! Copy of node 0.6's shape (foundation's undo module), without groups or poisoning.
//! Each entry is one command; there is no grouped rollback.

use std::any::Any;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandError {
    pub message: String,
}

impl CommandError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UndoError {
    InvalidCap,
    ApplyFailed(CommandError),
    UndoFailed(CommandError),
    RedoFailed(CommandError),
}

impl fmt::Display for UndoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCap => write!(f, "history capacity must be at least one"),
            Self::ApplyFailed(e) => write!(f, "apply failed: {e}"),
            Self::UndoFailed(e) => write!(f, "undo failed: {e}"),
            Self::RedoFailed(e) => write!(f, "redo failed: {e}"),
        }
    }
}

impl std::error::Error for UndoError {}

/// A failing apply or undo must leave the document unchanged. After successful
/// apply, undo restores the document, and apply can run again with the same effect.
pub trait Command<D> {
    /// Make the change, both initially and on redo.
    fn apply(&mut self, doc: &mut D) -> Result<(), CommandError>;
    /// Reverse a successful apply.
    fn undo(&mut self, doc: &mut D) -> Result<(), CommandError>;
    /// Absorb an already applied next command. Undo must reverse both, and apply
    /// must redo both, when returning true.
    fn merge(&mut self, next: &dyn Command<D>) -> bool {
        let _ = next;
        false
    }
    fn as_any(&self) -> &dyn Any;
}

pub struct History<D> {
    undo: Vec<Box<dyn Command<D>>>,
    redo: Vec<Box<dyn Command<D>>>,
    max_entries: usize,
    dropped: u64,
    barrier: bool,
}

impl<D> History<D> {
    pub fn new(max_entries: usize) -> Result<Self, UndoError> {
        if max_entries == 0 {
            return Err(UndoError::InvalidCap);
        }
        Ok(Self {
            undo: Vec::new(),
            redo: Vec::new(),
            max_entries,
            dropped: 0,
            barrier: true,
        })
    }

    pub fn execute(&mut self, doc: &mut D, mut cmd: Box<dyn Command<D>>) -> Result<(), UndoError> {
        cmd.apply(doc).map_err(UndoError::ApplyFailed)?;
        self.redo.clear();
        let merged = !self.barrier && self.undo.last_mut().is_some_and(|last| last.merge(&*cmd));
        if !merged {
            if self.undo.len() == self.max_entries {
                self.undo.remove(0);
                self.dropped += 1;
            }
            self.undo.push(cmd);
        }
        self.barrier = false;
        Ok(())
    }

    /// Ok(false) when there is nothing to undo.
    pub fn undo(&mut self, doc: &mut D) -> Result<bool, UndoError> {
        let Some(cmd) = self.undo.last_mut() else {
            self.barrier = true;
            return Ok(false);
        };
        cmd.undo(doc).map_err(UndoError::UndoFailed)?;
        self.redo.push(self.undo.pop().unwrap());
        self.barrier = true;
        Ok(true)
    }

    /// Ok(false) when there is nothing to redo.
    pub fn redo(&mut self, doc: &mut D) -> Result<bool, UndoError> {
        let Some(cmd) = self.redo.last_mut() else {
            self.barrier = true;
            return Ok(false);
        };
        cmd.apply(doc).map_err(UndoError::RedoFailed)?;
        self.undo.push(self.redo.pop().unwrap());
        self.barrier = true;
        Ok(true)
    }

    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn break_merge(&mut self) {
        self.barrier = true;
    }
}
