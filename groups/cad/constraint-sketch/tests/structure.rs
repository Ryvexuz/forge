use std::collections::HashSet;

// This is a conservative checker for the skeleton's manifest subset, not a
// general TOML parser. Unsupported syntax must fail, never hide dependencies.
fn check_dependency_free_manifest(manifest: &str) -> Result<(), String> {
    let mut section = "";
    let mut sections = HashSet::new();
    let mut package_keys = HashSet::new();

    for (index, line) in manifest.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let error = |reason| format!("manifest line {}: {reason}", index + 1);
        if line.starts_with('[') {
            if !matches!(
                line,
                "[package]" | "[dependencies]" | "[dev-dependencies]" | "[build-dependencies]"
            ) {
                return Err(error("unsupported table"));
            }
            if !sections.insert(line) {
                return Err(error("duplicate table"));
            }
            section = line;
            continue;
        }
        if section != "[package]" {
            return Err(error("dependency entry or entry outside package table"));
        }

        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| error("expected package field"))?;
        let key = key.trim();
        if key.is_empty()
            || !key
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        {
            return Err(error("unsupported package key"));
        }
        if !package_keys.insert(key) {
            return Err(error("duplicate package key"));
        }
        let value = value
            .trim()
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or_else(|| error("expected single-line string"))?;
        if value.contains(['"', '\\']) || value.chars().any(char::is_control) {
            return Err(error("unsupported string syntax"));
        }
    }

    if !sections.contains("[package]") || package_keys.is_empty() {
        return Err("manifest must contain a nonempty package table".to_owned());
    }
    Ok(())
}

#[test]
fn cargo_toml_has_no_dependencies() {
    check_dependency_free_manifest(include_str!("../Cargo.toml"))
        .expect("crate manifest must have no normal, development, or build dependencies");
}

#[test]
fn dependency_checker_accepts_empty_or_absent_tables() {
    let package = "[package]\r\nname = \"example\"\r\n";
    for tables in [
        "",
        "[dependencies]\n",
        "[dev-dependencies]\n",
        "[build-dependencies]\n",
        "[dependencies]\n# empty\n[dev-dependencies]\n[build-dependencies]\n",
    ] {
        let manifest = format!("{package}{tables}");
        assert_eq!(check_dependency_free_manifest(&manifest), Ok(()));
    }
}

#[test]
fn dependency_checker_rejects_entries_and_unsupported_syntax() {
    let package = "[package]\nname = \"example\"\n";
    for entry in [
        "[dependencies]\nother = \"1\"\n",
        "[dev-dependencies]\nother = { path = \"../other\" }\n",
        "[build-dependencies]\nother = { git = \"https://example.invalid/other\" }\n",
        "[dependencies.other]\nversion = \"1\"\n",
        "[target.'cfg(windows)'.dependencies]\nother = \"1\"\n",
        "[\"dependencies\"]\nother = \"1\"\n",
        "dependencies.other = \"1\"\n",
        "[workspace]\n",
        "[dependencies]\n[dependencies]\n",
        "name = \"duplicate\"\n",
        "description = \"\"\"\n[dependencies]\n\"\"\"\n",
        "description = \"# not a comment\"\n",
        "description = \"windows\\\\path\"\n",
        "description = \"embedded\rreturn\"\n",
    ] {
        let manifest = format!("{package}{entry}");
        assert!(
            check_dependency_free_manifest(&manifest).is_err(),
            "checker must reject {entry:?}"
        );
    }
    for manifest in ["", "# only a comment\n", "[dependencies]\n"] {
        assert!(
            check_dependency_free_manifest(manifest).is_err(),
            "checker must reject a manifest without package fields"
        );
    }
}
