use std::fs;
use std::path::{Path, PathBuf};

/// This skeleton intentionally permits only package metadata and empty
/// dependency tables. Unknown tables fail closed, including dependency subtables
/// and target-specific dependency tables.
fn check_manifest(manifest: &str) -> Result<(), String> {
    let mut section = None;
    let mut tables = Vec::new();
    for (index, line) in manifest.lines().enumerate() {
        // The skeleton has no multiline TOML values. Reject them rather than
        // interpreting their contents as table headers or comments.
        if line.contains("\"\"\"") || line.contains("'''") {
            return Err(format!("unsupported multiline TOML at line {}", index + 1));
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            let (header, remainder) = line
                .split_once(']')
                .ok_or_else(|| format!("invalid table header at line {}", index + 1))?;
            if !remainder.trim().is_empty() && !remainder.trim().starts_with('#') {
                return Err(format!("invalid table suffix at line {}", index + 1));
            }
            let name = header[1..].trim();
            if !matches!(
                name,
                "package" | "dependencies" | "dev-dependencies" | "build-dependencies"
            ) {
                return Err(format!("unsupported manifest table: {name}"));
            }
            if tables.contains(&name) {
                return Err(format!("duplicate manifest table: {name}"));
            }
            tables.push(name);
            section = Some(name);
        } else if section != Some("package") {
            return Err(format!("nonempty dependency table at line {}", index + 1));
        }
    }
    if !tables.contains(&"package") || !tables.contains(&"dependencies") {
        return Err("missing package or dependencies table".to_owned());
    }
    Ok(())
}

#[derive(Debug, PartialEq)]
enum Token {
    Word(String),
    RawWord(String),
    Symbol(char),
}

fn word_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

/// Tokenize imports without treating comments or literal contents as code.
fn tokens(source: &str) -> Result<Vec<Token>, String> {
    let chars: Vec<char> = source.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && chars.get(i + 1) == Some(&'*') {
            i += 2;
            let mut depth: usize = 1;
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
                return Err("unterminated block comment".to_owned());
            }
        } else {
            // Raw strings may have byte/C prefixes. Raw identifiers instead
            // proceed through the identifier branch below.
            let raw_start = if c == 'r' {
                Some(i + 1)
            } else if matches!(c, 'b' | 'c') && chars.get(i + 1) == Some(&'r') {
                Some(i + 2)
            } else {
                None
            };
            if let Some(start) = raw_start {
                let mut quote = start;
                while chars.get(quote) == Some(&'#') {
                    quote += 1;
                }
                if chars.get(quote) == Some(&'"') {
                    let hashes = quote - start;
                    i = quote + 1;
                    loop {
                        if i >= chars.len() {
                            return Err("unterminated raw string".to_owned());
                        }
                        if chars[i] == '"'
                            && (0..hashes).all(|n| chars.get(i + 1 + n) == Some(&'#'))
                        {
                            i += 1 + hashes;
                            break;
                        }
                        i += 1;
                    }
                    result.push(Token::Symbol('"'));
                    continue;
                }
            }
            if c == '"' {
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated string".to_owned()),
                        Some('"') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => i += 2,
                        Some(_) => i += 1,
                    }
                }
                result.push(Token::Symbol('"'));
            } else if c == '\'' {
                // A lifetime has no closing quote after its identifier.
                let mut end = i + 1;
                if chars.get(end).is_some_and(|c| word_start(*c)) {
                    while chars
                        .get(end)
                        .is_some_and(|c| word_start(*c) || c.is_ascii_digit())
                    {
                        end += 1;
                    }
                    if chars.get(end) != Some(&'\'') {
                        result.push(Token::Symbol('\''));
                        i += 1;
                        continue;
                    }
                }
                i += 1;
                loop {
                    match chars.get(i) {
                        None => return Err("unterminated character literal".to_owned()),
                        Some('\'') => {
                            i += 1;
                            break;
                        }
                        Some('\\') => i += 2,
                        Some(_) => i += 1,
                    }
                }
                result.push(Token::Symbol('\''));
            } else if word_start(c) {
                let raw = c == 'r' && chars.get(i + 1) == Some(&'#');
                if raw {
                    i += 2;
                }
                let start = i;
                while chars
                    .get(i)
                    .is_some_and(|c| word_start(*c) || c.is_ascii_digit())
                {
                    i += 1;
                }
                if i == start {
                    return Err("invalid raw identifier".to_owned());
                }
                let word = chars[start..i].iter().collect();
                result.push(if raw {
                    Token::RawWord(word)
                } else {
                    Token::Word(word)
                });
            } else {
                result.push(Token::Symbol(c));
                i += 1;
            }
        }
    }
    Ok(result)
}

fn allowed_root(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc" | "crate" | "self" | "super")
}

fn check_imports(source: &str) -> Result<(), String> {
    let tokens = tokens(source)?;
    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token, Token::Word(word) if word == "use") {
            continue;
        }
        // Root-level groups can contain further groups. Once an allowed root
        // has been consumed, its entire subtree is inside that root.
        let mut root_needed = true;
        let mut groups = Vec::new();
        let mut terminated = false;
        for token in &tokens[index + 1..] {
            match token {
                Token::Word(name) | Token::RawWord(name) if root_needed => {
                    if !allowed_root(name) {
                        return Err(format!("disallowed import root: {name}"));
                    }
                    root_needed = false;
                }
                Token::Symbol('{') => {
                    groups.push(root_needed);
                }
                Token::Symbol('}') => {
                    root_needed = groups
                        .pop()
                        .ok_or_else(|| "unbalanced import group".to_owned())?;
                }
                Token::Symbol(',') => {
                    root_needed = groups.last().copied().unwrap_or(true);
                }
                Token::Symbol(';') => {
                    if !groups.is_empty() {
                        return Err("unclosed import group".to_owned());
                    }
                    terminated = true;
                    break;
                }
                Token::Symbol(':') => {}
                _ if root_needed => return Err("invalid import root".to_owned()),
                _ => {}
            }
        }
        if !terminated {
            return Err("unterminated use declaration".to_owned());
        }
    }
    Ok(())
}

fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let entry = entry.expect("read source entry");
        let kind = entry.file_type().expect("read source entry type");
        assert!(!kind.is_symlink(), "source links are not supported");
        let path = entry.path();
        if kind.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

#[test]
fn cargo_toml_has_no_dependencies() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = fs::read_to_string(path).expect("read scene Cargo.toml");
    check_manifest(&manifest).expect("scene manifest must have empty dependency tables");
}

#[test]
fn src_uses_only_standard_or_local_roots() {
    let mut files = Vec::new();
    source_files(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut files,
    );
    files.sort();
    assert!(!files.is_empty(), "source scan must not be empty");
    for path in files {
        let source = fs::read_to_string(&path).expect("read Rust source");
        if let Err(error) = check_imports(&source) {
            panic!("{}: {error}", path.display());
        }
    }
}

#[test]
fn dependency_guard_rejects_inserted_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    check_manifest(manifest).expect("baseline manifest");
    let mutated = manifest.replace("[dependencies]", "[dependencies]\nforeign = \"1\"");
    assert_ne!(manifest, mutated, "mutation must change the manifest");
    assert!(check_manifest(&mutated)
        .expect_err("inserted dependency must fail")
        .contains("nonempty dependency table"));
    for table in ["dev-dependencies", "build-dependencies"] {
        check_manifest(&format!("{manifest}\n[{table}]\n# empty\n"))
            .expect("empty dependency table");
        assert!(check_manifest(&format!("{manifest}\n[{table}]\nforeign = {{}}\n")).is_err());
    }
    for suffix in [
        "[dependencies.foreign]\nversion = \"1\"",
        "[target.'cfg(windows)'.dependencies]\nforeign = \"1\"",
        "[workspace]",
        "[dependencies]\nforeign = \"1\"",
        "[\"dependencies\"]\nforeign = \"1\"",
    ] {
        assert!(check_manifest(&format!("{manifest}\n{suffix}\n")).is_err());
    }
    assert!(check_manifest("").is_err());
}

#[test]
fn import_guard_checks_groups_and_ignores_literals_and_comments() {
    for source in [
        "use std::fmt; pub(crate) use crate::scene;",
        "use {std::fmt, core::mem, {alloc::vec, self::local, super::parent}};",
        "use ::std::{fmt, collections::{HashMap, HashSet}};",
        "use r#std::fmt;",
        "use crate::r#use;",
        "// use foreign::X;\n/* outer /* use foreign::Y; */ */",
        "const S: &str = \"use foreign::X;\";",
        "const S: &str = r###\"use foreign::X;\"###;",
        "const S: &[u8] = br#\"use foreign::X;\"#;",
        "const C: char = '\"'; fn f<'a>(s: &'a str) {}",
    ] {
        check_imports(source).unwrap_or_else(|error| panic!("{source}: {error}"));
    }
    for source in [
        "use foreign::X;",
        "pub use ::foreign::X;",
        "use {std::fmt, foreign::X};",
        "use {{std::fmt}, {foreign::X}};",
        "use r#foreign::X;",
        "use /* comment */ foreign::X;",
        "use\nforeign\n::X;",
        "mod nested { use foreign::X; }",
    ] {
        assert!(
            check_imports(source)
                .expect_err(source)
                .contains("disallowed import root"),
            "{source}"
        );
    }
    for source in ["use std::fmt", "use {std::fmt;", "/*", "\"", "r#\""] {
        assert!(check_imports(source).is_err(), "{source}");
    }
    // Fixture size, not a parser limit: the guard must reach the final import.
    let repeated = "/* use foreign::X; */\n".repeat(4096);
    check_imports(&repeated).expect("comment-only input");
    assert!(check_imports(&format!("{repeated}use foreign::X;")).is_err());
}
