//! Bounded, target-independent command history. The default cap is MAX_HISTORY;
//! a zero cap becomes one. Successful runs discard redo and evict oldest entries.
//! Failed runs record nothing; failed undo/redo leave the command on its stack.
//! Commands must leave their target unchanged on failure.

use std::collections::VecDeque;

pub const MAX_HISTORY: usize = 10_000;

pub trait Command<T, E> {
    fn apply(&mut self, target: &mut T) -> Result<(), E>;
    fn undo(&mut self, target: &mut T) -> Result<(), E>;
}

pub struct History<'c, T, E> {
    done: VecDeque<Box<dyn Command<T, E> + 'c>>,
    undone: Vec<Box<dyn Command<T, E> + 'c>>,
    cap: usize,
}

impl<T, E> Default for History<'_, T, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'c, T, E> History<'c, T, E> {
    pub fn new() -> Self {
        Self::with_cap(MAX_HISTORY)
    }

    pub fn with_cap(cap: usize) -> Self {
        Self {
            done: VecDeque::new(),
            undone: Vec::new(),
            cap: cap.max(1),
        }
    }

    pub fn run(
        &mut self,
        target: &mut T,
        mut command: Box<dyn Command<T, E> + 'c>,
    ) -> Result<(), E> {
        command.apply(target)?;
        self.undone.clear();
        if self.done.len() == self.cap {
            let _ = self.done.pop_front();
        }
        self.done.push_back(command);
        Ok(())
    }

    pub fn undo(&mut self, target: &mut T) -> Result<bool, E> {
        let Some(command) = self.done.back_mut() else {
            return Ok(false);
        };
        command.undo(target)?;
        if let Some(command) = self.done.pop_back() {
            self.undone.push(command);
        }
        Ok(true)
    }

    pub fn redo(&mut self, target: &mut T) -> Result<bool, E> {
        let Some(command) = self.undone.last_mut() else {
            return Ok(false);
        };
        command.apply(target)?;
        if let Some(command) = self.undone.pop() {
            self.done.push_back(command);
        }
        Ok(true)
    }

    pub fn undo_len(&self) -> usize {
        self.done.len()
    }

    pub fn redo_len(&self) -> usize {
        self.undone.len()
    }
}
