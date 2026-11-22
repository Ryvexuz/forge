//! Checks the public, documentation-only module skeleton.

pub use reflect::{
    abi, change, desc, inspector, limits, macros, path, registry, sample, schema, script, text,
    value, watch,
};

#[test]
fn lib_exports_all_modules() {
    let library = include_str!("../src/lib.rs");
    let modules = [
        ("abi", include_str!("../src/abi.rs")),
        ("change", include_str!("../src/change.rs")),
        ("desc", include_str!("../src/desc.rs")),
        ("inspector", include_str!("../src/inspector.rs")),
        ("limits", include_str!("../src/limits.rs")),
        ("macros", include_str!("../src/macros.rs")),
        ("path", include_str!("../src/path.rs")),
        ("registry", include_str!("../src/registry.rs")),
        ("sample", include_str!("../src/sample.rs")),
        ("schema", include_str!("../src/schema.rs")),
        ("script", include_str!("../src/script.rs")),
        ("text", include_str!("../src/text.rs")),
        ("value", include_str!("../src/value.rs")),
        ("watch", include_str!("../src/watch.rs")),
    ];

    for (name, source) in modules {
        let declaration = format!("pub mod {name};");
        assert!(
            library.lines().any(|line| line == declaration),
            "missing public file module declaration: {name}"
        );
        assert!(
            source
                .lines()
                .any(|line| line.starts_with("//!") && !line[3..].trim().is_empty()),
            "module has no documentation: {name}"
        );
        assert!(
            source
                .lines()
                .all(|line| line.trim().is_empty() || line.starts_with("//!")),
            "module is not a documentation-only stub: {name}"
        );
    }
}
