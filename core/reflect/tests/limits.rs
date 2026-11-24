use reflect::limits::{Limits, LimitsError};

const VALID: &str = concat!(
    "[limits]\n",
    "max_path_length = 256\n",
    "max_text_file_bytes = 1048576\n",
    "max_list_length = 4096\n",
    "max_nesting_depth = 16\n",
);

#[test]
fn limits_read_from_reflect_toml() {
    let text =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/reflect.toml")).unwrap();
    let expected = Ok(Limits {
        max_path_length: 256,
        max_text_file_bytes: 1048576,
        max_list_length: 4096,
        max_nesting_depth: 16,
    });
    assert_eq!(Limits::parse(&text), expected);
    assert_eq!(Limits::parse(&text.replace('\n', "\r\n")), expected);
    assert_eq!(Limits::parse(&VALID.replace("4096", "4096 # cap")), expected);
    assert_eq!(
        Limits::parse(&format!("# comment\n\t\n{}", VALID.replace(" = ", "\t=\t"))),
        expected
    );
    let largest = VALID
        .replace("256", &usize::MAX.to_string())
        .replace("1048576", &u64::MAX.to_string());
    let limits = Limits::parse(&largest).unwrap();
    assert_eq!(limits.max_path_length, usize::MAX);
    assert_eq!(limits.max_text_file_bytes, u64::MAX);
}

#[test]
fn limits_refuse_bad_config() {
    use LimitsError::*;
    let check = |text: &str, error| {
        assert_eq!(Limits::parse(text), Err(error), "config: {text:?}");
    };
    // With assignments but no table, KeyOutsideTable wins in line order.
    check("# no table\n \t\n", MissingTable);
    check(
        VALID.strip_prefix("[limits]\n").unwrap(),
        KeyOutsideTable { line: 1 },
    );
    for (key, assignment) in [
        ("max_path_length", "max_path_length = 256\n"),
        ("max_text_file_bytes", "max_text_file_bytes = 1048576\n"),
        ("max_list_length", "max_list_length = 4096\n"),
        ("max_nesting_depth", "max_nesting_depth = 16\n"),
    ] {
        check(&VALID.replace(assignment, ""), MissingKey { key });
    }
    check(
        &VALID.replace("max_nesting_depth = 16", "max_depth = 3"),
        UnknownKey {
            line: 5,
            key: "max_depth".into(),
        },
    );
    check(
        &format!("{VALID}max_list_length = 8\n"),
        DuplicateKey {
            line: 6,
            key: "max_list_length".into(),
        },
    );
    for table in ["[other]", "[limits] # comment"] {
        check(&VALID.replace("[limits]", table), UnknownTable { line: 1 });
    }
    check(&format!("{VALID}[limits]\n"), UnknownTable { line: 6 });
    check(
        &format!("max_path_length = 256\n{VALID}"),
        KeyOutsideTable { line: 1 },
    );
    for value in [
        "-1",
        "+5",
        "1_000",
        "\"256\"",
        "2.5",
        "016",
        "99999999999999999999999",
        "",
        "NaN",
        "inf",
        "２５６",
        "1 2",
        "1\r2",
        "1\u{b}",
        "1=2",
    ] {
        check(
            &VALID.replace("256", value),
            BadValue {
                line: 2,
                key: "max_path_length".into(),
            },
        );
    }
    check(
        &VALID.replace("1048576", "18446744073709551616"),
        BadValue {
            line: 3,
            key: "max_text_file_bytes".into(),
        },
    );
    if usize::BITS < u64::BITS {
        check(
            &VALID.replace("256", &u64::MAX.to_string()),
            BadValue {
                line: 2,
                key: "max_path_length".into(),
            },
        );
    }
    check(
        &VALID.replace("256", "0"),
        Zero {
            line: 2,
            key: "max_path_length".into(),
        },
    );
    for assignment in ["max_path_length 256", " = 5"] {
        check(
            &VALID.replace("max_path_length = 256", assignment),
            BadLine { line: 2 },
        );
    }
    check(
        &VALID.replace("256", "-1").replace("4096", "0"),
        BadValue {
            line: 2,
            key: "max_path_length".into(),
        },
    );
    check(
        &format!("{VALID}max_list_length = -1\n"),
        DuplicateKey {
            line: 6,
            key: "max_list_length".into(),
        },
    );
    check(
        &VALID.replace("256", &"9".repeat(100_000)),
        BadValue {
            line: 2,
            key: "max_path_length".into(),
        },
    );
    check(
        &VALID.replace("256", "\"256 # not a string\""),
        BadValue {
            line: 2,
            key: "max_path_length".into(),
        },
    );
    check(
        "[limits]\n",
        MissingKey {
            key: "max_path_length",
        },
    );
}
