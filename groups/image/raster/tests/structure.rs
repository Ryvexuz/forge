use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const EXTERNAL_USE_ALLOW_LIST: &[&str] = &[];
const MANIFEST: &str = include_str!("../Cargo.toml");

// This is intentionally a restricted manifest grammar, not a partial TOML
// parser that silently accepts syntax it does not understand.
fn check_manifest(manifest: &str) -> Result<(), String> {
    let mut section = "";
    let mut sections = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for (index, line) in manifest.lines().enumerate() {
        let line = manifest_line(line).map_err(|error| format!("line {}: {error}", index + 1))?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if !matches!(line, "[package]" | "[dependencies]") {
                return Err(format!("unsupported or forbidden table: {line}"));
            }
            if !sections.insert(line.to_owned()) {
                return Err(format!("duplicate table: {line}"));
            }
            section = if line == "[package]" {
                "package"
            } else {
                "dependencies"
            };
            continue;
        }
        if section != "package" {
            return Err(format!("entry outside package metadata: {line}"));
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| format!("invalid metadata: {line}"))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            || !keys.insert(key.to_owned())
        {
            return Err(format!("unsupported or duplicate metadata key: {key}"));
        }
        let value = value.trim();
        let quote = value
            .chars()
            .next()
            .filter(|ch| matches!(ch, '"' | '\''))
            .ok_or_else(|| format!("metadata must be a single-line string: {key}"))?;
        let inner = value
            .strip_prefix(quote)
            .and_then(|value| value.strip_suffix(quote))
            .ok_or_else(|| format!("unterminated metadata string: {key}"))?;
        if inner.contains(quote) || inner.contains('\\') || inner.chars().any(char::is_control) {
            return Err(format!("unsupported string syntax: {key}"));
        }
        if (key == "name" && inner != "raster") || (key == "edition" && inner != "2021") {
            return Err(format!("unexpected {key}: {inner}"));
        }
        if inner.is_empty() {
            return Err(format!("empty metadata: {key}"));
        }
    }
    if !sections.contains("[package]") || !sections.contains("[dependencies]") {
        return Err("missing package or empty dependencies table".into());
    }
    for key in ["name", "version", "edition"] {
        if !keys.contains(key) {
            return Err(format!("missing package key: {key}"));
        }
    }
    Ok(())
}

fn manifest_line(line: &str) -> Result<&str, String> {
    let mut quote = None;
    for (index, ch) in line.char_indices() {
        match (quote, ch) {
            (None, '#') => return Ok(&line[..index]),
            (None, '"' | '\'') => quote = Some(ch),
            (Some(open), close) if open == close => quote = None,
            (_, '\\') => return Err("escapes are not supported in this manifest grammar".into()),
            _ => {}
        }
    }
    if quote.is_some() {
        return Err("unterminated string".into());
    }
    Ok(line)
}

#[test]
fn cargo_toml_has_no_dependencies() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest =
        fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    check_manifest(&manifest).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    println!(
        "manifest_checked: {}; dependencies: empty; workspace_table: absent",
        path.display()
    );
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    Symbol(char),
    Literal,
}

// Strings, raw strings, characters, and nested comments cannot introduce imports.
fn tokens(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        let next = chars.get(i + 1).copied();
        if ch.is_whitespace() {
            i += 1;
        } else if ch == '/' && next == Some('/') {
            i += 2;
            while i < chars.len() && !matches!(chars[i], '\n' | '\r') {
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            i += 2;
            let mut depth = 1_usize;
            while i < chars.len() && depth != 0 {
                if chars[i] == '/' && chars.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && chars.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            if depth != 0 {
                return Err("unterminated block comment".into());
            }
        } else if let Some((body, hashes)) = raw_string_start(&chars, i) {
            i = body;
            loop {
                if i == chars.len() {
                    return Err("unterminated raw string".into());
                }
                if chars[i] == '"'
                    && chars[i + 1..].iter().take(hashes).count() == hashes
                    && chars[i + 1..].iter().take(hashes).all(|ch| *ch == '#')
                {
                    i += 1 + hashes;
                    break;
                }
                i += 1;
            }
            result.push(Token::Literal);
        } else if ch == '"' {
            i = quoted_end(&chars, i, '"')?;
            result.push(Token::Literal);
        } else if ch == '\'' && (next == Some('\\') || chars.get(i + 2) == Some(&'\'')) {
            i = quoted_end(&chars, i, '\'')?;
            result.push(Token::Literal);
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            if ch == 'r' && next == Some('#') {
                i += 2;
            }
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            result.push(Token::Word(chars[start..i].iter().collect()));
        } else {
            result.push(Token::Symbol(ch));
            i += 1;
        }
    }
    Ok(result)
}

fn raw_string_start(chars: &[char], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    if matches!(chars.get(i), Some('b' | 'c')) {
        i += 1;
    }
    if chars.get(i) != Some(&'r') {
        return None;
    }
    i += 1;
    let mut hashes = 0;
    while chars.get(i) == Some(&'#') {
        hashes += 1;
        i += 1;
    }
    (chars.get(i) == Some(&'"')).then_some((i + 1, hashes))
}

fn quoted_end(chars: &[char], start: usize, quote: char) -> Result<usize, String> {
    let mut i = start + 1;
    while i < chars.len() {
        if chars[i] == quote {
            return Ok(i + 1);
        }
        if chars[i] == '\\' {
            i += 1;
        }
        i += 1;
    }
    Err("unterminated literal".into())
}

fn check_uses(source: &str, external: &[&str]) -> Result<(), String> {
    let tokens = tokens(source)?;
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] != Token::Word("use".into()) {
            i += 1;
            continue;
        }
        i += 1;
        let mut rooted = false;
        let mut groups = Vec::new();
        loop {
            match tokens.get(i) {
                Some(Token::Symbol(';')) if groups.is_empty() => break,
                Some(Token::Symbol('{')) => groups.push(rooted),
                Some(Token::Symbol('}')) => {
                    rooted = groups.pop().ok_or("unmatched import group")?;
                }
                Some(Token::Symbol(',')) => {
                    rooted = *groups.last().ok_or("comma outside import group")?;
                }
                Some(Token::Word(root)) if !rooted => {
                    let root = root.strip_prefix("r#").unwrap_or(root);
                    if !matches!(root, "std" | "core" | "alloc" | "crate" | "self" | "super")
                        && !external.contains(&root)
                    {
                        return Err(format!("external use root is not allowed: {root}"));
                    }
                    rooted = true;
                }
                Some(Token::Word(_)) | Some(Token::Symbol(':')) => {}
                Some(Token::Symbol('*')) if rooted => {}
                _ => return Err("unsupported or unterminated use tree".into()),
            }
            i += 1;
        }
        i += 1;
    }
    Ok(())
}

fn check_source_tree(path: &Path, files: &mut usize) -> Result<(), String> {
    let entries = fs::read_dir(path).map_err(|error| format!("{}: {error}", path.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            check_source_tree(&path, files)?;
        } else if kind.is_file() {
            let source = fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            check_uses(&source, EXTERNAL_USE_ALLOW_LIST)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            *files += 1;
        } else {
            return Err(format!(
                "not a regular source file/directory: {}",
                path.display()
            ));
        }
    }
    Ok(())
}

#[test]
fn src_has_no_path_use_outside_the_crate() {
    let mut files = 0;
    check_source_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    )
    .unwrap_or_else(|error| panic!("source imports: {error}"));
    assert!(files > 0, "source traversal must not pass on an empty tree");
    println!("source_files_checked: {files}; external_use_allow_list: {EXTERNAL_USE_ALLOW_LIST:?}");
}

#[test]
fn manifest_checker_rejects_dependency_mutations() {
    assert_eq!(check_manifest(MANIFEST), Ok(()));
    for entry in [
        "foreign = \"1\"",
        "foreign = { path = \"../foreign\" }",
        "\"foreign\" = { git = \"https://invalid.example/foreign\" }",
        "[dependencies.foreign]\nversion = \"1\"",
        "[dev-dependencies]\nforeign = \"1\"",
        "[build-dependencies]\nforeign = \"1\"",
        "[target.'cfg(windows)'.dependencies]\nforeign = \"1\"",
        "[workspace]",
        "[dependencies]",
        "foreign.version = \"1\"",
    ] {
        let mutated = format!("{MANIFEST}\n{entry}\n");
        assert!(
            check_manifest(&mutated).is_err(),
            "accepted dependency/workspace mutation: {entry}"
        );
    }
    let comments = format!("# [workspace]\r\n{MANIFEST}\r\n# foreign = \"1\"\r\n");
    assert_eq!(check_manifest(&comments), Ok(()));
    assert!(check_manifest("").is_err());
    assert!(check_manifest(&MANIFEST.replace("2021", "2018")).is_err());
    assert!(check_manifest(&MANIFEST.replace("[dependencies]", "")).is_err());
    assert!(check_manifest(&format!("{MANIFEST}\nforeign = \"unterminated")).is_err());
    assert!(check_manifest(&MANIFEST.replace("edition =", "name =")).is_err());
    assert!(check_manifest(&MANIFEST.replace("\"2021\"", "\"20\r21\"")).is_err());
    assert!(check_manifest(&MANIFEST.replace("\"raster\"", "\"C:\\\\foreign\"")).is_err());
    let long_comment = format!("#{}\n{MANIFEST}\nforeign = \"1\"\n", "x".repeat(100_000));
    assert!(check_manifest(&long_comment).is_err());
}

#[test]
fn import_checker_rejects_external_import_variants() {
    for source in [
        "use foreign::Thing;",
        "pub use ::foreign::Thing as Alias;",
        "use r#foreign::Thing;",
        "use {std::fmt, foreign::Thing};",
        "use {\r\n std::fmt,\r\n {::foreign::Thing}\r\n};",
        "mod nested { use foreign::*; }",
        "use std::fmt; use foreign::Thing;",
        "use std::{fmt, io}; use {core::mem, foreign::*};",
    ] {
        assert!(
            check_uses(source, &[]).is_err(),
            "accepted external import: {source}"
        );
    }
    assert_eq!(check_uses("use foreign::Thing;", &["foreign"]), Ok(()));
    assert!(check_uses("use other::Thing;", &["foreign"]).is_err());
    assert!(check_uses("use std::{fmt", &[]).is_err());
    assert!(check_uses("use std::fmt", &[]).is_err());
    assert!(check_uses("/* unterminated", &[]).is_err());
    assert!(check_uses("const X: &str = \"unterminated", &[]).is_err());
    assert!(check_uses("const X: &str = r###\"unterminated", &[]).is_err());
}

#[test]
fn import_checker_ignores_comments_and_literals_but_not_following_imports() {
    let source = r####"
        // use foreign::Thing;
        /* nested /* use foreign::Thing; */ comment */
        const TEXT: &str = "escaped \" use foreign::Thing;";
        const RAW: &str = r###"use foreign::Thing; \"##"###;
        const BYTES: &[u8] = br#"use foreign::Thing;"#;
        const CSTRING: &CStr = cr#"use foreign::Thing;"#;
        const C: char = '"';
        const ESCAPED: char = '\'';
        struct Keyword { r#use: bool }
        fn borrow<'a>(x: &'a str) -> &'a str { x }
        pub(crate) use ::std::{fmt, io::{self, Read}};
        use r#std::fmt as renamed;
        use {core::mem, alloc::vec, crate::tile, self::local, super::parent};
    "####;
    assert_eq!(check_uses(source, &[]), Ok(()));
    assert!(check_uses(&format!("{source}\nuse foreign::Thing;"), &[]).is_err());
    let long_comment = format!("/* {} */ use foreign::Thing;", "x".repeat(100_000));
    assert!(check_uses(&long_comment, &[]).is_err());
}
