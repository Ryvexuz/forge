//! Validated applied-change records and their object-safe receiver.

use crate::value::Value;

/// One applied field change, built only through `Change::new`.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    type_name: String,
    path: String,
    old: Value,
    new: Value,
}

impl Change {
    /// Refuses empty names, empty paths and changes of value kind, in that order.
    pub fn new(type_name: &str, path: &str, old: Value, new: Value) -> Result<Change, ChangeError> {
        if type_name.is_empty() {
            return Err(ChangeError::EmptyTypeName);
        }
        if path.is_empty() {
            return Err(ChangeError::EmptyPath);
        }
        if old.kind_name() != new.kind_name() {
            return Err(ChangeError::KindChanged {
                old: old.kind_name(),
                new: new.kind_name(),
            });
        }
        Ok(Self {
            type_name: type_name.into(),
            path: path.into(),
            old,
            new,
        })
    }

    /// Returns the owning type's name.
    pub fn type_name(&self) -> &str {
        &self.type_name
    }

    /// Returns the changed field's path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the value before the change.
    pub fn old(&self) -> &Value {
        &self.old
    }

    /// Returns the value after the change.
    pub fn new_value(&self) -> &Value {
        &self.new
    }
}

/// Reasons an applied-change record cannot be constructed.
#[derive(Clone, Debug, PartialEq)]
pub enum ChangeError {
    /// The owning type has no name.
    EmptyTypeName,
    /// The changed field has no path.
    EmptyPath,
    /// A set cannot change the value's kind.
    KindChanged {
        /// The original value's kind.
        old: &'static str,
        /// The replacement value's kind.
        new: &'static str,
    },
}

impl std::fmt::Display for ChangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyTypeName => f.write_str("change has an empty type name"),
            Self::EmptyPath => f.write_str("change has an empty path"),
            Self::KindChanged { old, new } => write!(f, "change kind differs: {old} to {new}"),
        }
    }
}

impl std::error::Error for ChangeError {}

/// Receives applied changes under the R5 caller and R7 implementation contract.
pub trait ChangeSink {
    /// Receives one applied change.
    fn changed(&mut self, change: &Change);
}
