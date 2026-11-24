//! Type and field description module.

use crate::value::Value;

/// Field shapes, including full descriptors for nested structs.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Bool,
    I64,
    U64,
    F64,
    Text,
    List(Box<Kind>),
    Array(Box<Kind>, usize),
    Struct(Box<TypeDesc>),
    Option(Box<Kind>),
    Enum {
        name: String,
        variants: Vec<String>,
    },
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bool => "bool",
            Self::I64 => "i64",
            Self::U64 => "u64",
            Self::F64 => "f64",
            Self::Text => "text",
            Self::List(_) => "list",
            Self::Array(_, _) => "array",
            Self::Struct(_) => "struct",
            Self::Option(_) => "option",
            Self::Enum { .. } => "enum",
        }
    }

    /// Checks shape recursively, without numeric conversion or range checks.
    pub fn accepts(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Bool, Value::Bool(_))
            | (Self::I64, Value::I64(_))
            | (Self::U64, Value::U64(_))
            | (Self::F64, Value::F64(_))
            | (Self::Text, Value::Text(_)) => true,
            (Self::List(kind), Value::List(values)) => values.iter().all(|v| kind.accepts(v)),
            (Self::Array(kind, n), Value::List(values)) => {
                values.len() == *n && values.iter().all(|v| kind.accepts(v))
            }
            (Self::Struct(desc), Value::Struct(values)) => {
                values.len() == desc.fields().len()
                    && desc
                        .fields()
                        .iter()
                        .zip(values)
                        .all(|(f, v)| f.kind.accepts(v))
            }
            (Self::Option(_), Value::Option(None)) => true,
            (Self::Option(kind), Value::Option(Some(value))) => kind.accepts(value),
            (Self::Enum { variants, .. }, Value::Enum(value)) => variants.contains(value),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FieldDesc {
    pub name: String,
    pub kind: Kind,
    pub label: Option<String>,
    pub unit: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub readonly: bool,
    pub hidden: bool,
    /// Checked for shape only; checking the default's range belongs to set_path.
    pub default: Option<Value>,
    pub renamed_from: Option<String>,
    /// Type version in which this field first appeared.
    pub since: u32,
}

impl FieldDesc {
    pub fn new(name: &str, kind: Kind) -> FieldDesc {
        Self {
            name: name.to_string(),
            kind,
            label: None,
            unit: None,
            min: None,
            max: None,
            readonly: false,
            hidden: false,
            default: None,
            renamed_from: None,
            since: 1,
        }
    }
}

/// A descriptor validated at construction, with no mutable access to its fields.
#[derive(Clone, Debug, PartialEq)]
pub struct TypeDesc {
    name: String,
    version: u32,
    fields: Vec<FieldDesc>,
}

impl TypeDesc {
    pub fn new(name: &str, version: u32, fields: Vec<FieldDesc>) -> Result<TypeDesc, DescError> {
        fn identifier(name: &str) -> Result<(), DescError> {
            let mut bytes = name.bytes();
            let first = bytes
                .next()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_');
            if first && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                Ok(())
            } else {
                Err(DescError::BadName { name: name.into() })
            }
        }
        fn kind_valid(kind: &Kind, field: &str) -> Result<(), DescError> {
            match kind {
                Kind::List(k) | Kind::Option(k) => kind_valid(k, field)?,
                Kind::Array(k, n) => {
                    if *n == 0 {
                        return Err(DescError::Empty {
                            field: field.into(),
                        });
                    }
                    kind_valid(k, field)?;
                }
                Kind::Enum { name, variants } => {
                    identifier(name)?;
                    if variants.is_empty() {
                        return Err(DescError::Empty {
                            field: field.into(),
                        });
                    }
                    for (i, variant) in variants.iter().enumerate() {
                        identifier(variant)?;
                        if variants[..i].contains(variant) {
                            return Err(DescError::DuplicateVariant {
                                enum_name: name.clone(),
                                variant: variant.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
            Ok(())
        }
        identifier(name)?;
        if version == 0 {
            return Err(DescError::BadVersion {
                field: None,
                version,
            });
        }
        for field in &fields {
            identifier(&field.name)?;
            kind_valid(&field.kind, &field.name)?;
            if field.since == 0 || field.since > version {
                return Err(DescError::BadVersion {
                    field: Some(field.name.clone()),
                    version: field.since,
                });
            }
            if (field.min.is_some() || field.max.is_some())
                && (!matches!(field.kind, Kind::I64 | Kind::U64 | Kind::F64)
                    || field.min.is_some_and(|v| !v.is_finite())
                    || field.max.is_some_and(|v| !v.is_finite())
                    || matches!((field.min, field.max), (Some(a), Some(b)) if a > b))
            {
                return Err(DescError::BadRange {
                    field: field.name.clone(),
                });
            }
            if field.default.as_ref().is_some_and(|v| !field.kind.accepts(v)) {
                return Err(DescError::BadDefault {
                    field: field.name.clone(),
                });
            }
            if let Some(old) = &field.renamed_from {
                identifier(old)?;
                if fields.iter().any(|f| &f.name == old) {
                    return Err(DescError::BadRename {
                        field: field.name.clone(),
                    });
                }
            }
            for text in [&field.label, &field.unit].into_iter().flatten() {
                if text.is_empty() || text.chars().any(char::is_control) {
                    return Err(DescError::BadText {
                        field: field.name.clone(),
                    });
                }
            }
        }
        for (i, field) in fields.iter().enumerate() {
            if fields[..i].iter().any(|f| f.name == field.name) {
                return Err(DescError::DuplicateField {
                    name: field.name.clone(),
                });
            }
        }
        Ok(Self {
            name: name.into(),
            version,
            fields,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn fields(&self) -> &[FieldDesc] {
        &self.fields
    }

    pub fn field(&self, index: usize) -> Option<&FieldDesc> {
        self.fields.get(index)
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name == name)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DescError {
    BadName {
        name: String,
    },
    DuplicateField {
        name: String,
    },
    DuplicateVariant {
        enum_name: String,
        variant: String,
    },
    Empty {
        field: String,
    },
    BadVersion {
        field: Option<String>,
        version: u32,
    },
    BadRange {
        field: String,
    },
    BadDefault {
        field: String,
    },
    BadRename {
        field: String,
    },
    BadText {
        field: String,
    },
}

impl std::fmt::Display for DescError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid descriptor: {self:?}")
    }
}

impl std::error::Error for DescError {}

#[derive(Clone, Debug, PartialEq)]
pub enum SetError {
    NoField {
        index: usize,
        count: usize,
    },
    WrongKind {
        field: String,
        expected: &'static str,
        got: &'static str,
    },
}

impl std::fmt::Display for SetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot set field: {self:?}")
    }
}

impl std::error::Error for SetError {}

/// Field indexes are descriptor positions. Refused sets must leave all fields unchanged.
/// Implementations check shape, not range, readonly attributes or change events.
pub trait Reflect {
    fn desc() -> TypeDesc
    where
        Self: Sized;
    fn dyn_desc(&self) -> TypeDesc;
    fn get(&self, index: usize) -> Option<Value>;
    fn set(&mut self, index: usize, value: Value) -> Result<(), SetError>;
}
