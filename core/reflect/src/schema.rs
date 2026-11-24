//! Canonical schema text and its FNV-1a 64 version signal.
//!
//! R9's docs/NODE16.md will copy these rules: LF-terminated lines, single spaces
//! between items, a `reflect schema 1` header, then root-first depth-first type
//! blocks (`type Name version=V fields=N`, ordered field lines, `end`).
//! Fields write name, kind, since, then present label/unit/min/max/readonly/hidden/
//! default/renamed_from in that order. Kinds use list<K>, array<K;N>, struct<Name>,
//! option<K>, enum<Name:V1|V2>; scalars use their lowercase names. Floats use 0x
//! plus sixteen lowercase bit-pattern digits; integers use decimal. Quoted UTF-8
//! strings escape backslash, quote, LF, CR, TAB and other controls as \u{hex}.
//! Defaults use comma-separated [] lists, {} structs, none/some(v) options,
//! true/false booleans and bare enum variants. Equal named types appear once.

use crate::desc::{Kind, TypeDesc};
use crate::value::Value;
use std::collections::BTreeMap;

/// FNV-1a 64, using the standard offset basis and wrapping prime multiplication.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Formats exactly sixteen zero-padded lowercase hexadecimal digits.
pub fn hash_hex(hash: u64) -> String {
    format!("{hash:016x}")
}

/// A schema cannot assign different descriptors to one type name.
#[derive(Clone, Debug, PartialEq)]
pub enum SchemaError {
    /// A nested type differs from the first descriptor with this name.
    ConflictingType {
        /// The shared type name.
        name: String,
    },
}

impl std::fmt::Display for SchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConflictingType { name } => write!(f, "conflicting schema type: {name}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// Writes the root and all nested struct descriptors in canonical form.
pub fn schema_text(desc: &TypeDesc) -> Result<String, SchemaError> {
    let mut seen = BTreeMap::new();
    seen.insert(desc.name(), desc);
    let mut types = vec![desc];
    let mut pending: Vec<_> = desc.fields().iter().rev().map(|f| &f.kind).collect();
    while let Some(mut kind) = pending.pop() {
        loop {
            match kind {
                Kind::List(inner) | Kind::Array(inner, _) | Kind::Option(inner) => kind = inner,
                Kind::Struct(nested) => {
                    if let Some(first) = seen.get(nested.name()) {
                        if *first != nested.as_ref() {
                            return Err(SchemaError::ConflictingType {
                                name: nested.name().into(),
                            });
                        }
                    } else {
                        seen.insert(nested.name(), nested.as_ref());
                        types.push(nested.as_ref());
                        pending.extend(nested.fields().iter().rev().map(|f| &f.kind));
                    }
                    break;
                }
                _ => break,
            }
        }
    }

    let mut text = String::from("reflect schema 1\n");
    for ty in types {
        text.push_str(&format!(
            "type {} version={} fields={}\n",
            ty.name(),
            ty.version(),
            ty.fields().len()
        ));
        for field in ty.fields() {
            text.push_str(&format!("field {} kind=", field.name));
            write_kind(&mut text, &field.kind);
            text.push_str(&format!(" since={}", field.since));
            for (key, value) in [("label", &field.label), ("unit", &field.unit)] {
                if let Some(value) = value {
                    text.push_str(&format!(" {key}="));
                    write_text(&mut text, value);
                }
            }
            for (key, value) in [("min", field.min), ("max", field.max)] {
                if let Some(value) = value {
                    text.push_str(&format!(" {key}=0x{}", hash_hex(value.to_bits())));
                }
            }
            if field.readonly {
                text.push_str(" readonly");
            }
            if field.hidden {
                text.push_str(" hidden");
            }
            if let Some(value) = &field.default {
                text.push_str(" default=");
                write_value(&mut text, value);
            }
            if let Some(name) = &field.renamed_from {
                text.push_str(&format!(" renamed_from={name}"));
            }
            text.push('\n');
        }
        text.push_str("end\n");
    }
    Ok(text)
}

/// Hashes exactly the bytes returned by `schema_text`.
pub fn schema_hash(desc: &TypeDesc) -> Result<u64, SchemaError> {
    Ok(fnv1a64(schema_text(desc)?.as_bytes()))
}

fn write_kind(text: &mut String, mut kind: &Kind) {
    let mut endings = Vec::new();
    loop {
        match kind {
            Kind::List(inner) | Kind::Option(inner) => {
                text.push_str(kind.name());
                text.push('<');
                endings.push(">".into());
                kind = inner;
            }
            Kind::Array(inner, size) => {
                text.push_str("array<");
                endings.push(format!(";{size}>"));
                kind = inner;
            }
            Kind::Struct(desc) => {
                text.push_str(&format!("struct<{}>", desc.name()));
                break;
            }
            Kind::Enum { name, variants } => {
                text.push_str(&format!("enum<{name}:{}>", variants.join("|")));
                break;
            }
            _ => {
                text.push_str(kind.name());
                break;
            }
        }
    }
    for ending in endings.into_iter().rev() {
        text.push_str(&ending);
    }
}

fn write_text(text: &mut String, value: &str) {
    text.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => text.push_str("\\\\"),
            '"' => text.push_str("\\\""),
            '\n' => text.push_str("\\n"),
            '\r' => text.push_str("\\r"),
            '\t' => text.push_str("\\t"),
            ch if ch.is_control() => text.push_str(&format!("\\u{{{:x}}}", u32::from(ch))),
            ch => text.push(ch),
        }
    }
    text.push('"');
}

fn write_value(text: &mut String, value: &Value) {
    enum Part<'a> {
        Value(&'a Value),
        Char(char),
    }
    let mut pending = vec![Part::Value(value)];
    while let Some(part) = pending.pop() {
        let value = match part {
            Part::Char(ch) => {
                text.push(ch);
                continue;
            }
            Part::Value(value) => value,
        };
        match value {
            Value::Bool(value) => text.push_str(if *value { "true" } else { "false" }),
            Value::I64(value) => text.push_str(&value.to_string()),
            Value::U64(value) => text.push_str(&value.to_string()),
            Value::F64(value) => text.push_str(&format!("0x{}", hash_hex(value.to_bits()))),
            Value::Text(value) => write_text(text, value),
            Value::Enum(value) => text.push_str(value),
            Value::Option(None) => text.push_str("none"),
            Value::Option(Some(value)) => {
                text.push_str("some(");
                pending.push(Part::Char(')'));
                pending.push(Part::Value(value));
            }
            Value::List(values) | Value::Struct(values) => {
                let (open, close) = if matches!(value, Value::List(_)) {
                    ('[', ']')
                } else {
                    ('{', '}')
                };
                text.push(open);
                pending.push(Part::Char(close));
                for (i, value) in values.iter().enumerate().rev() {
                    pending.push(Part::Value(value));
                    if i != 0 {
                        pending.push(Part::Char(','));
                    }
                }
            }
        }
    }
}
