use reflect::desc::Reflect;
use reflect::sample::{Vehicle, Wheel};
use reflect::{inspector, schema};
use std::io::Write;
use std::path::{Component, Path};

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
        other => {
            let (flag, stage) = match other {
                Mode::Roundtrip { .. } => ("--roundtrip", "R9b"),
                Mode::Load { .. } => ("--load", "R9b"),
                Mode::Script { .. } => ("--script", "R9b"),
                Mode::GlueProof => ("--glue-proof", "R10"),
                Mode::Watch => ("--watch", "R10"),
                Mode::InjectFault => ("--inject-fault", "R10"),
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
