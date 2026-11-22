const MANIFEST: &str = include_str!("../Cargo.toml");

// A closed allowlist avoids accepting unrecognized TOML forms as dependency-free
// without introducing a parser dependency into this skeleton.
fn matches_dependency_free_scaffold(manifest: &str) -> bool {
    manifest
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .eq([
            "[package]",
            "name = \"brush-engine\"",
            "version = \"0.1.0\"",
            "edition = \"2021\"",
            "[dependencies]",
        ])
}

#[test]
fn cargo_toml_has_no_dependencies() {
    assert!(
        matches_dependency_free_scaffold(MANIFEST),
        "Cargo.toml must match the dependency-free scaffold: only the package \
         metadata and an empty [dependencies] table are allowed"
    );
}

#[test]
fn dependency_line_mutation_is_rejected() {
    assert!(
        matches_dependency_free_scaffold(MANIFEST),
        "the unmodified manifest must be accepted before testing mutations"
    );
    let mutated = MANIFEST.replace("[dependencies]", "[dependencies]\nunexpected = \"0\"");
    assert_ne!(mutated, MANIFEST, "mutation must change the manifest");
    assert!(
        !matches_dependency_free_scaffold(&mutated),
        "adding one dependency line must fail the dependency guard"
    );
    assert!(
        matches_dependency_free_scaffold(MANIFEST),
        "the original manifest must remain unchanged"
    );
}

#[test]
fn alternate_dependency_forms_are_rejected() {
    assert!(
        matches_dependency_free_scaffold(MANIFEST),
        "the unmodified manifest must be accepted before testing mutations"
    );
    for addition in [
        "[dev-dependencies]\nunexpected = \"0\"",
        "[build-dependencies]\nunexpected = \"0\"",
        "[target.'cfg(windows)'.dependencies]\nunexpected = \"0\"",
        "[dependencies.unexpected]\nversion = \"0\"",
        "[workspace.dependencies]\nunexpected = \"0\"",
        "[workspace]",
        "[[dependencies.unexpected]]\nversion = \"0\"",
    ] {
        let mutated = format!("{MANIFEST}\n{addition}\n");
        assert!(
            !matches_dependency_free_scaffold(&mutated),
            "unexpected table must fail the dependency guard: {addition}"
        );
    }
    for declaration in [
        "unexpected = { version = \"0\" }",
        "unexpected = { path = \"../other\" }",
        "unexpected = { workspace = true }",
        "\"unexpected\" = \"0\" # dependency with a trailing comment",
    ] {
        let replacement = format!("[dependencies]\n{declaration}");
        let mutated = MANIFEST.replace("[dependencies]", &replacement);
        assert!(
            !matches_dependency_free_scaffold(&mutated),
            "unexpected declaration must fail the dependency guard: {declaration}"
        );
    }
}

#[test]
fn empty_or_duplicate_scaffolds_are_rejected() {
    for manifest in [
        "",
        "# comments alone do not prove dependency absence\n",
        "[dependencies]\n",
    ] {
        assert!(
            !matches_dependency_free_scaffold(manifest),
            "an incomplete manifest must not pass: {manifest:?}"
        );
    }
    let duplicate = format!("{MANIFEST}\n[dependencies]\n");
    assert!(
        !matches_dependency_free_scaffold(&duplicate),
        "duplicate tables must not pass"
    );
}
