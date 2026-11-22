use scene::json::{read, ErrorKind, Value};

#[test]
fn fixed_document_and_escapes() {
    assert_eq!(
        read(r#"{"z":[null,true,false,-2.5e1],"a":"\"\\\/\b\f\n\r\t\u0041\uD834\uDD1Eé","z":0}"#),
        Ok(Value::Object(vec![
            (
                "z".into(),
                Value::Array(vec![
                    Value::Null,
                    Value::Bool(true),
                    Value::Bool(false),
                    Value::Number(-25.0)
                ])
            ),
            ("a".into(), Value::String("\"\\/\u{8}\u{c}\n\r\tA𝄞é".into())),
            ("z".into(), Value::Number(0.0)),
        ]))
    );
}

#[test]
fn every_error_kind_and_byte_offset() {
    for (text, kind, offset) in [
        ("[", ErrorKind::UnexpectedEnd, 1),
        ("[1,]", ErrorKind::UnexpectedToken, 3),
        (r#""\q""#, ErrorKind::InvalidEscape, 2),
        (r#""\uDC00""#, ErrorKind::InvalidUnicode, 7),
        ("\"é\n\"", ErrorKind::ControlCharacter, 3),
        ("01", ErrorKind::InvalidNumber, 1),
        ("1e309", ErrorKind::NonFiniteNumber, 5),
        ("null x", ErrorKind::TrailingCharacters, 5),
    ] {
        let error = read(text).expect_err(text);
        assert_eq!((error.kind, error.offset), (kind, offset), "{text}");
        assert!(error.to_string().contains(&format!("byte {offset}")));
    }
}

#[test]
fn malformed_syntax_is_rejected() {
    for text in [
        "",
        "+1",
        ".1",
        "-",
        "1.",
        "1e+",
        "NaN",
        "Infinity",
        "[1 2]",
        "{\"a\" 1}",
        "{\"a\":1,}",
        "/*x*/null",
        "tru",
        "\"\\uD800x\"",
        "\"\\u12xz\"",
        "\"\\",
        "\"unterminated",
        "\u{a0}null",
    ] {
        assert!(read(text).is_err(), "{text:?}");
    }
}

#[test]
fn finite_extremes_and_large_input() {
    for (text, expected) in [
        ("1.7976931348623157e308", f64::MAX),
        ("5e-324", f64::from_bits(1)),
        ("1e-9999", 0.0),
        ("-0", -0.0),
    ] {
        let Value::Number(value) = read(text).unwrap() else {
            panic!("number expected")
        };
        assert_eq!(value.to_bits(), expected.to_bits());
    }
    let text = format!("[{}null]", "null,".repeat(10000));
    let Value::Array(items) = read(&text).unwrap() else {
        panic!("array expected")
    };
    assert_eq!(items.len(), 10001);
}
