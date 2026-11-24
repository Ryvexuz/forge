//! Reflected type registry module.

use crate::desc::{Kind, Reflect, TypeDesc};
use crate::macros::Field;
use crate::value::Value;
use std::fmt;

type Constructor = fn(Value) -> Option<Box<dyn Reflect + Send>>;

struct Entry {
    desc: TypeDesc,
    build: Constructor,
}

/// Types in registration order, with erased constructors.
#[derive(Default)]
pub struct Registry {
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    Duplicate { name: String },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Duplicate { name } => write!(f, "duplicate type {name}"),
        }
    }
}

impl std::error::Error for RegistryError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CreateError {
    NoType { name: String },
    Build { name: String },
}

impl fmt::Display for CreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoType { name } => write!(f, "no type {name}"),
            Self::Build { name } => write!(f, "cannot build {name} from its zero value"),
        }
    }
}

impl std::error::Error for CreateError {}

impl Registry {
    pub fn new() -> Registry {
        Self::default()
    }

    pub fn register<T: Reflect + Field + Send + 'static>(&mut self) -> Result<(), RegistryError> {
        let desc = T::desc();
        if self.find(desc.name()).is_some() {
            return Err(RegistryError::Duplicate {
                name: desc.name().into(),
            });
        }
        self.entries.push(Entry {
            desc,
            build: |value| {
                T::from_value(value).map(|object| Box::new(object) as Box<dyn Reflect + Send>)
            },
        });
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn desc(&self, index: usize) -> Option<&TypeDesc> {
        self.entries.get(index).map(|entry| &entry.desc)
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        self.entries.iter().position(|entry| entry.desc.name() == name)
    }

    pub fn create(&self, name: &str) -> Result<Box<dyn Reflect + Send>, CreateError> {
        let entry = self
            .find(name)
            .and_then(|index| self.entries.get(index))
            .ok_or_else(|| CreateError::NoType { name: name.into() })?;
        (entry.build)(zero_value(&Kind::Struct(Box::new(entry.desc.clone()))))
            .ok_or_else(|| CreateError::Build { name: name.into() })
    }
}

/// Declared defaults take precedence over shape-derived zeroes; no range check.
/// An unvalidated, empty enum has an empty name (and cannot build a valid field).
pub fn zero_value(kind: &Kind) -> Value {
    match kind {
        Kind::Bool => Value::Bool(false),
        Kind::I64 => Value::I64(0),
        Kind::U64 => Value::U64(0),
        Kind::F64 => Value::F64(0.0),
        Kind::Text => Value::Text(String::new()),
        Kind::List(_) => Value::List(Vec::new()),
        Kind::Array(item, count) => Value::List((0..*count).map(|_| zero_value(item)).collect()),
        Kind::Struct(desc) => Value::Struct(
            desc.fields()
                .iter()
                .map(|field| match &field.default {
                    Some(value) => value.clone(),
                    None => zero_value(&field.kind),
                })
                .collect(),
        ),
        Kind::Option(_) => Value::Option(None),
        Kind::Enum { variants, .. } => Value::Enum(match variants.first() {
            Some(name) => name.clone(),
            None => String::new(),
        }),
    }
}
