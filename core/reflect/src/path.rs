//! Root-relative paths and the validated, announced field-edit entry point.

use crate::change::{Change, ChangeError, ChangeSink};
use crate::desc::{FieldDesc, Kind, Reflect, SetError, TypeDesc};
use crate::limits::Limits;
use crate::value::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Segment {
    Field(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path {
    segments: Vec<Segment>,
}

impl Path {
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, segment) in self.segments.iter().enumerate() {
            match segment {
                Segment::Field(name) => write!(f, "{}{name}", if i == 0 { "" } else { "." })?,
                Segment::Index(index) => write!(f, "[{index}]")?,
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathError {
    TooLong {
        len: usize,
        max: usize,
    },
    Empty,
    EmptySegment {
        at: usize,
    },
    BadName {
        at: usize,
    },
    BadChar {
        at: usize,
    },
    UnclosedIndex {
        at: usize,
    },
    BadIndex {
        at: usize,
        text: String,
    },
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong { len, max } => write!(f, "path is {len} bytes, over the limit {max}"),
            Self::Empty => f.write_str("empty path"),
            Self::EmptySegment { at } => write!(f, "empty name at byte {at}"),
            Self::BadName { at } => write!(f, "bad name at byte {at}"),
            Self::BadChar { at } => write!(f, "unexpected character at byte {at}"),
            Self::UnclosedIndex { at } => write!(f, "unclosed index at byte {at}"),
            Self::BadIndex { at, text } => write!(f, "bad index \"{text}\" at byte {at}"),
        }
    }
}

impl std::error::Error for PathError {}

pub fn parse_path(text: &str, limits: &Limits) -> Result<Path, PathError> {
    if text.len() > limits.max_path_length {
        return Err(PathError::TooLong {
            len: text.len(),
            max: limits.max_path_length,
        });
    }
    if text.is_empty() {
        return Err(PathError::Empty);
    }
    let bytes = text.as_bytes();
    let mut at = 0;
    let mut segments = Vec::new();
    loop {
        match bytes.get(at) {
            None | Some(b'.' | b'[') => return Err(PathError::EmptySegment { at }),
            Some(b) if b.is_ascii_alphabetic() || *b == b'_' => {}
            _ => return Err(PathError::BadName { at }),
        }
        let start = at;
        at += 1;
        while bytes
            .get(at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            at += 1;
        }
        // Name boundaries are ASCII; index boundaries are ASCII brackets.
        let name = text.get(start..at).ok_or(PathError::BadName { at: start })?;
        segments.push(Segment::Field(name.into()));
        loop {
            match bytes.get(at) {
                None => return Ok(Path { segments }),
                Some(b'.') => {
                    at += 1;
                    break;
                }
                Some(b'[') => {
                    let open = at;
                    at += 1;
                    let start = at;
                    while bytes.get(at).is_some_and(|b| *b != b']') {
                        at += 1;
                    }
                    if bytes.get(at).is_none() {
                        return Err(PathError::UnclosedIndex { at: open });
                    }
                    let raw = text.get(start..at).ok_or(PathError::BadChar { at: start })?;
                    let bad = || PathError::BadIndex {
                        at: start,
                        text: raw.into(),
                    };
                    if raw.is_empty()
                        || !raw.bytes().all(|b| b.is_ascii_digit())
                        || (raw.len() > 1 && raw.starts_with('0'))
                    {
                        return Err(bad());
                    }
                    segments.push(Segment::Index(raw.parse().map_err(|_| bad())?));
                    at += 1;
                }
                _ => return Err(PathError::BadChar { at }),
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum AccessError {
    NoField {
        path: String,
        name: String,
    },
    IndexOutOfRange {
        path: String,
        index: usize,
        len: usize,
    },
    NotIndexable {
        path: String,
        kind: &'static str,
    },
    NotAStruct {
        path: String,
        kind: &'static str,
    },
    WrongKind {
        path: String,
        expected: &'static str,
        got: &'static str,
    },
    NonFinite {
        path: String,
    },
    OutOfRange {
        path: String,
    },
    ReadOnly {
        path: String,
    },
    TooLong {
        path: String,
        max: usize,
    },
    Set {
        path: String,
        error: SetError,
    },
    Change {
        path: String,
        error: ChangeError,
    },
}

impl fmt::Display for AccessError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoField { path, name } => write!(f, "{path}: no field {name}"),
            Self::IndexOutOfRange { path, index, len } => {
                write!(f, "{path}: index {index} out of range (length {len})")
            }
            Self::NotIndexable { path, kind } => write!(f, "{path}: {kind} cannot be indexed"),
            Self::NotAStruct { path, kind } => write!(f, "{path}: {kind} has no fields"),
            Self::WrongKind {
                path,
                expected,
                got,
            } => {
                write!(f, "{path}: expected {expected}, got {got}")
            }
            Self::NonFinite { path } => write!(f, "{path}: NaN or infinity"),
            Self::OutOfRange { path } => write!(f, "{path}: value outside the field's range"),
            Self::ReadOnly { path } => write!(f, "{path}: field is read-only"),
            Self::TooLong { path, max } => write!(f, "{path}: more than {max} items"),
            Self::Set { path, error } => write!(f, "{path}: {error}"),
            Self::Change { path, error } => write!(f, "{path}: {error}"),
        }
    }
}

impl std::error::Error for AccessError {}

fn no_field(path: &str, name: &str) -> AccessError {
    AccessError::NoField {
        path: path.into(),
        name: name.into(),
    }
}

fn root_field<'a>(desc: &'a TypeDesc, path: &Path) -> Result<(usize, &'a FieldDesc), AccessError> {
    let name = match path.segments.first() {
        Some(Segment::Field(name)) => name,
        _ => return Err(no_field(&path.to_string(), "")),
    };
    let missing = || no_field(&path.to_string(), name);
    let index = desc.field_index(name).ok_or_else(missing)?;
    Ok((index, desc.field(index).ok_or_else(missing)?))
}

fn extended(path: &str, segment: &Segment) -> String {
    match segment {
        Segment::Field(name) => format!("{path}.{name}"),
        Segment::Index(index) => format!("{path}[{index}]"),
    }
}

// Both accessors walk an owned snapshot, never mutable object state.
fn walk<'a>(
    mut value: &'a mut Value,
    field: &'a FieldDesc,
    path: &Path,
    writing: bool,
) -> Result<(&'a mut Value, &'a Kind, Option<&'a FieldDesc>), AccessError> {
    let text = path.to_string();
    let mut prefix = field.name.clone();
    let mut target = Some(field);
    let mut kind = &field.kind;
    if writing && field.readonly {
        return Err(AccessError::ReadOnly { path: prefix });
    }
    for segment in path.segments.iter().skip(1) {
        prefix = extended(&prefix, segment);
        let (index, next, next_field) = match (segment, kind) {
            (Segment::Field(name), Kind::Struct(desc)) => {
                let missing = || no_field(&text, name);
                let i = desc.field_index(name).ok_or_else(missing)?;
                let f = desc.field(i).ok_or_else(missing)?;
                if writing && f.readonly {
                    return Err(AccessError::ReadOnly { path: prefix });
                }
                (i, &f.kind, Some(f))
            }
            (Segment::Index(i), Kind::List(k) | Kind::Array(k, _)) => (*i, k.as_ref(), None),
            (Segment::Field(_), _) => {
                return Err(AccessError::NotAStruct {
                    path: text,
                    kind: kind.name(),
                });
            }
            (Segment::Index(_), _) => {
                return Err(AccessError::NotIndexable {
                    path: text,
                    kind: kind.name(),
                });
            }
        };
        let got = value.kind_name();
        value = match (segment, value) {
            (Segment::Field(name), Value::Struct(items)) => {
                items.get_mut(index).ok_or_else(|| no_field(&text, name))?
            }
            (Segment::Index(_), Value::List(items)) => {
                let len = items.len();
                items
                    .get_mut(index)
                    .ok_or_else(|| AccessError::IndexOutOfRange {
                        path: text.clone(),
                        index,
                        len,
                    })?
            }
            _ => {
                return Err(AccessError::WrongKind {
                    path: text,
                    expected: kind.name(),
                    got,
                });
            }
        };
        kind = next;
        target = next_field;
    }
    Ok((value, kind, target))
}

pub fn get_path(root: &dyn Reflect, path: &Path) -> Result<Value, AccessError> {
    let desc = root.dyn_desc();
    let (index, field) = root_field(&desc, path)?;
    let mut top = root
        .get(index)
        .ok_or_else(|| no_field(&path.to_string(), &field.name))?;
    let (value, _, _) = walk(&mut top, field, path, false)?;
    Ok(value.clone())
}

// Separate passes preserve non-finite-before-length error precedence.
fn values_valid(value: &Value, max: Option<usize>) -> bool {
    let mut pending = vec![value];
    while let Some(value) = pending.pop() {
        match value {
            Value::F64(n) if max.is_none() && !n.is_finite() => return false,
            Value::List(items) if max.is_some_and(|max| items.len() > max) => return false,
            Value::List(items) | Value::Struct(items) => pending.extend(items),
            Value::Option(Some(item)) => pending.push(item),
            _ => {}
        }
    }
    true
}

fn ranges(
    kind: &Kind,
    value: &Value,
    field: Option<&FieldDesc>,
    path: &str,
) -> Result<(), AccessError> {
    if let Some(field) = field {
        let inside = match value {
            Value::F64(n) => {
                field.min.is_none_or(|min| *n >= min) && field.max.is_none_or(|max| *n <= max)
            }
            Value::I64(n) => integer_range(i128::from(*n), field),
            Value::U64(n) => integer_range(i128::from(*n), field),
            _ => true,
        };
        if !inside {
            return Err(AccessError::OutOfRange { path: path.into() });
        }
    }
    match (kind, value) {
        (Kind::Struct(desc), Value::Struct(items)) => {
            for (field, item) in desc.fields().iter().zip(items) {
                ranges(
                    &field.kind,
                    item,
                    Some(field),
                    &format!("{path}.{}", field.name),
                )?;
            }
        }
        (Kind::List(k) | Kind::Array(k, _), Value::List(items)) => {
            for (i, item) in items.iter().enumerate() {
                ranges(k, item, None, &format!("{path}[{i}]"))?;
            }
        }
        (Kind::Option(k), Value::Option(Some(item))) => ranges(k, item, None, path)?,
        _ => {}
    }
    Ok(())
}

fn integer_range(n: i128, field: &FieldDesc) -> bool {
    field.min.is_none_or(|min| n >= min.ceil() as i128)
        && field.max.is_none_or(|max| n <= max.floor() as i128)
}

fn holds_readonly(kind: &Kind) -> bool {
    match kind {
        Kind::Struct(desc) => desc
            .fields()
            .iter()
            .any(|f| f.readonly || holds_readonly(&f.kind)),
        Kind::List(k) | Kind::Array(k, _) | Kind::Option(k) => holds_readonly(k),
        _ => false,
    }
}

pub fn set_path(
    root: &mut dyn Reflect,
    path: &Path,
    value: Value,
    limits: &Limits,
    sink: &mut dyn ChangeSink,
) -> Result<(), AccessError> {
    let text = path.to_string();
    let desc = root.dyn_desc();
    let (index, field) = root_field(&desc, path)?;
    let mut top = root.get(index).ok_or_else(|| no_field(&text, &field.name))?;
    let (target, kind, target_field) = walk(&mut top, field, path, true)?;
    if !kind.accepts(&value) {
        return Err(AccessError::WrongKind {
            path: text,
            expected: kind.name(),
            got: value.kind_name(),
        });
    }
    if !values_valid(&value, None) {
        return Err(AccessError::NonFinite { path: text });
    }
    if !values_valid(&value, Some(limits.max_list_length)) {
        return Err(AccessError::TooLong {
            path: text,
            max: limits.max_list_length,
        });
    }
    ranges(kind, &value, target_field, &text)?;
    if *target != value && holds_readonly(kind) {
        return Err(AccessError::ReadOnly { path: text });
    }
    let old = target.clone();
    *target = value.clone();
    let change = Change::new(desc.name(), &text, old, value).map_err(|error| AccessError::Change {
        path: text.clone(),
        error,
    })?;
    root.set(index, top).map_err(|error| AccessError::Set { path: text, error })?;
    sink.changed(&change);
    Ok(())
}
