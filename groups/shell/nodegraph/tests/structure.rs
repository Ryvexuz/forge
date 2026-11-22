use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[derive(Debug)]
struct Manifest {
    tables: BTreeSet<String>,
    entries: BTreeMap<(String, String), String>,
}

// This intentionally reads only the simple TOML subset needed by the skeleton.
// Unsupported syntax is an error, never an ignored line.
fn read_manifest(source: &str) -> Result<Manifest, String> {
    let mut manifest = Manifest {
        tables: BTreeSet::new(),
        entries: BTreeMap::new(),
    };
    let mut table = String::new();
    for (index, raw) in source.lines().enumerate() {
        let line = without_comment(raw)
            .map_err(|error| format!("line {}: {error}", index + 1))?
            .trim();
        if line.is_empty() {
            continue;
        }
        if let Some(section) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if !section.split('.').all(bare_key) {
                return Err(format!("unsupported table: {line}"));
            }
            if !manifest.tables.insert(section.to_owned()) {
                return Err(format!("duplicate table: {section}"));
            }
            table = section.to_owned();
            continue;
        }
        if table.is_empty() {
            return Err("assignment outside a table".into());
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("expected assignment: {line}"))?;
        let key = key.trim();
        if !bare_key(key) {
            return Err(format!("unsupported key: {key}"));
        }
        let value = string_value(value.trim())?;
        if manifest
            .entries
            .insert((table.clone(), key.to_owned()), value)
            .is_some()
        {
            return Err(format!("duplicate key: {table}.{key}"));
        }
    }
    if !manifest.tables.contains("package")
        || !manifest
            .entries
            .contains_key(&("package".into(), "name".into()))
    {
        return Err("missing package name".into());
    }
    Ok(manifest)
}

fn bare_key(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
}

fn without_comment(line: &str) -> Result<&str, String> {
    let mut quote = None;
    for (index, ch) in line.char_indices() {
        match (quote, ch) {
            (None, '#') => return Ok(&line[..index]),
            (None, '"' | '\'') => quote = Some(ch),
            (Some('"'), '\\') => return Err("escaped strings are unsupported".into()),
            (Some(open), close) if open == close => quote = None,
            _ => {}
        }
    }
    if quote.is_some() {
        return Err("unterminated string".into());
    }
    Ok(line)
}

fn string_value(text: &str) -> Result<String, String> {
    for quote in ['"', '\''] {
        if let Some(body) = text.strip_prefix(quote).and_then(|s| s.strip_suffix(quote)) {
            if body.contains(quote) || body.chars().any(char::is_control) {
                return Err("unsupported string content".into());
            }
            return Ok(body.to_owned());
        }
    }
    Err(format!("expected a simple string value: {text}"))
}

fn dependency_free(source: &str) -> Result<(), String> {
    let manifest = read_manifest(source)?;
    for table in &manifest.tables {
        if table.split('.').any(|part| part == "workspace") {
            return Err("crate-local workspace is forbidden".into());
        }
        if table.split('.').any(dependency_key) && table != "dependencies" {
            return Err(format!("dependency table is forbidden: {table}"));
        }
    }
    if !manifest.tables.contains("dependencies") {
        return Err("missing empty [dependencies] table".into());
    }
    for (table, key) in manifest.entries.keys() {
        if table == "dependencies" || dependency_key(key) {
            return Err(format!("dependency assignment is forbidden: {table}.{key}"));
        }
    }
    Ok(())
}

fn dependency_key(key: &str) -> bool {
    matches!(
        key,
        "dependencies" | "dev-dependencies" | "build-dependencies"
    )
}

fn no_abort_on_panic(source: &str) -> Result<(), String> {
    let manifest = read_manifest(source)?;
    for ((table, key), value) in &manifest.entries {
        if key == "panic" && value != "unwind" {
            return Err(format!(
                "non-unwinding panic setting: {table}.{key}={value}"
            ));
        }
    }
    Ok(())
}

fn no_kind_names(source: &str, names: &BTreeSet<String>) -> Result<(), String> {
    if source.trim().is_empty() || names.is_empty() {
        return Err("source and built-in name set must not be empty".into());
    }
    let lower = source.to_ascii_lowercase();
    for name in names {
        if lower.contains(name.as_str()) {
            return Err(format!("source names built-in kind namespace: {name}"));
        }
    }
    Ok(())
}

fn crate_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_source() -> String {
    fs::read_to_string(crate_root().join("Cargo.toml")).expect("read nodegraph Cargo.toml")
}

#[test]
fn cargo_toml_has_no_dependencies() {
    let result = dependency_free(&manifest_source());
    println!("nodegraph dependency-free manifest: {result:?}");
    assert_eq!(result, Ok(()));
}

#[test]
fn cargo_toml_does_not_abort_on_panic() {
    let result = no_abort_on_panic(&manifest_source());
    println!("nodegraph unwinding panic policy: {result:?}");
    assert_eq!(result, Ok(()));
}

#[test]
fn editor_source_names_no_kind() {
    let mut names = BTreeSet::from(["kinds".to_owned()]);
    for entry in fs::read_dir(crate_root().join("src/kinds")).expect("read built-in modules") {
        let path = entry.expect("read built-in directory entry").path();
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .expect("UTF-8 built-in module name");
            if stem != "mod" {
                names.insert(stem.to_ascii_lowercase());
            }
        }
    }
    assert!(names.contains("math"), "math module must participate");
    assert!(names.contains("image"), "image module must participate");
    for file in ["editor.rs", "history.rs", "view.rs"] {
        let source = fs::read_to_string(crate_root().join("src").join(file))
            .expect("read kind-independent source");
        let result = no_kind_names(&source, &names);
        println!("nodegraph src/{file} kind-independent: {result:?}; forbidden={names:?}");
        assert_eq!(result, Ok(()), "{file}");
    }
}

#[test]
fn cargo_toml_identifies_the_skeleton_package() {
    let manifest = read_manifest(&manifest_source()).expect("simple skeleton manifest");
    for (key, expected) in [("name", "nodegraph"), ("edition", "2021")] {
        assert_eq!(
            manifest
                .entries
                .get(&("package".into(), key.into()))
                .map(String::as_str),
            Some(expected)
        );
    }
}

const BASE: &str = "[package]\nname = \"nodegraph\"\n[dependencies]\n";

#[test]
fn dependency_guard_rejects_mutations() {
    assert_eq!(dependency_free(BASE), Ok(()));
    for mutation in [
        "example = \"1\"\n",
        "example = '1' # dependency comment\n",
        "example = { path = \"../other\" }\n",
        "[dev-dependencies]\nexample = \"1\"\n",
        "[build-dependencies]\nexample = \"1\"\n",
        "[target.windows.dependencies]\nexample = \"1\"\n",
        "[target.'cfg(windows)'.dependencies]\nexample = \"1\"\n",
        "[dependencies.example]\n",
        "[workspace]\n",
        "[package.metadata]\ndependencies = \"hidden\"\n",
    ] {
        let result = dependency_free(&format!("{BASE}{mutation}"));
        assert!(result.is_err(), "accepted dependency mutation: {mutation}");
    }
    assert_eq!(
        dependency_free(&format!("{BASE}# example = \"1\"\n")),
        Ok(())
    );
}

#[test]
fn panic_guard_rejects_mutations() {
    assert_eq!(no_abort_on_panic(BASE), Ok(()));
    for mutation in [
        "[profile.release]\npanic = \"abort\"\n",
        "[profile.dev]\npanic='abort' # hostile comment\n",
        "[profile.release]\n\"panic\" = \"abort\"\n",
        "[profile.release]\npanic = \"abort\"\npanic = \"unwind\"\n",
        "[profile.release]\npanic = \"unknown\"\n",
    ] {
        let result = no_abort_on_panic(&format!("{BASE}{mutation}"));
        assert!(result.is_err(), "accepted panic mutation: {mutation}");
    }
    assert_eq!(
        no_abort_on_panic(&format!("{BASE}[profile.release]\npanic = \"unwind\"\n")),
        Ok(())
    );
    assert_eq!(
        no_abort_on_panic(&format!("{BASE}# panic = \"abort\"\n")),
        Ok(())
    );
}

#[test]
fn manifest_reader_rejects_ambiguous_or_empty_input() {
    for source in [
        "",
        "# comments only",
        "[package]\n",
        "[package]\nname = \"nodegraph\n",
        "[package]\nname = \"nodegraph\"\nname = \"other\"\n",
        "[package]\nname = \"nodegraph\"\n[package]\n",
        "[package]\nname = \"\"\"nodegraph\"\"\"\n",
        "[package]\nname = \"node\\\"graph\"\n",
        "[package]\nname = \"node\rgraph\"\n",
        "[package]\nname = { hidden = \"value\" }\n",
        "[package]\nname = [\"nodegraph\"]\n",
    ] {
        assert!(read_manifest(source).is_err(), "accepted {source:?}");
    }
    let manifest = read_manifest("[package]\r\nname = \"node#graph\" # comment\r\n")
        .expect("hash inside string is not a comment");
    assert_eq!(
        manifest
            .entries
            .get(&("package".into(), "name".into()))
            .map(String::as_str),
        Some("node#graph")
    );
}

#[test]
fn kind_name_guard_rejects_mutations() {
    let names = BTreeSet::from(["math".into(), "image".into(), "kinds".into()]);
    assert_eq!(no_kind_names("//! Generic graph editing.", &names), Ok(()));
    for source in [
        "use crate::kinds::math;",
        "use crate::kinds::image;",
        "const NAME: &str = \"Image\";",
        "// Math operations",
        "struct ImageKind;",
        "",
    ] {
        assert!(
            no_kind_names(source, &names).is_err(),
            "accepted kind-name mutation: {source:?}"
        );
    }
    assert!(no_kind_names("generic source", &BTreeSet::new()).is_err());
}

#[test]
fn guards_inspect_large_inputs_without_truncation() {
    // Fixture size only; the guards do not impose an input-length limit.
    let padding = "generic ".repeat(16_384);
    let manifest =
        format!("[package]\nname = \"nodegraph\"\ndescription = \"{padding}\"\n[dependencies]\n");
    assert_eq!(dependency_free(&manifest), Ok(()));
    assert_eq!(no_abort_on_panic(&manifest), Ok(()));
    let dependency = format!("{manifest}example = \"1\"\n");
    assert!(dependency_free(&dependency).is_err());
    let abort = format!("{manifest}[profile.release]\npanic = \"abort\"\n");
    assert!(no_abort_on_panic(&abort).is_err());
    let names = BTreeSet::from(["image".into()]);
    assert_eq!(no_kind_names(&padding, &names), Ok(()));
    assert!(no_kind_names(&format!("{padding}Image"), &names).is_err());
}
