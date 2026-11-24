use std::collections::BTreeMap;

fn spaces(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn c_type(rust: &str) -> &'static str {
    match rust {
        "*const u8" => "const uint8_t *",
        "*mut u8" => "uint8_t *",
        "usize" => "size_t",
        "u64" => "uint64_t",
        "*mut u64" => "uint64_t *",
        "*mut u32" => "uint32_t *",
        "*mut usize" => "size_t *",
        other => panic!("unmapped Rust ABI type: {other}"),
    }
}

fn rust_exports(source: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<_> = source.lines().collect();
    let mut exports = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((_, signature)) = line.split_once("extern \"C\" fn ") else {
            continue;
        };
        assert!(index > 0);
        assert_eq!(lines[index - 1].trim(), "#[no_mangle]");
        assert!(line.starts_with("pub unsafe "), "non-public/unsafe export");
        let (name, signature) = signature.split_once('(').expect("parameter list");
        let (params, result) = signature.split_once(')').expect("one-line signature");
        assert_eq!(result.trim(), "-> i32 {");
        let params = params
            .split(',')
            .filter(|param| !param.trim().is_empty())
            .map(|param| {
                let (_, ty) = param.split_once(':').expect("named Rust parameter");
                c_type(&spaces(ty)).to_string()
            })
            .collect();
        exports.push((name.to_string(), params));
    }
    exports
}

fn header_exports(header: &str) -> Vec<(String, Vec<String>)> {
    header
        .lines()
        .filter_map(|line| {
            let signature = line.strip_prefix("int32_t ")?;
            let signature = signature.strip_suffix(");").expect("one-line prototype");
            let (name, params) = signature.split_once('(').expect("C parameters");
            let params = if params == "void" {
                Vec::new()
            } else {
                params
                    .split(',')
                    .map(|param| {
                        let param = param.trim();
                        let start = param
                            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                            .expect("named C parameter")
                            + 1;
                        assert!(!param[start..].is_empty(), "missing C parameter name");
                        spaces(param[..start].trim())
                    })
                    .collect()
            };
            Some((name.to_string(), params))
        })
        .collect()
}

fn rust_codes(source: &str) -> BTreeMap<String, i32> {
    let mut codes = BTreeMap::new();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("pub const REFLECT_") {
            let (name, rest) = rest.split_once(": ").expect("typed constant");
            let (ty, value) = rest.split_once(" = ").expect("constant value");
            assert_eq!(ty, if name == "ABI_VERSION" { "u32" } else { "i32" });
            let value = value
                .strip_suffix(';')
                .expect("constant terminator")
                .parse()
                .expect("integer constant");
            assert_eq!(codes.insert(format!("REFLECT_{name}"), value), None);
        }
    }
    codes
}

fn header_codes(header: &str) -> BTreeMap<String, i32> {
    let mut codes = BTreeMap::new();
    for line in header.lines() {
        if let Some(rest) = line.strip_prefix("#define REFLECT_") {
            let (name, value) = rest.split_once(' ').expect("integer macro");
            let value = value.parse().expect("integer macro value");
            assert_eq!(codes.insert(format!("REFLECT_{name}"), value), None);
        }
    }
    codes
}

#[test]
fn header_matches_exports() {
    let header = include_str!("../include/reflect.h");
    let rust = include_str!("../src/abi.rs");
    let exports = rust_exports(rust);
    assert_eq!(exports.len(), 11);
    assert_eq!(exports, header_exports(header));
    let codes = rust_codes(rust);
    assert_eq!(codes.len(), 16);
    assert_eq!(codes, header_codes(header));
    assert!(!header.contains("struct"));
    assert!(!rust.contains("#[repr(C)]"));

    // Behavioural controls: both parsers must expose drift, not ignore it.
    let extra = "\n#[no_mangle]\npub unsafe extern \"C\" fn reflect_debug_count() -> i32 {\n}\n";
    assert_ne!(
        rust_exports(&format!("{rust}{extra}")),
        header_exports(header)
    );
    let removed = "int32_t reflect_destroy(uint64_t handle);\n";
    assert_eq!(header.matches(removed).count(), 1);
    let missing = header.replace(removed, "");
    assert_ne!(exports, header_exports(&missing));
}
