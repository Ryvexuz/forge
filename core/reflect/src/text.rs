//! Canonical, versioned values. Only the top-level descriptor is versioned.

use crate::desc::{Kind, TypeDesc};
use crate::limits::Limits;
use crate::schema::{hash_hex, schema_hash, write_text, SchemaError};
use crate::value::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub value: Value,
    pub from_version: u32,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    Renamed {
        line: usize,
        from: String,
        to: String,
    },
    Ignored {
        line: usize,
        name: String,
    },
    Defaulted {
        field: String,
    },
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Renamed { line, from, to } => {
                write!(f, "field {to} read from old name {from} (line {line})")
            }
            Self::Ignored { line, name } => {
                write!(f, "unknown field {name} ignored (line {line})")
            }
            Self::Defaulted { field } => write!(f, "field {field} set to its default"),
        }
    }
}

// Keep the public variants and their exact diagnostic text together.
macro_rules! errors {
    ($name:ident { $($variant:ident $( { $($field:ident : $ty:ty),* } )? => $message:literal),* $(,)? }) => {
        #[derive(Clone, Debug, PartialEq)]
        pub enum $name {
            $($variant $( { $($field: $ty),* } )?,)*
            Schema(SchemaError),
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self {
                    $(Self::$variant $( { $($field),* } )? => write!(f, $message $(, $($field = $field),*)?),)*
                    Self::Schema(error) => fmt::Display::fmt(error, f),
                }
            }
        }
        impl std::error::Error for $name {}
    };
}

errors!(SaveError {
    WrongKind { path: String } => "{path}: value does not match its kind",
    NonFinite { path: String } => "{path}: NaN or infinity",
    TooLong { path: String, max: usize } => "{path}: more than {max} items",
    TooDeep { path: String, max: usize } => "{path}: nested deeper than {max}",
    TooLarge { bytes: u64, max: u64 } => "text is {bytes} bytes, over the limit {max}",
});

errors!(LoadError {
    TooLarge { bytes: u64, max: u64 } => "text is {bytes} bytes, over the limit {max}",
    CarriageReturn { line: usize } => "line {line}: carriage return (files use LF line ends)",
    NoFinalNewline => "text does not end with a line break",
    BadHeader { line: usize } => "line {line}: bad header",
    WrongType { line: usize, expected: String, found: String } => "line {line}: file holds type {found}, expected {expected}",
    NewerVersion { line: usize, file: u32, current: u32 } => "line {line}: file version {file} is newer than type version {current}",
    HashMismatch { line: usize, file: String, current: String } => "line {line}: schema hash {file} does not match {current}",
    BadLine { line: usize } => "line {line}: expected <field> = <value>",
    UnknownField { line: usize, name: String } => "line {line}: unknown field {name}",
    DuplicateField { line: usize, name: String } => "line {line}: field {name} given twice",
    OutOfOrder { line: usize, name: String } => "line {line}: field {name} out of order",
    MissingField { name: String } => "missing field {name}",
    MissingEnd => "missing end line",
    TrailingLine { line: usize } => "line {line}: text after end",
    BadValue { line: usize, path: String } => "line {line}: {path}: bad value",
    BadNumber { line: usize, path: String, text: String } => "line {line}: {path}: bad number {text}",
    IntegerRange { line: usize, path: String, text: String } => "line {line}: {path}: {text} is out of range",
    NonFinite { line: usize, path: String } => "line {line}: {path}: NaN or infinity",
    BadEscape { line: usize, path: String } => "line {line}: {path}: bad escape",
    RawControl { line: usize, path: String } => "line {line}: {path}: raw control character in text",
    BadVariant { line: usize, path: String, variant: String } => "line {line}: {path}: unknown variant {variant}",
    WrongLength { line: usize, path: String, len: usize, expected: usize } => "line {line}: {path}: array has {len} items, expected {expected}",
    StructFields { line: usize, path: String } => "line {line}: {path}: struct fields do not match its type",
    TooLong { line: usize, path: String, max: usize } => "line {line}: {path}: more than {max} items",
    TooDeep { line: usize, path: String, max: usize } => "line {line}: {path}: nested deeper than {max}",
});

// Value diagnostics always carry the same line/path context.
macro_rules! at {
    ($variant:ident, $line:expr, $path:expr $(, $key:ident = $value:expr)* $(,)?) => {
        LoadError::$variant { line: $line, path: $path.into(), $($key: $value),* }
    };
}

pub fn save_text(desc: &TypeDesc, value: &Value, limits: &Limits) -> Result<String, SaveError> {
    if !Kind::Struct(Box::new(desc.clone())).accepts(value) {
        return Err(SaveError::WrongKind {
            path: desc.name().into(),
        });
    }
    let hash = hash_hex(schema_hash(desc).map_err(SaveError::Schema)?);
    let (name, version) = (desc.name(), desc.version());
    let mut text = format!("reflect value 1\ntype {name} version={version} hash={hash}\n");
    if let Value::Struct(values) = value {
        for (field, value) in desc.fields().iter().zip(values) {
            text.push_str(&format!("{} = ", field.name));
            let mut emit = |s: &str| text.push_str(s);
            write_value(&field.kind, value, &field.name, 1, limits, &mut emit)?;
            text.push('\n');
        }
    }
    text.push_str("end\n");
    let bytes = text.len() as u64;
    if bytes > limits.max_text_file_bytes {
        return Err(SaveError::TooLarge {
            bytes,
            max: limits.max_text_file_bytes,
        });
    }
    Ok(text)
}

fn write_value(
    kind: &Kind,
    value: &Value,
    path: &str,
    depth: usize,
    limits: &Limits,
    emit: &mut dyn FnMut(&str),
) -> Result<(), SaveError> {
    if depth > limits.max_nesting_depth {
        return Err(SaveError::TooDeep {
            path: path.into(),
            max: limits.max_nesting_depth,
        });
    }
    match (kind, value) {
        (_, Value::Bool(v)) => emit(if *v { "true" } else { "false" }),
        (_, Value::I64(v)) => emit(&v.to_string()),
        (_, Value::U64(v)) => emit(&v.to_string()),
        (_, Value::F64(v)) => {
            if !v.is_finite() {
                return Err(SaveError::NonFinite { path: path.into() });
            }
            emit(&format!("{v:?}"));
        }
        (_, Value::Text(v)) => {
            let mut text = String::new();
            write_text(&mut text, v);
            emit(&text);
        }
        (_, Value::Enum(v)) => emit(v),
        (Kind::List(k) | Kind::Array(k, _), Value::List(values)) => {
            if values.len() > limits.max_list_length {
                return Err(SaveError::TooLong {
                    path: path.into(),
                    max: limits.max_list_length,
                });
            }
            emit("[");
            for (i, v) in values.iter().enumerate() {
                if i != 0 {
                    emit(",");
                }
                write_value(k, v, &format!("{path}[{i}]"), depth + 1, limits, emit)?;
            }
            emit("]");
        }
        (Kind::Struct(d), Value::Struct(values)) => {
            emit("{");
            for (i, (f, v)) in d.fields().iter().zip(values).enumerate() {
                if i != 0 {
                    emit(",");
                }
                emit(&f.name);
                emit("=");
                let path = format!("{path}.{}", f.name);
                write_value(&f.kind, v, &path, depth + 1, limits, emit)?;
            }
            emit("}");
        }
        (_, Value::Option(None)) => emit("none"),
        (Kind::Option(k), Value::Option(Some(v))) => {
            emit("some(");
            write_value(k, v, path, depth + 1, limits, emit)?;
            emit(")");
        }
        _ => return Err(SaveError::WrongKind { path: path.into() }),
    }
    Ok(())
}

fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

pub fn load_text(desc: &TypeDesc, text: &str, limits: &Limits) -> Result<Loaded, LoadError> {
    let bytes = text.len() as u64;
    if bytes > limits.max_text_file_bytes {
        return Err(LoadError::TooLarge {
            bytes,
            max: limits.max_text_file_bytes,
        });
    }
    if let Some((prefix, _)) = text.split_once('\r') {
        return Err(LoadError::CarriageReturn {
            line: prefix.bytes().filter(|b| *b == b'\n').count() + 1,
        });
    }
    let body = text.strip_suffix('\n').ok_or(LoadError::NoFinalNewline)?;
    let mut lines = body.split('\n');
    if lines.next() != Some("reflect value 1") {
        return Err(LoadError::BadHeader { line: 1 });
    }
    let header = || LoadError::BadHeader { line: 2 };
    let mut parts = lines.next().ok_or_else(header)?.split(' ');
    if parts.next() != Some("type") {
        return Err(header());
    }
    let name = parts.next().ok_or_else(header)?;
    if name != desc.name() {
        return Err(LoadError::WrongType {
            line: 2,
            expected: desc.name().into(),
            found: name.into(),
        });
    }
    let version = parts
        .next()
        .and_then(|s| s.strip_prefix("version="))
        .ok_or_else(header)?;
    let version = version
        .parse::<u32>()
        .ok()
        .filter(|v| *v > 0 && v.to_string() == version)
        .ok_or_else(header)?;
    if version > desc.version() {
        return Err(LoadError::NewerVersion {
            line: 2,
            file: version,
            current: desc.version(),
        });
    }
    let hash = parts
        .next()
        .and_then(|s| s.strip_prefix("hash="))
        .ok_or_else(header)?;
    if hash.len() != 16
        || !hash
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || parts.next().is_some()
    {
        return Err(header());
    }
    let current = hash_hex(schema_hash(desc).map_err(LoadError::Schema)?);
    let old = version < desc.version();
    if !old && hash != current {
        return Err(LoadError::HashMismatch {
            line: 2,
            file: hash.into(),
            current,
        });
    }
    if !lines.clone().any(|entry| entry == "end") {
        return Err(LoadError::MissingEnd);
    }
    let mut values: Vec<Option<Value>> = desc.fields().iter().map(|_| None).collect();
    let mut notes = Vec::new();
    let mut previous = None;
    let mut ended = false;
    for (offset, entry) in lines.enumerate() {
        let line = offset + 3;
        if ended {
            return Err(LoadError::TrailingLine { line });
        }
        if entry == "end" {
            ended = true;
            continue;
        }
        let (name, token) = entry
            .split_once(" = ")
            .filter(|(n, v)| identifier(n) && !v.is_empty())
            .ok_or(LoadError::BadLine { line })?;
        let index = desc.field_index(name).or_else(|| {
            if old {
                desc.fields()
                    .iter()
                    .position(|f| f.renamed_from.as_deref() == Some(name))
            } else {
                None
            }
        });
        let Some(index) = index else {
            if !old {
                return Err(LoadError::UnknownField {
                    line,
                    name: name.into(),
                });
            }
            notes.push(Note::Ignored {
                line,
                name: name.into(),
            });
            continue;
        };
        let field = desc.field(index).ok_or(LoadError::BadLine { line })?;
        let slot = values.get_mut(index).ok_or(LoadError::BadLine { line })?;
        if slot.is_some() {
            return Err(LoadError::DuplicateField {
                line,
                name: field.name.clone(),
            });
        }
        if !old && previous.is_some_and(|i| index < i) {
            return Err(LoadError::OutOfOrder {
                line,
                name: field.name.clone(),
            });
        }
        previous = Some(index);
        let mut parser = Parser {
            rest: token,
            line,
            limits,
        };
        *slot = Some(parser.value(&field.kind, &field.name, 1)?);
        if !parser.rest.is_empty() {
            return Err(parser.bad(&field.name));
        }
        if name != field.name {
            notes.push(Note::Renamed {
                line,
                from: name.into(),
                to: field.name.clone(),
            });
        }
    }
    let mut result = Vec::new();
    for (field, value) in desc.fields().iter().zip(values) {
        let default = field.default.as_ref();
        if let Some(value) = value {
            result.push(value);
        } else if let Some(default) = default.filter(|_| old && field.since > version) {
            // Defaults bypass parsing, but not finiteness, list or depth limits.
            write_value(&field.kind, default, &field.name, 1, limits, &mut |_| {})
                .map_err(|e| match e {
                    SaveError::NonFinite { path } => at!(NonFinite, 2, path),
                    SaveError::TooLong { path, max } => at!(TooLong, 2, path, max = max),
                    SaveError::TooDeep { path, max } => at!(TooDeep, 2, path, max = max),
                    _ => at!(BadValue, 2, &field.name),
                })?;
            result.push(default.clone());
            notes.push(Note::Defaulted {
                field: field.name.clone(),
            });
        } else {
            return Err(LoadError::MissingField {
                name: field.name.clone(),
            });
        }
    }
    Ok(Loaded {
        value: Value::Struct(result),
        from_version: version,
        notes,
    })
}

struct Parser<'a> {
    rest: &'a str,
    line: usize,
    limits: &'a Limits,
}

impl Parser<'_> {
    fn take(&mut self, prefix: &str) -> bool {
        if let Some(rest) = self.rest.strip_prefix(prefix) {
            self.rest = rest;
            true
        } else {
            false
        }
    }

    fn bad(&self, path: &str) -> LoadError {
        at!(BadValue, self.line, path)
    }

    fn value(&mut self, kind: &Kind, path: &str, depth: usize) -> Result<Value, LoadError> {
        let line = self.line;
        if depth > self.limits.max_nesting_depth {
            return Err(at!(TooDeep, line, path, max = self.limits.max_nesting_depth));
        }
        match kind {
            Kind::Text => self.string(path).map(Value::Text),
            Kind::List(k) | Kind::Array(k, _) => {
                if !self.take("[") {
                    return Err(self.bad(path));
                }
                let mut values = Vec::new();
                if !self.take("]") {
                    loop {
                        if values.len() >= self.limits.max_list_length {
                            return Err(at!(TooLong, line, path, max = self.limits.max_list_length));
                        }
                        let child_path = format!("{path}[{}]", values.len());
                        values.push(self.value(k, &child_path, depth + 1)?);
                        if self.take("]") {
                            break;
                        }
                        if !self.take(",") {
                            return Err(self.bad(path));
                        }
                    }
                }
                if let Kind::Array(_, expected) = kind {
                    if values.len() != *expected {
                        let len = values.len();
                        return Err(at!(WrongLength, line, path, len = len, expected = *expected));
                    }
                }
                Ok(Value::List(values))
            }
            Kind::Struct(d) => {
                let error = || at!(StructFields, line, path);
                if !self.take("{") {
                    return Err(error());
                }
                let mut values = Vec::new();
                for (i, field) in d.fields().iter().enumerate() {
                    if (i != 0 && !self.take(",")) || !self.take(&format!("{}=", field.name)) {
                        return Err(error());
                    }
                    let path = format!("{path}.{}", field.name);
                    values.push(self.value(&field.kind, &path, depth + 1)?);
                }
                if !self.take("}") {
                    return Err(error());
                }
                Ok(Value::Struct(values))
            }
            Kind::Option(k) => {
                if self.take("none") {
                    return Ok(Value::Option(None));
                }
                if !self.take("some(") {
                    return Err(self.bad(path));
                }
                let value = self.value(k, path, depth + 1)?;
                if !self.take(")") {
                    return Err(self.bad(path));
                }
                Ok(Value::Option(Some(Box::new(value))))
            }
            _ => {
                let end = self
                    .rest
                    .find([',', ']', '}', ')'])
                    .unwrap_or(self.rest.len());
                let token = self.rest.get(..end).ok_or_else(|| self.bad(path))?;
                self.rest = self.rest.get(end..).ok_or_else(|| self.bad(path))?;
                let number = || at!(BadNumber, line, path, text = token.into());
                let range = || at!(IntegerRange, line, path, text = token.into());
                match kind {
                    Kind::Bool => match token {
                        "true" => Ok(Value::Bool(true)),
                        "false" => Ok(Value::Bool(false)),
                        _ => Err(self.bad(path)),
                    },
                    Kind::I64 | Kind::U64 => {
                        let digits = if matches!(kind, Kind::I64) {
                            token.strip_prefix('-').map_or(token, |s| s)
                        } else {
                            token
                        };
                        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                            return Err(number());
                        }
                        let (value, canonical) = if matches!(kind, Kind::I64) {
                            let v = token.parse::<i64>().map_err(|_| range())?;
                            (Value::I64(v), v.to_string())
                        } else {
                            let v = token.parse::<u64>().map_err(|_| range())?;
                            (Value::U64(v), v.to_string())
                        };
                        if canonical != token {
                            return Err(number());
                        }
                        Ok(value)
                    }
                    Kind::F64 => {
                        let v = token.parse::<f64>().map_err(|_| number())?;
                        if !v.is_finite() {
                            return Err(at!(NonFinite, line, path));
                        }
                        if format!("{v:?}") != token {
                            return Err(number());
                        }
                        Ok(Value::F64(v))
                    }
                    Kind::Enum { variants, .. } => {
                        if !variants.iter().any(|v| v == token) {
                            return Err(at!(BadVariant, line, path, variant = token.into()));
                        }
                        Ok(Value::Enum(token.into()))
                    }
                    _ => Err(self.bad(path)),
                }
            }
        }
    }

    fn next(&mut self) -> Option<char> {
        let mut chars = self.rest.chars();
        let ch = chars.next()?;
        self.rest = chars.as_str();
        Some(ch)
    }

    fn string(&mut self, path: &str) -> Result<String, LoadError> {
        let original = self.rest;
        let line = self.line;
        let escape = || at!(BadEscape, line, path);
        if !self.take("\"") {
            return Err(self.bad(path));
        }
        let mut value = String::new();
        loop {
            let ch = self.next().ok_or_else(|| self.bad(path))?;
            match ch {
                '"' => break,
                '\\' => {
                    let ch = match self.next().ok_or_else(|| self.bad(path))? {
                        '\\' => '\\',
                        '"' => '"',
                        'n' => '\n',
                        'r' => '\r',
                        't' => '\t',
                        'u' => {
                            if !self.take("{") {
                                return Err(escape());
                            }
                            let mut code = 0;
                            let mut count = 0;
                            loop {
                                let ch = self.next().ok_or_else(|| self.bad(path))?;
                                if ch == '}' {
                                    break;
                                }
                                count += 1;
                                if count > 6 {
                                    return Err(escape());
                                }
                                code = code * 16 + ch.to_digit(16).ok_or_else(escape)?;
                            }
                            if count == 0 {
                                return Err(escape());
                            }
                            char::from_u32(code).ok_or_else(escape)?
                        }
                        _ => return Err(escape()),
                    };
                    value.push(ch);
                }
                ch if ch.is_control() => return Err(at!(RawControl, line, path)),
                ch => value.push(ch),
            }
        }
        let mut canonical = String::new();
        write_text(&mut canonical, &value);
        if original.get(..original.len() - self.rest.len()) != Some(canonical.as_str()) {
            return Err(escape());
        }
        Ok(value)
    }
}
