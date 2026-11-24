//! Reflection inspector module.

use crate::desc::{FieldDesc, Kind, TypeDesc};

#[derive(Clone, Debug, PartialEq)]
pub struct Widget {
    pub path: String,
    pub kind: &'static str,
    pub label: String,
    pub unit: Option<String>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub of: Option<&'static str>,
    pub len: Option<usize>,
    pub variants: Vec<String>,
    pub readonly: bool,
}

fn widget(field: &FieldDesc, kind: &Kind, path: String, label: String, readonly: bool) -> Widget {
    let of = match kind {
        Kind::List(k) | Kind::Array(k, _) | Kind::Option(k) => Some(k.name()),
        _ => None,
    };
    Widget {
        path,
        kind: kind.name(),
        label,
        unit: field.unit.clone(),
        min: field.min,
        max: field.max,
        of,
        len: match kind {
            Kind::Array(_, n) => Some(*n),
            _ => None,
        },
        variants: match kind {
            Kind::Enum { variants, .. } => variants.clone(),
            _ => Vec::new(),
        },
        readonly,
    }
}

fn push_fields<'a>(
    stack: &mut Vec<(&'a FieldDesc, String, bool)>,
    desc: &'a TypeDesc,
    prefix: &str,
    readonly: bool,
) {
    for field in desc.fields().iter().rev() {
        let path = if prefix.is_empty() {
            field.name.clone()
        } else {
            format!("{prefix}.{}", field.name)
        };
        stack.push((field, path, readonly));
    }
}

/// Describe visible fields in depth-first descriptor order without reading values.
pub fn widgets(desc: &TypeDesc) -> Vec<Widget> {
    let mut result = Vec::new();
    let mut stack = Vec::new();
    push_fields(&mut stack, desc, "", false);
    while let Some((field, path, inherited)) = stack.pop() {
        if field.hidden {
            continue;
        }
        let readonly = inherited || field.readonly;
        let label = field.label.as_ref().unwrap_or(&field.name).clone();
        result.push(widget(
            field,
            &field.kind,
            path.clone(),
            label.clone(),
            readonly,
        ));
        match &field.kind {
            Kind::Struct(child) => push_fields(&mut stack, child, &path, readonly),
            Kind::Option(k) => {
                if let Kind::Struct(child) = k.as_ref() {
                    push_fields(&mut stack, child, &path, readonly);
                }
            }
            Kind::List(k) | Kind::Array(k, _) => {
                let path = format!("{path}[0]");
                if let Kind::Struct(child) = k.as_ref() {
                    push_fields(&mut stack, child, &path, readonly);
                } else {
                    result.push(widget(field, k, path, format!("{label} item"), readonly));
                }
            }
            _ => {}
        }
    }
    result
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// One line per widget, each ending "\n"; the empty string for no widgets.
pub fn to_text(desc: &TypeDesc) -> String {
    let mut text = String::new();
    for w in widgets(desc) {
        text.push_str(&format!(
            "widget path={} kind={} label={}",
            w.path,
            w.kind,
            quoted(&w.label)
        ));
        if let Some(unit) = w.unit {
            text.push_str(&format!(" unit={}", quoted(&unit)));
        }
        if let Some(min) = w.min {
            text.push_str(&format!(" min={min}"));
        }
        if let Some(max) = w.max {
            text.push_str(&format!(" max={max}"));
        }
        if let Some(of) = w.of {
            text.push_str(&format!(" of={of}"));
        }
        if let Some(len) = w.len {
            text.push_str(&format!(" len={len}"));
        }
        if !w.variants.is_empty() {
            text.push_str(&format!(" variants={}", w.variants.join("|")));
        }
        if w.readonly {
            text.push_str(" readonly");
        }
        text.push('\n');
    }
    text
}
