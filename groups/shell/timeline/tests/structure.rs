use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Deliberately accept a small, single-line TOML subset rather than partially
/// interpreting arbitrary TOML. Unsupported syntax must fail closed.
fn check_manifest(text: &str) -> Result<(), String> {
    let mut table = "";
    let mut tables = BTreeSet::new();
    let mut fields = BTreeSet::new();

    for (index, raw) in text.lines().enumerate() {
        let line = raw.split_once('#').map_or(raw, |(head, _)| head).trim();
        if line.is_empty() {
            continue;
        }
        let error = |reason: &str| format!("manifest line {}: {reason}", index + 1);
        if line.starts_with('[') {
            table = match line {
                "[package]" => "package",
                "[lib]" => "lib",
                "[dependencies]" => "dependencies",
                "[dev-dependencies]" => "dev-dependencies",
                _ => return Err(error("unsupported table (dependencies must remain empty)")),
            };
            if !tables.insert(table) {
                return Err(error("duplicate table"));
            }
            continue;
        }
        if matches!(table, "dependencies" | "dev-dependencies") {
            return Err(error("dependency tables must be empty"));
        }
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| error("expected a single-line assignment"))?;
        let key = key.trim();
        let value = value.trim();
        if !fields.insert((table, key)) {
            return Err(error("duplicate field"));
        }
        let valid = match (table, key) {
            ("package", "name") => value == "\"timeline\"",
            ("package", "edition") => value == "\"2021\"",
            ("package", "version") => value
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .is_some_and(|value| {
                    !value.is_empty()
                        && !value
                            .chars()
                            .any(|ch| ch.is_control() || matches!(ch, '"' | '\\'))
                }),
            ("lib", "crate-type") => value
                .strip_prefix('[')
                .and_then(|value| value.strip_suffix(']'))
                .is_some_and(|types| {
                    types
                        .split(',')
                        .map(str::trim)
                        .eq(["\"lib\"", "\"cdylib\""])
                }),
            _ => false,
        };
        if !valid {
            return Err(error("unsupported field or value"));
        }
    }

    for required in ["package", "lib", "dependencies", "dev-dependencies"] {
        if !tables.contains(required) {
            return Err(format!("missing [{required}] table"));
        }
    }
    for required in [
        ("package", "name"),
        ("package", "version"),
        ("package", "edition"),
        ("lib", "crate-type"),
    ] {
        if !fields.contains(&required) {
            return Err(format!("missing {}.{}", required.0, required.1));
        }
    }
    Ok(())
}

fn has_forbidden_path(text: &str) -> bool {
    text.contains("projects/foundation")
        || text.as_bytes().windows(3).any(|part| {
            part[0].is_ascii_alphabetic() && part[1] == b':' && matches!(part[2], b'/' | b'\\')
        })
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("cannot enumerate {}: {error}", directory.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| {
                panic!("cannot read entry in {}: {error}", directory.display())
            });
            let path = entry.path();
            let kind = entry
                .file_type()
                .unwrap_or_else(|error| panic!("cannot inspect {}: {error}", path.display()));
            assert!(!kind.is_symlink(), "symbolic link: {}", path.display());
            if kind.is_dir() {
                pending.push(path);
            } else if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs" | "py" | "h" | "md")
            ) {
                assert!(kind.is_file(), "not a regular file: {}", path.display());
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

#[test]
fn structure_no_dependencies_and_no_absolute_paths() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join("Cargo.toml");
    let metadata =
        fs::symlink_metadata(&manifest_path).expect("manifest metadata must be readable");
    assert!(metadata.is_file(), "manifest must be a regular file");
    let manifest = fs::read_to_string(&manifest_path).expect("manifest must be readable UTF-8");
    check_manifest(&manifest).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        !has_forbidden_path(&manifest),
        "forbidden path text in Cargo.toml"
    );
    println!("manifest: empty dependency tables; lib and cdylib; edition 2021");

    let files = source_files(root);
    for required in ["src/lib.rs", "README.md", "tests/structure.rs"] {
        assert!(
            files.contains(&root.join(required)),
            "source scan must include {required}"
        );
    }
    let exempt = root.join("tests/structure.rs");
    for path in &files {
        let text = fs::read_to_string(path)
            .unwrap_or_else(|error| panic!("cannot read {} as UTF-8: {error}", path.display()));
        if path == &exempt {
            continue;
        }
        assert!(
            !has_forbidden_path(&text),
            "forbidden path text in {}",
            path.display()
        );
        println!(
            "source checked: {}",
            path.strip_prefix(root)
                .expect("source stays in crate")
                .display()
        );
    }
    println!("source files read (including exempt test): {}", files.len());
}

const VALID_MANIFEST: &str = r#"[package]
name = "timeline"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["lib", "cdylib"]
[dependencies]
[dev-dependencies]
"#;

#[test]
fn manifest_check_rejects_dependency_bypasses() {
    assert!(check_manifest(VALID_MANIFEST).is_ok());
    assert!(check_manifest(&format!("# comment\r\n{VALID_MANIFEST}")).is_ok());
    assert!(check_manifest(&VALID_MANIFEST.replace('\n', "\r\n")).is_ok());

    for (label, suffix) in [
        ("direct dependency", "other = \"1\""),
        ("inline dependency", "other = { path = \"../other\" }"),
        ("quoted key", "\"other\" = \"1\""),
        ("dotted key", "other.version = \"1\""),
        (
            "dependency subtable",
            "[dependencies.other]\nversion = \"1\"",
        ),
        ("build dependency", "[build-dependencies]\nother = \"1\""),
        (
            "target dependency",
            "[target.'cfg(windows)'.dependencies]\nother = \"1\"",
        ),
        ("quoted table", "[\"dependencies\"]\nother = \"1\""),
        ("workspace", "[workspace]"),
        ("duplicate table", "[dependencies]"),
    ] {
        assert!(
            check_manifest(&format!("{VALID_MANIFEST}{suffix}\n")).is_err(),
            "manifest guard accepted {label}"
        );
    }
    let populated_dependencies =
        VALID_MANIFEST.replace("[dependencies]", "[dependencies]\nother = \"1\"");
    assert!(check_manifest(&populated_dependencies).is_err());
    for invalid in [
        String::new(),
        VALID_MANIFEST.replace("[dependencies]\n", ""),
        VALID_MANIFEST.replace("[dev-dependencies]\n", ""),
        VALID_MANIFEST.replace(
            "version = \"0.1.0\"",
            "version = \"0.1.0\"\nversion = \"1\"",
        ),
        VALID_MANIFEST.replace("version = \"0.1.0\"", "version = \"\"\"\ntext\n\"\"\""),
        VALID_MANIFEST.replace("version = \"0.1.0\"", "version = \"text#not a comment\""),
        VALID_MANIFEST.replace("\"cdylib\"", "\"cdy lib\""),
        format!("dependencies = {{ other = \"1\" }}\n{VALID_MANIFEST}"),
        format!("dependencies.other = \"1\"\n{VALID_MANIFEST}"),
    ] {
        assert!(check_manifest(&invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn path_check_rejects_drive_paths_and_legacy_reference() {
    for forbidden in [
        "C:/source",
        r"C:\source",
        "z:/",
        r"z:\",
        "prefix a:/suffix",
        "prefix\nD:/source\r\nsuffix",
        "projects/foundation",
        "prefix projects/foundation/suffix",
    ] {
        assert!(has_forbidden_path(forbidden), "accepted {forbidden:?}");
    }
    for allowed in [
        "",
        "src/time.rs",
        "kind: String",
        "1:/",
        "C:relative",
        "\u{03bb}:/",
    ] {
        assert!(!has_forbidden_path(allowed), "rejected {allowed:?}");
    }
}

#[test]
fn checks_do_not_ignore_large_prefixes() {
    let prefix = "# harmless comment\n".repeat(65_536);
    assert!(check_manifest(&format!("{prefix}{VALID_MANIFEST}")).is_ok());
    assert!(check_manifest(&format!("{prefix}{VALID_MANIFEST}other = \"1\"\n")).is_err());
    assert!(has_forbidden_path(&format!("{prefix}Q:/forbidden")));
    assert!(has_forbidden_path(&format!("{prefix}projects/foundation")));
}

#[test]
fn timeline_data_clones_independently_and_compares_by_value() {
    use timeline::time::{Rate, Time, TimeError, TICKS_PER_SECOND};
    use timeline::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};

    assert_eq!(TICKS_PER_SECOND, 254_016_000_000);
    let rate = Rate { num: 24, den: 1 };
    let copied_rate = rate;
    assert_eq!(rate, copied_rate);
    assert_ne!(TimeError::Overflow, TimeError::BadRate);
    assert_ne!(Interp::Step, Interp::Linear);
    assert_ne!(Interp::Bezier, Interp::Hermite);

    let original = Timeline {
        tracks: vec![Track {
            id: u64::MAX,
            kind: "animation".into(),
            name: "Track".into(),
            clips: vec![Clip {
                start: Time(i64::MIN),
                len: Time(i64::MAX),
                source: "source-id".into(),
                source_offset: Time(0),
                name: "Clip".into(),
            }],
            channels: vec![Channel {
                target: "position.x".into(),
                keys: vec![Keyframe {
                    time: Time(-1),
                    value: 1.0,
                    interp: Interp::Bezier,
                    handles: Some([(-1.0, 0.0), (1.0, 0.0)]),
                }],
            }],
        }],
    };
    let mut changed = original.clone();
    assert_eq!(changed, original);
    changed.tracks[0].channels[0].keys[0].value = 2.0;
    changed.tracks[0].clips[0].name.push_str(" edited");
    assert_ne!(changed, original);
    assert_eq!(original.tracks[0].channels[0].keys[0].value, 1.0);
    assert_eq!(original.tracks[0].clips[0].name, "Clip");
}
