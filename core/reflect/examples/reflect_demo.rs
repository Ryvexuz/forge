use reflect::change::{Change, ChangeSink};
use reflect::desc::{Reflect, TypeDesc};
use reflect::limits::Limits;
use reflect::sample::{DriveMode, Vehicle, Wheel};
use reflect::text::{load_text, save_text, Note};
use reflect::value::Value;
use reflect::{inspector, schema, script::run_script};
use std::io::Write;
use std::path::{Component, Path};

#[path = "reflect_demo/fault.rs"]
pub mod fault;
#[path = "reflect_demo/watch.rs"]
pub mod watch;

#[derive(Debug, PartialEq, Eq)]
pub enum Mode {
    ExpectSchema { ty: String, file: String },
    ExpectInspector { ty: String, file: String },
    Roundtrip { ty: String, file: String },
    Load { ty: String, file: String },
    Script { file: String, expected: String },
    GlueProof,
    Watch,
    InjectFault,
}

fn relative(value: &str) -> Result<String, String> {
    // Check both separator conventions even when running on a Unix host.
    if value.is_empty()
        || value.contains(':')
        || value.chars().any(char::is_control)
        || value
            .split(['/', '\\'])
            .any(|p| p.is_empty() || p == "." || p == "..")
        || !Path::new(value)
            .components()
            .all(|p| matches!(p, Component::Normal(_)))
    {
        return Err(format!("usage: invalid relative file {value:?}"));
    }
    Ok(value.to_owned())
}

/// Parse exactly one mode. Errors start with `usage: `.
pub fn parse(args: &[String]) -> Result<Mode, String> {
    let flag = args
        .first()
        .ok_or_else(|| "usage: expected one mode flag".to_owned())?;
    let arity = match flag.as_str() {
        "--expect-schema" | "--expect-inspector" | "--roundtrip" | "--load" | "--script" => 2,
        "--glue-proof" | "--watch" | "--inject-fault" => 0,
        _ => return Err(format!("usage: unknown flag {flag:?}")),
    };
    if args.len() != arity + 1 || args[1..].iter().any(|v| v.starts_with("--")) {
        return Err(format!("usage: {flag:?} requires exactly {arity} values"));
    }
    match flag.as_str() {
        "--glue-proof" => Ok(Mode::GlueProof),
        "--watch" => Ok(Mode::Watch),
        "--inject-fault" => Ok(Mode::InjectFault),
        "--script" => Ok(Mode::Script {
            file: relative(&args[1])?,
            expected: relative(&args[2])?,
        }),
        _ => {
            let ty = args[1].clone();
            if ty != "Vehicle" && ty != "Wheel" {
                return Err(format!("usage: unknown type {ty:?} for {flag}"));
            }
            let file = relative(&args[2])?;
            Ok(match flag.as_str() {
                "--expect-schema" => Mode::ExpectSchema { ty, file },
                "--expect-inspector" => Mode::ExpectInspector { ty, file },
                "--roundtrip" => Mode::Roundtrip { ty, file },
                _ => Mode::Load { ty, file },
            })
        }
    }
}

fn execute(args: &[String], root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let mode = parse(args)?;
    let (ty, file, is_schema) = match mode {
        Mode::ExpectSchema { ty, file } => (ty, file, true),
        Mode::ExpectInspector { ty, file } => (ty, file, false),
        Mode::Roundtrip { ty, file } => return roundtrip(&ty, &file, root, out),
        Mode::Load { ty, file } => return load(&ty, &file, root, out),
        Mode::Script { file, expected } => return script(&file, &expected, root, out),
        Mode::Watch => return watch::run(out),
        Mode::InjectFault => return fault::run(root, out),
        other => {
            let (flag, stage) = match other {
                Mode::GlueProof => ("--glue-proof", "R10"),
                _ => return Err("usage: invalid mode".into()),
            };
            return Err(format!("usage: {flag} is not implemented yet ({stage})"));
        }
    };
    let expected = std::fs::read(root.join(&file))
        .map_err(|e| format!("usage: cannot read {file:?}: {e}"))?;
    let desc = if ty == "Vehicle" {
        Vehicle::desc()
    } else {
        Wheel::desc()
    };
    let (text, prefix) = if is_schema {
        let text = schema::schema_text(&desc).map_err(|e| format!("FAIL schema {e}"))?;
        let hash = schema::schema_hash(&desc).map_err(|e| format!("FAIL schema {e}"))?;
        let prefix = format!(
            "schema {ty} version={} fields={} hash={}",
            desc.version(),
            desc.fields().len(),
            schema::hash_hex(hash)
        );
        (text, prefix)
    } else {
        (
            inspector::to_text(&desc),
            format!("inspect {ty} widgets={}", inspector::widgets(&desc).len()),
        )
    };
    let different = text.as_bytes() != expected;
    writeln!(
        out,
        "{prefix} expected={file} result={}",
        if different { "FAIL" } else { "PASS" }
    )
    .map_err(|e| format!("FAIL output: {e}"))?;
    if different {
        let mut actual_lines = text.as_bytes().split_inclusive(|b| *b == b'\n');
        let mut expected_lines = expected.split_inclusive(|b| *b == b'\n');
        let mut line = 1;
        loop {
            let a = actual_lines.next();
            let b = expected_lines.next();
            if a != b {
                break;
            }
            if a.is_none() {
                return Err("FAIL comparison: unequal bytes without differing line".into());
            }
            line += 1;
        }
        writeln!(out, "diff line={line}").map_err(|e| format!("FAIL output: {e}"))?;
        Ok(1)
    } else {
        Ok(0)
    }
}

fn limits() -> Result<Limits, String> {
    Limits::parse(include_str!("../reflect.toml")).map_err(|e| format!("FAIL limits {e}"))
}

fn read(root: &Path, file: &str) -> Result<Vec<u8>, String> {
    std::fs::read(root.join(file)).map_err(|e| format!("usage: cannot read {file:?}: {e}"))
}

fn line(out: &mut dyn Write, text: impl std::fmt::Display) -> Result<(), String> {
    writeln!(out, "{text}").map_err(|e| format!("FAIL output: {e}"))
}

// Zero means identical bytes (possible when only the script marker count fails).
fn diff_line(actual: &[u8], expected: &[u8]) -> usize {
    let mut a = actual.split_inclusive(|b| *b == b'\n');
    let mut b = expected.split_inclusive(|b| *b == b'\n');
    let mut number = 1;
    loop {
        let left = a.next();
        let right = b.next();
        if left != right {
            return number;
        }
        if left.is_none() {
            return 0;
        }
        number += 1;
    }
}

fn wheel() -> Wheel {
    Wheel {
        radius_m: 0.31,
        width_m: 0.21,
        steered: true,
    }
}

fn vehicle() -> Vehicle {
    Vehicle {
        name: "Rally".into(),
        mass_kg: 1350.0,
        position: [0.0, 0.5, -2.25],
        wheels: vec![
            wheel(),
            Wheel {
                radius_m: 0.32,
                width_m: 0.22,
                steered: false,
            },
        ],
        note: Some("demo".into()),
        drive: DriveMode::All,
    }
}

fn snapshot(object: &dyn Reflect) -> Result<Value, String> {
    object
        .dyn_desc()
        .fields()
        .iter()
        .enumerate()
        .map(|(i, field)| {
            object
                .get(i)
                .ok_or_else(|| format!("missing reflected field {}", field.name))
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Struct)
}

fn descriptor(ty: &str) -> TypeDesc {
    if ty == "Vehicle" {
        Vehicle::desc()
    } else {
        Wheel::desc()
    }
}

fn roundtrip(ty: &str, file: &str, root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let expected = read(root, file)?;
    let limits = limits()?;
    let desc = descriptor(ty);
    let value = if ty == "Vehicle" {
        snapshot(&vehicle())
    } else {
        snapshot(&wheel())
    }
    .map_err(|e| format!("FAIL roundtrip {e}"))?;
    let first = save_text(&desc, &value, &limits).map_err(|e| format!("FAIL roundtrip {e}"))?;
    let loaded = load_text(&desc, &first, &limits).map_err(|e| format!("FAIL roundtrip {e}"))?;
    let second =
        save_text(&desc, &loaded.value, &limits).map_err(|e| format!("FAIL roundtrip {e}"))?;
    let golden = first.as_bytes() == expected;
    let text_identical = first == second;
    let value_equal = value == loaded.value;
    let pass = golden && text_identical && value_equal;
    line(
        out,
        format!(
            "roundtrip {ty} bytes={} golden={golden} text_identical={text_identical} value_equal={value_equal} result={}",
            first.len(),
            if pass { "PASS" } else { "FAIL" }
        ),
    )?;
    if !golden {
        line(
            out,
            format!("diff line={}", diff_line(first.as_bytes(), &expected)),
        )?;
    }
    Ok(u8::from(!pass))
}

fn load(ty: &str, file: &str, root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let bytes = read(root, file)?;
    let limits = limits()?;
    let desc = descriptor(ty);
    let loaded = match std::str::from_utf8(&bytes) {
        Ok(text) => load_text(&desc, text, &limits).map_err(|e| e.to_string()),
        Err(_) => Err("file is not UTF-8".into()),
    };
    match loaded {
        Ok(loaded) => {
            let mut defaulted = 0usize;
            let mut renamed = 0usize;
            let mut ignored = 0usize;
            for note in &loaded.notes {
                match note {
                    Note::Defaulted { .. } => defaulted += 1,
                    Note::Renamed { .. } => renamed += 1,
                    Note::Ignored { .. } => ignored += 1,
                }
            }
            line(
                out,
                format!(
                    "load {ty} file={file} from_version={} to_version={} defaulted={defaulted} renamed={renamed} ignored={ignored} result=PASS",
                    loaded.from_version,
                    desc.version()
                ),
            )?;
            for note in loaded.notes {
                line(out, format!("note: {note}"))?;
            }
            Ok(0)
        }
        Err(error) => {
            line(out, format!("load {ty} file={file} result=FAIL"))?;
            line(out, format!("error: {error}"))?;
            Ok(1)
        }
    }
}

struct DropChanges;

impl ChangeSink for DropChanges {
    fn changed(&mut self, _: &Change) {}
}

fn script(file: &str, expected: &str, root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let bytes = read(root, file)?;
    let expected_bytes = read(root, expected)?;
    let limits = limits()?;
    let mut vehicle = vehicle();
    let report = match std::str::from_utf8(&bytes) {
        Ok(text) => run_script(text, "vehicle", &mut vehicle, &limits, &mut DropChanges)
            .map(|report| {
                let marked = text
                    .split_terminator('\n')
                    .filter(|line| line.ends_with(" # expect-error"))
                    .count();
                (report, marked)
            })
            .map_err(|e| e.to_string()),
        Err(_) => Err("file is not UTF-8".into()),
    };
    let (report, marked) = match report {
        Ok(report) => report,
        Err(error) => {
            line(out, format!("script {file} result=FAIL"))?;
            line(out, format!("error: {error}"))?;
            return Ok(1);
        }
    };
    let value = snapshot(&vehicle).map_err(|e| format!("FAIL script {e}"))?;
    let final_text =
        save_text(&Vehicle::desc(), &value, &limits).map_err(|e| format!("FAIL script {e}"))?;
    let pass = report.errors.len() == marked && final_text.as_bytes() == expected_bytes;
    line(
        out,
        format!(
            "script {file} commands={} ok={} errors={} expected={expected} result={}",
            report.commands,
            report.ok,
            report.errors.len(),
            if pass { "PASS" } else { "FAIL" }
        ),
    )?;
    for error in report.errors {
        line(out, format!("error line {}: {}", error.line, error.error))?;
    }
    if !pass {
        line(
            out,
            format!(
                "diff line={}",
                diff_line(final_text.as_bytes(), &expected_bytes)
            ),
        )?;
    }
    Ok(u8::from(!pass))
}

/// Run one mode; paths are relative to the supplied crate root.
pub fn run(args: &[String], root: &Path, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    match execute(args, root, out) {
        Ok(code) => code,
        Err(message) => {
            let code = if message.starts_with("usage: ") { 2 } else { 1 };
            if writeln!(err, "reflect_demo: {message}").is_err() {
                return 1;
            }
            code
        }
    }
}

#[cfg(not(test))]
fn main() {
    let args: Result<Vec<_>, _> = std::env::args_os().skip(1).map(|v| v.into_string()).collect();
    let code = match args {
        Ok(args) => run(
            &args,
            Path::new(env!("CARGO_MANIFEST_DIR")),
            &mut std::io::stdout(),
            &mut std::io::stderr(),
        ),
        Err(_) => {
            if writeln!(std::io::stderr(), "reflect_demo: usage: non-UTF-8 argument").is_err() {
                std::process::exit(1);
            }
            2
        }
    };
    std::process::exit(i32::from(code));
}
