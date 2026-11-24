mod support;

use reflect::desc::{FieldDesc, Kind, TypeDesc};
use reflect::limits::Limits;
use reflect::schema::{hash_hex, schema_hash, schema_text};
use reflect::text::{load_text, save_text, LoadError, Note, SaveError};
use reflect::value::Value;
use support::{random_value, XorShift64};

const L: Limits = Limits {
    max_path_length: 256,
    max_text_file_bytes: 1048576,
    max_list_length: 4096,
    max_nesting_depth: 16,
};
const EDGE: &str = include_str!("fixtures/values_edge.txt");
const SCHEMA: &str = "\
reflect schema 1
type Edge version=1 fields=16
field flag kind=bool since=1
field small kind=i64 since=1
field big kind=u64 since=1
field neg_zero kind=f64 since=1
field max kind=f64 since=1
field min_pos kind=f64 since=1
field ratio kind=f64 since=1
field empty_text kind=text since=1
field escapes kind=text since=1
field empty_list kind=list<i64> since=1
field pair kind=array<u64;2> since=1
field inner kind=struct<Inner> since=1
field inners kind=list<struct<Inner>> since=1
field opt_none kind=option<f64> since=1
field opt_some kind=option<list<text>> since=1
field mode kind=enum<Mode:Alpha|Beta> since=1
end
type Inner version=1 fields=2
field x kind=f64 since=1
field flag kind=bool since=1
end
";
const CAR_V1: &str = "\
reflect value 1
type Car version=1 hash=0000000000000000
name = \"c\"
mass = 1200.0
colour = \"red\"
note = none
end
";

fn enumeration(name: &str, variants: &[&str]) -> Kind {
    Kind::Enum {
        name: name.into(),
        variants: variants.iter().map(|s| (*s).into()).collect(),
    }
}

fn edge() -> TypeDesc {
    let inner = Kind::Struct(Box::new(
        TypeDesc::new(
            "Inner",
            1,
            vec![
                FieldDesc::new("x", Kind::F64),
                FieldDesc::new("flag", Kind::Bool),
            ],
        )
        .unwrap(),
    ));
    TypeDesc::new(
        "Edge",
        1,
        vec![
            FieldDesc::new("flag", Kind::Bool),
            FieldDesc::new("small", Kind::I64),
            FieldDesc::new("big", Kind::U64),
            FieldDesc::new("neg_zero", Kind::F64),
            FieldDesc::new("max", Kind::F64),
            FieldDesc::new("min_pos", Kind::F64),
            FieldDesc::new("ratio", Kind::F64),
            FieldDesc::new("empty_text", Kind::Text),
            FieldDesc::new("escapes", Kind::Text),
            FieldDesc::new("empty_list", Kind::List(Box::new(Kind::I64))),
            FieldDesc::new("pair", Kind::Array(Box::new(Kind::U64), 2)),
            FieldDesc::new("inner", inner.clone()),
            FieldDesc::new("inners", Kind::List(Box::new(inner))),
            FieldDesc::new("opt_none", Kind::Option(Box::new(Kind::F64))),
            FieldDesc::new(
                "opt_some",
                Kind::Option(Box::new(Kind::List(Box::new(Kind::Text)))),
            ),
            FieldDesc::new("mode", enumeration("Mode", &["Alpha", "Beta"])),
        ],
    )
    .unwrap()
}

fn e() -> Value {
    Value::Struct(vec![
        Value::Bool(true),
        Value::I64(i64::MIN),
        Value::U64(u64::MAX),
        Value::F64(-0.0),
        Value::F64(f64::MAX),
        Value::F64(f64::MIN_POSITIVE),
        Value::F64(0.1),
        Value::Text("".into()),
        Value::Text("q\"b\\n\nr\rt\tc\u{1}é".into()),
        Value::List(vec![]),
        Value::List(vec![Value::U64(0), Value::U64(7)]),
        Value::Struct(vec![Value::F64(1.5), Value::Bool(true)]),
        Value::List(vec![Value::Struct(vec![
            Value::F64(-2.0),
            Value::Bool(false),
        ])]),
        Value::Option(None),
        Value::Option(Some(Box::new(Value::List(vec![
            Value::Text("a".into()),
            Value::Text("".into()),
        ])))),
        Value::Enum("Beta".into()),
    ])
}

fn replace_line(text: &str, line: usize, replacement: &str) -> String {
    assert!(line > 0 && line <= text.lines().count());
    let mut lines: Vec<_> = text.lines().collect();
    lines[line - 1] = replacement;
    format!("{}\n", lines.join("\n"))
}

fn error(line: usize, replacement: &str) -> LoadError {
    load_text(&edge(), &replace_line(EDGE, line, replacement), &L).unwrap_err()
}

macro_rules! rejects {
    ($line:expr, $replacement:expr, $variant:ident $(, $key:ident = $value:expr)* $(,)?) => {
        assert_eq!(error($line, $replacement), LoadError::$variant {
            line: $line, $($key: $value),*
        });
    };
}

fn changed(mut value: Value, index: usize, replacement: Value) -> Value {
    let Value::Struct(fields) = &mut value else {
        panic!("test value must be a struct");
    };
    fields[index] = replacement;
    value
}

#[test]
fn value_text_roundtrip() {
    let desc = edge();
    let kind = Kind::Struct(Box::new(desc.clone()));
    let mut rng = XorShift64::new(0x5eed_1234);
    for iteration in 0..300 {
        let value = random_value(&kind, &mut rng, 3);
        let text = save_text(&desc, &value, &L).unwrap();
        let loaded = load_text(&desc, &text, &L).unwrap();
        assert_eq!(loaded.value, value, "iteration {iteration}");
        assert_eq!(loaded.from_version, 1);
        assert!(loaded.notes.is_empty());
        assert_eq!(save_text(&desc, &loaded.value, &L).unwrap(), text);
    }
    // Bit-pattern references exercise subnormal and boundary floats without arithmetic.
    for bits in [
        1,
        0x000f_ffff_ffff_ffff,
        0x0010_0000_0000_0000,
        0x7fef_ffff_ffff_ffff,
    ] {
        let value = changed(e(), 6, Value::F64(f64::from_bits(bits)));
        let text = save_text(&desc, &value, &L).unwrap();
        let loaded = load_text(&desc, &text, &L).unwrap();
        let Value::Struct(fields) = loaded.value else {
            panic!("struct");
        };
        let Value::F64(v) = fields[6] else {
            panic!("float");
        };
        assert_eq!(v.to_bits(), bits);
    }
}

#[test]
fn text_save_is_byte_identical() {
    let desc = edge();
    assert_eq!(schema_text(&desc).unwrap(), SCHEMA);
    assert_eq!(hash_hex(schema_hash(&desc).unwrap()), "2b7a4dee687b1aba");
    assert_eq!(save_text(&desc, &e(), &L).unwrap().as_bytes(), EDGE.as_bytes());
    let loaded = load_text(&desc, EDGE, &L).unwrap();
    assert_eq!(loaded.value, e());
    let Value::Struct(fields) = &loaded.value else {
        panic!("struct");
    };
    let Value::F64(zero) = fields[3] else {
        panic!("float");
    };
    assert_eq!(zero.to_bits(), (-0.0f64).to_bits());
    assert_eq!(save_text(&desc, &loaded.value, &L).unwrap(), EDGE);
}

#[test]
fn text_old_version_defaults_renamed_ignored() {
    let mut mass = FieldDesc::new("mass_kg", Kind::F64);
    mass.renamed_from = Some("mass".into());
    let mut drive = FieldDesc::new("drive", enumeration("Drive", &["Front", "Rear"]));
    drive.since = 2;
    drive.default = Some(Value::Enum("Rear".into()));
    let desc = TypeDesc::new(
        "Car",
        2,
        vec![
            FieldDesc::new("name", Kind::Text),
            mass,
            drive,
            FieldDesc::new("note", Kind::Option(Box::new(Kind::Text))),
        ],
    )
    .unwrap();
    let loaded = load_text(&desc, CAR_V1, &L).unwrap();
    assert_eq!(
        loaded.value,
        Value::Struct(vec![
            Value::Text("c".into()),
            Value::F64(1200.0),
            Value::Enum("Rear".into()),
            Value::Option(None),
        ])
    );
    assert_eq!(loaded.from_version, 1);
    assert_eq!(
        loaded.notes,
        vec![
            Note::Renamed {
                line: 4,
                from: "mass".into(),
                to: "mass_kg".into(),
            },
            Note::Ignored {
                line: 5,
                name: "colour".into(),
            },
            Note::Defaulted {
                field: "drive".into(),
            },
        ]
    );
    assert_eq!(
        loaded.notes.iter().map(ToString::to_string).collect::<Vec<_>>(),
        [
            "field mass_kg read from old name mass (line 4)",
            "unknown field colour ignored (line 5)",
            "field drive set to its default",
        ]
    );
    assert_eq!(
        load_text(&desc, &CAR_V1.replace("name = \"c\"\n", ""), &L),
        Err(LoadError::MissingField {
            name: "name".into(),
        })
    );
    assert_eq!(
        load_text(&desc, &CAR_V1.replace("end\n", "mass_kg = 1.0\nend\n"), &L),
        Err(LoadError::DuplicateField {
            line: 7,
            name: "mass_kg".into(),
        })
    );
    let current = save_text(&desc, &loaded.value, &L).unwrap();
    assert_eq!(
        load_text(&desc, &current.replace("drive = Rear\n", ""), &L),
        Err(LoadError::MissingField {
            name: "drive".into(),
        })
    );
    // Unknown old values are deliberately opaque, but their assignment syntax is not.
    assert!(load_text(&desc, &CAR_V1.replace("\"red\"", "not[a{value"), &L).is_ok());
    assert_eq!(
        load_text(&desc, &CAR_V1.replace("colour = \"red\"", "colour = "), &L),
        Err(LoadError::BadLine { line: 5 })
    );
    let unordered = CAR_V1.replace("name = \"c\"\nmass = 1200.0", "mass = 1200.0\nname = \"c\"");
    assert_eq!(load_text(&desc, &unordered, &L).unwrap().value, loaded.value);
    assert_eq!(
        load_text(&desc, &current.replace("mass_kg = ", "mass = "), &L),
        Err(LoadError::UnknownField {
            line: 4,
            name: "mass".into(),
        })
    );
    // Defaults must not bypass the value guards.
    let mut default = FieldDesc::new("added", Kind::F64);
    default.since = 2;
    default.default = Some(Value::F64(f64::NAN));
    let defaults = TypeDesc::new("Defaults", 2, vec![default]).unwrap();
    let old = "reflect value 1\ntype Defaults version=1 hash=0000000000000000\nend\n";
    assert_eq!(
        load_text(&defaults, old, &L),
        Err(LoadError::NonFinite {
            line: 2,
            path: "added".into(),
        })
    );
}

#[test]
fn text_rejects_newer_version() {
    let mut mass = FieldDesc::new("mass_kg", Kind::F64);
    mass.since = 2;
    mass.default = Some(Value::F64(1000.0));
    let desc = TypeDesc::new("Vehicle", 2, vec![FieldDesc::new("name", Kind::Text), mass]).unwrap();
    let err = load_text(&desc, include_str!("fixtures/vehicle.v3_future.txt"), &L).unwrap_err();
    assert_eq!(
        err,
        LoadError::NewerVersion {
            line: 2,
            file: 3,
            current: 2,
        }
    );
    assert_eq!(
        err.to_string(),
        "line 2: file version 3 is newer than type version 2"
    );
    rejects!(
        2,
        "type Edge version=1 hash=0000000000000000",
        HashMismatch,
        file = "0000000000000000".into(),
        current = "2b7a4dee687b1aba".into(),
    );
    rejects!(
        2,
        "type Other version=1 hash=2b7a4dee687b1aba",
        WrongType,
        expected = "Edge".into(),
        found = "Other".into(),
    );
    for header in [
        "type Edge version=0 hash=2b7a4dee687b1aba",
        "type Edge version=01 hash=2b7a4dee687b1aba",
        "type Edge version=+1 hash=2b7a4dee687b1aba",
        "type Edge version=4294967296 hash=2b7a4dee687b1aba",
        "type Edge version=1 hash=2B7A4DEE687B1ABA",
    ] {
        assert_eq!(error(2, header), LoadError::BadHeader { line: 2 });
    }
    assert_eq!(error(1, "reflect value 2"), LoadError::BadHeader { line: 1 });
}

#[test]
fn text_rejects_nan_inf_and_bad_escapes() {
    for token in ["NaN", "inf", "-inf", "infinity", "1e9999"] {
        rejects!(9, &format!("ratio = {token}"), NonFinite, path = "ratio".into());
    }
    for (line, path, token) in [
        (9, "ratio", "+0.1"),
        (9, "ratio", "0.10"),
        (9, "ratio", "1e1"),
        (4, "small", "-0"),
        (5, "big", "-1"),
        (5, "big", "07"),
        (4, "small", "+5"),
        (4, "small", "-"),
    ] {
        let replacement = format!("{path} = {token}");
        rejects!(line, &replacement, BadNumber, path = path.into(), text = token.into());
    }
    rejects!(9, "ratio = ", BadLine);
    for (line, path, token) in [
        (4, "small", "-9223372036854775809"),
        (5, "big", "18446744073709551616"),
    ] {
        rejects!(
            line,
            &format!("{path} = {token}"),
            IntegerRange,
            path = path.into(),
            text = token.into(),
        );
    }
    for token in [
        r#""a\qb""#,
        r#""\u{41}""#,
        r#""\u{}""#,
        r#""\u{d800}""#,
        r#""\u{110000}""#,
        r#""\u{0000001}""#,
    ] {
        rejects!(11, &format!("escapes = {token}"), BadEscape, path = "escapes".into());
    }
    rejects!(11, "escapes = \"a\tb\"", RawControl, path = "escapes".into());
    for token in ["\"abc", "\"a\"b\"", "\"abc\\", "\"\\u{1"] {
        rejects!(10, &format!("empty_text = {token}"), BadValue, path = "empty_text".into());
    }
    rejects!(18, "mode = Gamma", BadVariant, path = "mode".into(), variant = "Gamma".into());
    rejects!(13, "pair = [1]", WrongLength, path = "pair".into(), len = 1, expected = 2);
    rejects!(14, "inner = {flag=true,x=1.5}", StructFields, path = "inner".into());
    rejects!(11, "escapes = \"a\rb\"", CarriageReturn);
    assert_eq!(
        save_text(&edge(), &changed(e(), 6, Value::F64(f64::NAN)), &L),
        Err(SaveError::NonFinite {
            path: "ratio".into(),
        })
    );
    let nested = Value::List(vec![Value::Struct(vec![
        Value::F64(f64::INFINITY),
        Value::Bool(false),
    ])]);
    assert_eq!(
        save_text(&edge(), &changed(e(), 12, nested), &L),
        Err(SaveError::NonFinite {
            path: "inners[0].x".into(),
        })
    );
    assert_eq!(
        save_text(&edge(), &Value::Bool(true), &L),
        Err(SaveError::WrongKind {
            path: "Edge".into(),
        })
    );
}

#[test]
fn text_limits_and_line_rules() {
    let bytes = EDGE.len() as u64;
    let small = Limits {
        max_text_file_bytes: bytes - 1,
        ..L
    };
    assert_eq!(
        load_text(&edge(), EDGE, &small),
        Err(LoadError::TooLarge {
            bytes,
            max: bytes - 1,
        })
    );
    assert_eq!(
        save_text(&edge(), &e(), &small),
        Err(SaveError::TooLarge {
            bytes,
            max: bytes - 1,
        })
    );
    let short = Limits {
        max_list_length: 1,
        ..L
    };
    assert_eq!(
        load_text(&edge(), EDGE, &short),
        Err(LoadError::TooLong {
            line: 13,
            path: "pair".into(),
            max: 1,
        })
    );
    assert_eq!(
        save_text(&edge(), &e(), &short),
        Err(SaveError::TooLong {
            path: "pair".into(),
            max: 1,
        })
    );
    let shallow = Limits {
        max_nesting_depth: 2,
        ..L
    };
    assert_eq!(
        load_text(&edge(), EDGE, &shallow),
        Err(LoadError::TooDeep {
            line: 15,
            path: "inners[0].x".into(),
            max: 2,
        })
    );
    assert_eq!(
        save_text(&edge(), &e(), &shallow),
        Err(SaveError::TooDeep {
            path: "inners[0].x".into(),
            max: 2,
        })
    );
    let mut swapped: Vec<_> = EDGE.lines().collect();
    swapped.swap(2, 3);
    let swapped = format!("{}\n", swapped.join("\n"));
    for (text, expected) in [
        (
            EDGE.replacen("flag = true\n", "flag = true\n\n", 1),
            LoadError::BadLine { line: 4 },
        ),
        (replace_line(EDGE, 3, "flag=true"), LoadError::BadLine { line: 3 }),
        (
            swapped,
            LoadError::OutOfOrder {
                line: 4,
                name: "flag".into(),
            },
        ),
        (
            EDGE.replacen("flag = true\n", "flag = true\nflag = true\n", 1),
            LoadError::DuplicateField {
                line: 4,
                name: "flag".into(),
            },
        ),
        (
            EDGE.replacen("flag = true\n", "", 1),
            LoadError::MissingField {
                name: "flag".into(),
            },
        ),
        (
            EDGE.replace("end\n", "extra = 1\nend\n"),
            LoadError::UnknownField {
                line: 19,
                name: "extra".into(),
            },
        ),
        (EDGE.replace("end\n", ""), LoadError::MissingEnd),
        (format!("{EDGE}extra\n"), LoadError::TrailingLine { line: 20 }),
        (EDGE.trim_end_matches('\n').into(), LoadError::NoFinalNewline),
    ] {
        assert_eq!(load_text(&edge(), &text, &L), Err(expected), "{text}");
    }
    // Oversize input is rejected before CR/header checks; no claimed-size allocation.
    let huge = "\r".repeat(L.max_text_file_bytes as usize + 1);
    assert_eq!(
        load_text(&edge(), &huge, &L),
        Err(LoadError::TooLarge {
            bytes: huge.len() as u64,
            max: L.max_text_file_bytes,
        })
    );
    let hostile = "[".repeat(L.max_list_length + 1);
    rejects!(
        12,
        &format!("empty_list = [{hostile}"),
        BadNumber,
        path = "empty_list[0]".into(),
        text = hostile,
    );
}
