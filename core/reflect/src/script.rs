//! Line commands over any reflected root; saves are returned, never written.

use crate::change::ChangeSink;
use crate::desc::{FieldDesc, Kind, Reflect, TypeDesc};
use crate::limits::Limits;
use crate::path::{get_path, parse_path, set_path, AccessError, Path, PathError, Segment};
use crate::schema::{hash_hex, schema_hash};
use crate::text::{load_text, save_text, SaveError};
use crate::value::Value;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub struct ScriptReport {
    pub commands: usize,
    pub ok: usize,
    pub errors: Vec<LineError>,
    pub output: Vec<String>,
    pub saves: Vec<Saved>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineError {
    pub line: usize,
    pub error: ScriptError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub name: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptError {
    UnknownCommand {
        name: String,
    },
    MissingArgument {
        command: String,
    },
    ExtraArgument {
        command: String,
    },
    BadRoot {
        path: String,
        root: String,
    },
    Path {
        path: String,
        error: PathError,
    },
    Access(AccessError),
    BadValue {
        text: String,
        kind: &'static str,
    },
    NotAList {
        path: String,
        kind: &'static str,
    },
    NotAnItem {
        path: String,
    },
    BadSaveName {
        name: String,
    },
    Save(SaveError),
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownCommand { name } => write!(f, "unknown command {name}"),
            Self::MissingArgument { command } => write!(f, "missing argument for {command}"),
            Self::ExtraArgument { command } => write!(f, "too many arguments for {command}"),
            Self::BadRoot { path, root } => write!(f, "path {path} does not start with {root}."),
            Self::Path { path, error } => write!(f, "bad path {path}: {error}"),
            Self::Access(error) => fmt::Display::fmt(error, f),
            Self::BadValue { text, kind } => write!(f, "bad {kind} value {text}"),
            Self::NotAList { path, kind } => write!(f, "{path} is not a list ({kind})"),
            Self::NotAnItem { path } => write!(f, "remove needs a list item path, got {path}"),
            Self::BadSaveName { name } => write!(f, "bad save name {name}"),
            Self::Save(error) => write!(f, "cannot save: {error}"),
        }
    }
}

impl std::error::Error for ScriptError {}

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptFailure {
    TooLarge {
        bytes: u64,
        max: u64,
    },
    CarriageReturn {
        line: usize,
    },
    Unexpected {
        line: usize,
        error: ScriptError,
    },
    ExpectedErrorMissing {
        line: usize,
    },
}

impl fmt::Display for ScriptFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { bytes, max } => {
                write!(f, "script is {bytes} bytes, over the limit {max}")
            }
            Self::CarriageReturn { line } => {
                write!(f, "line {line}: carriage return (scripts use LF line ends)")
            }
            Self::Unexpected { line, error } => write!(f, "line {line}: {error}"),
            Self::ExpectedErrorMissing { line } => {
                write!(f, "line {line}: expected an error, the command succeeded")
            }
        }
    }
}

impl std::error::Error for ScriptFailure {}

/// Stops at the first unexpected outcome, retaining earlier applied commands.
pub fn run_script(
    text: &str,
    root_name: &str,
    root: &mut dyn Reflect,
    limits: &Limits,
    sink: &mut dyn ChangeSink,
) -> Result<ScriptReport, ScriptFailure> {
    let bytes = text.len() as u64;
    if bytes > limits.max_text_file_bytes {
        return Err(ScriptFailure::TooLarge {
            bytes,
            max: limits.max_text_file_bytes,
        });
    }
    if let Some((prefix, _)) = text.split_once('\r') {
        return Err(ScriptFailure::CarriageReturn {
            line: prefix.bytes().filter(|b| *b == b'\n').count() + 1,
        });
    }
    let mut report = ScriptReport {
        commands: 0,
        ok: 0,
        errors: Vec::new(),
        output: Vec::new(),
        saves: Vec::new(),
    };
    for (index, raw) in text.split_terminator('\n').enumerate() {
        if raw.bytes().all(|b| b == b' ' || b == b'\t') || raw.starts_with('#') {
            continue;
        }
        let line = index + 1;
        let marked = raw.strip_suffix(" # expect-error");
        let command = marked.map_or(raw, |command| command);
        report.commands += 1;
        match execute(command, root_name, root, limits, sink, &mut report) {
            Ok(()) if marked.is_some() => {
                return Err(ScriptFailure::ExpectedErrorMissing { line });
            }
            Ok(()) => report.ok += 1,
            Err(error) if marked.is_some() => {
                report.output.push(format!("error line {line}: {error}"));
                report.errors.push(LineError { line, error });
            }
            Err(error) => return Err(ScriptFailure::Unexpected { line, error }),
        }
    }
    Ok(report)
}

fn script_path(text: &str, root: &str, limits: &Limits) -> Result<(String, Path), ScriptError> {
    let rest = text
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('.'))
        .ok_or_else(|| ScriptError::BadRoot {
            path: text.into(),
            root: root.into(),
        })?;
    Ok((rest.into(), local_path(rest, limits)?))
}

fn local_path(text: &str, limits: &Limits) -> Result<Path, ScriptError> {
    parse_path(text, limits).map_err(|error| ScriptError::Path {
        path: text.into(),
        error,
    })
}

fn target(root: &dyn Reflect, path: &Path) -> Result<(Kind, Value), ScriptError> {
    // Check access first so missing fields and items keep the accessor's errors.
    let value = get_path(root, path).map_err(ScriptError::Access)?;
    let mut kind = Kind::Struct(Box::new(root.dyn_desc()));
    for segment in path.segments() {
        kind = match (segment, kind) {
            (Segment::Field(name), Kind::Struct(desc)) => desc
                .fields()
                .iter()
                .find(|field| field.name == *name)
                .map(|field| field.kind.clone())
                .ok_or_else(|| {
                    ScriptError::Access(AccessError::NoField {
                        path: path.to_string(),
                        name: name.clone(),
                    })
                })?,
            (Segment::Index(_), Kind::List(item) | Kind::Array(item, _)) => *item,
            (Segment::Field(_), kind) => {
                return Err(ScriptError::Access(AccessError::NotAStruct {
                    path: path.to_string(),
                    kind: kind.name(),
                }));
            }
            (Segment::Index(_), kind) => {
                return Err(ScriptError::Access(AccessError::NotIndexable {
                    path: path.to_string(),
                    kind: kind.name(),
                }));
            }
        };
    }
    Ok((kind, value))
}

fn bad_value(text: &str, kind: &Kind) -> ScriptError {
    ScriptError::BadValue {
        text: text.into(),
        kind: kind.name(),
    }
}

fn value_desc(kind: &Kind, text: &str) -> Result<TypeDesc, ScriptError> {
    TypeDesc::new("ScriptValue", 1, vec![FieldDesc::new("v", kind.clone())])
        .map_err(|_| bad_value(text, kind))
}

fn read_value(text: &str, kind: &Kind, limits: &Limits) -> Result<Value, ScriptError> {
    let desc = value_desc(kind, text)?;
    let hash = hash_hex(schema_hash(&desc).map_err(|_| bad_value(text, kind))?);
    let file =
        format!("reflect value 1\ntype ScriptValue version=1 hash={hash}\nv = {text}\nend\n");
    let loaded = load_text(&desc, &file, limits).map_err(|_| bad_value(text, kind))?;
    match loaded.value {
        Value::Struct(values) if values.len() == 1 => {
            values.into_iter().next().ok_or_else(|| bad_value(text, kind))
        }
        _ => Err(bad_value(text, kind)),
    }
}

fn list_items(value: Value, path: &str) -> Result<Vec<Value>, ScriptError> {
    match value {
        Value::List(items) => Ok(items),
        other => Err(ScriptError::Access(AccessError::WrongKind {
            path: path.into(),
            expected: "list",
            got: other.kind_name(),
        })),
    }
}

fn execute(
    line: &str,
    root_name: &str,
    root: &mut dyn Reflect,
    limits: &Limits,
    sink: &mut dyn ChangeSink,
    report: &mut ScriptReport,
) -> Result<(), ScriptError> {
    let (command, args) = line.split_once(' ').unwrap_or((line, ""));
    let missing = || ScriptError::MissingArgument {
        command: command.into(),
    };
    if !matches!(command, "set" | "get" | "add" | "remove" | "save") {
        return Err(ScriptError::UnknownCommand {
            name: command.into(),
        });
    }
    if args.is_empty() {
        return Err(missing());
    }
    if matches!(command, "set" | "add") {
        let (text, token) = args.split_once(' ').ok_or_else(missing)?;
        if text.is_empty() || token.is_empty() {
            return Err(missing());
        }
        let (rest, path) = script_path(text, root_name, limits)?;
        let (kind, current) = target(root, &path)?;
        let value = if command == "add" {
            let Kind::List(item) = kind else {
                return Err(ScriptError::NotAList {
                    path: rest,
                    kind: kind.name(),
                });
            };
            let value = read_value(token, &item, limits)?;
            let mut items = list_items(current, &rest)?;
            items.push(value);
            Value::List(items)
        } else {
            read_value(token, &kind, limits)?
        };
        return set_path(root, &path, value, limits, sink).map_err(ScriptError::Access);
    }
    if args.contains(' ') {
        return Err(ScriptError::ExtraArgument {
            command: command.into(),
        });
    }
    if command == "save" {
        let mut bytes = args.bytes();
        if !bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
            || args.contains("..")
        {
            return Err(ScriptError::BadSaveName { name: args.into() });
        }
        let desc = root.dyn_desc();
        let values = desc
            .fields()
            .iter()
            .enumerate()
            .map(|(i, field)| {
                root.get(i).ok_or_else(|| {
                    ScriptError::Access(AccessError::NoField {
                        path: field.name.clone(),
                        name: field.name.clone(),
                    })
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let text = save_text(&desc, &Value::Struct(values), limits).map_err(ScriptError::Save)?;
        report.output.push(format!("saved {args} bytes={}", text.len()));
        report.saves.push(Saved {
            name: args.into(),
            text,
        });
        return Ok(());
    }
    let (rest, path) = script_path(args, root_name, limits)?;
    if command == "get" {
        let (kind, value) = target(root, &path)?;
        let desc = value_desc(&kind, args)?;
        let text = save_text(&desc, &Value::Struct(vec![value]), limits)
            .map_err(|error| match error {
                SaveError::Schema(_) => bad_value(args, &kind),
                other => ScriptError::Save(other),
            })?;
        let token = text
            .split('\n')
            .nth(2)
            .and_then(|line| line.strip_prefix("v = "))
            .ok_or_else(|| bad_value(args, &kind))?;
        report.output.push(format!("get {args} = {token}"));
        return Ok(());
    }
    let Some(Segment::Index(index)) = path.segments().last() else {
        return Err(ScriptError::NotAnItem { path: rest });
    };
    get_path(root, &path).map_err(ScriptError::Access)?;
    let (parent, _) = rest
        .rsplit_once('[')
        .ok_or_else(|| ScriptError::NotAnItem { path: rest.clone() })?;
    let parent_path = local_path(parent, limits)?;
    let (kind, value) = target(root, &parent_path)?;
    if !matches!(kind, Kind::List(_)) {
        return Err(ScriptError::NotAList {
            path: parent.into(),
            kind: kind.name(),
        });
    }
    let items = list_items(value, parent)?
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i != index)
        .map(|(_, value)| value)
        .collect();
    set_path(root, &parent_path, Value::List(items), limits, sink).map_err(ScriptError::Access)
}
