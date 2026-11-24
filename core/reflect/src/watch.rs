//! Recording and registration-ordered delivery of applied changes.

use crate::change::{Change, ChangeSink};

/// Records every applied change, in the order received.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Journal {
    entries: Vec<Change>,
}

impl Journal {
    /// Creates an empty journal.
    pub fn new() -> Journal {
        Self::default()
    }

    /// Returns recorded changes, oldest first.
    pub fn entries(&self) -> &[Change] {
        &self.entries
    }

    /// Returns the number of recorded changes.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Reports whether no changes are recorded.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Removes every recorded change.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

impl ChangeSink for Journal {
    fn changed(&mut self, change: &Change) {
        self.entries.push(change.clone());
    }
}

/// Hands each change to every registered watcher, in registration order.
pub struct FanOut<'a> {
    watchers: Vec<&'a mut dyn ChangeSink>,
}

impl<'a> FanOut<'a> {
    /// Creates a fan-out with no watchers.
    pub fn new() -> FanOut<'a> {
        Self {
            watchers: Vec::new(),
        }
    }

    /// Registers a watcher and returns its zero-based position.
    pub fn add(&mut self, watcher: &'a mut dyn ChangeSink) -> usize {
        let position = self.len();
        self.watchers.push(watcher);
        position
    }

    /// Returns the number of registered watchers.
    pub fn len(&self) -> usize {
        self.watchers.len()
    }

    /// Reports whether no watchers are registered.
    pub fn is_empty(&self) -> bool {
        self.watchers.is_empty()
    }
}

impl Default for FanOut<'_> {
    fn default() -> Self {
        Self::new()
    }
}

impl ChangeSink for FanOut<'_> {
    fn changed(&mut self, change: &Change) {
        for watcher in &mut self.watchers {
            watcher.changed(change);
        }
    }
}
