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
    use ErrorKind::*;
    for (text, kind, offset) in [
        ("", UnexpectedEnd, 0),
        ("+1", UnexpectedToken, 0),
        (".1", UnexpectedToken, 0),
        ("-", InvalidNumber, 1),
        ("1.", InvalidNumber, 2),
        ("1e+", InvalidNumber, 3),
        ("NaN", UnexpectedToken, 0),
        ("Infinity", UnexpectedToken, 0),
        ("[1 2]", UnexpectedToken, 3),
        ("{\"a\"}", UnexpectedToken, 4),
        ("{\"a\" 1}", UnexpectedToken, 5),
        ("{\"a\":1,}", UnexpectedToken, 7),
        ("/*x*/null", UnexpectedToken, 0),
        ("tru", UnexpectedEnd, 3),
        // High surrogate not followed by a low-surrogate escape.
        ("\"\\uD800x\"", InvalidUnicode, 7),
        ("\"\\uD800\"", InvalidUnicode, 7),
        ("\"\\uD800\\u0041\"", InvalidUnicode, 13),
        // A bad hex digit is reported at the digit itself.
        ("\"\\u12xz\"", InvalidUnicode, 5),
        ("\"\\", UnexpectedEnd, 2),
        ("\"unterminated", UnexpectedEnd, 13),
        ("\u{a0}null", UnexpectedToken, 0),
    ] {
        let error = read(text).expect_err(text);
        assert_eq!((error.kind, error.offset), (kind, offset), "{text:?}");
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

#[test]
fn deep_nesting_is_an_error_not_a_crash() {
    let text = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let err = read(&text).unwrap_err();
    assert_eq!(err.kind, ErrorKind::TooDeep);
    assert_eq!(err.offset, scene::json::MAX_DEPTH);
    let object_text = format!("{}1{}", "{\"a\":".repeat(100_000), "}".repeat(100_000));
    assert_eq!(read(&object_text).unwrap_err().kind, ErrorKind::TooDeep);
    let limit = scene::json::MAX_DEPTH;
    let ok = format!("{}{}", "[".repeat(limit), "]".repeat(limit));
    assert!(read(&ok).is_ok());
}

#[test]
fn writer_fixed_document_round_trip() {
    let input = r#" { "z": [null,true,false,-2.5e1,{},[]], "a": "\"\\\/\b\f\n\r\t\u0000\u001f\u0041\uD834\uDD1E", "z": 0 } "#;
    let expected = concat!(
        r#"{"z":[null,true,false,-25,{},[]],"a":"\"\\/\b\f\n\r\t\u0000\u001fA"#,
        "\u{1d11e}",
        r#"","z":0}"#
    );
    let value = read(input).unwrap();
    let output = scene::json::write(&value).unwrap();
    assert_eq!(output, expected);
    assert_eq!(read(&output).unwrap(), value);
    assert_eq!(scene::json::write(&value).unwrap(), output);
}

#[test]
fn writer_escapes_keys_and_all_control_characters() {
    let controls: String = (0..=31).map(char::from).collect();
    let value = Value::Object(vec![(
        "\"\\\n".into(),
        Value::String(format!("{controls}/\u{e9}\u{1d11e}")),
    )]);
    let expected = concat!(
        r#"{"\"\\\n":"\u0000\u0001\u0002\u0003\u0004\u0005\u0006\u0007"#,
        r#"\b\t\n\u000b\f\r\u000e\u000f"#,
        r#"\u0010\u0011\u0012\u0013\u0014\u0015\u0016\u0017"#,
        r#"\u0018\u0019\u001a\u001b\u001c\u001d\u001e\u001f/"#,
        "\u{e9}\u{1d11e}",
        r#""}"#
    );
    let output = scene::json::write(&value).unwrap();
    assert_eq!(output, expected);
    assert_eq!(read(&output).unwrap(), value);
}

#[test]
fn writer_numbers_preserve_finite_bits_and_reject_nonfinite() {
    for number in [
        0.0,
        -0.0,
        f64::from_bits(1),
        -f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        f64::MIN,
        1.0000000000000002,
    ] {
        let output = scene::json::write(&Value::Number(number)).unwrap();
        let Value::Number(actual) = read(&output).unwrap() else {
            panic!("number expected");
        };
        assert_eq!(actual.to_bits(), number.to_bits());
    }
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (value, offset) in [
            (Value::Number(number), 0),
            (
                Value::Object(vec![("\u{e9}".into(), Value::Number(number))]),
                6,
            ),
        ] {
            let error = scene::json::write(&value).unwrap_err();
            assert_eq!(error.kind, ErrorKind::NonFiniteNumber);
            assert_eq!(error.offset, offset);
        }
    }
}

#[test]
fn writer_depth_boundary_and_large_array() {
    let mut value = Value::Null;
    for _ in 0..scene::json::MAX_DEPTH {
        value = Value::Array(vec![value]);
    }
    let output = scene::json::write(&value).unwrap();
    assert_eq!(read(&output).unwrap(), value);
    value = Value::Array(vec![value]);
    let error = scene::json::write(&value).unwrap_err();
    assert_eq!(error.kind, ErrorKind::TooDeep);
    assert_eq!(error.offset, scene::json::MAX_DEPTH);

    let value = Value::Array(vec![Value::Null; 10001]);
    let expected = format!("[{}null]", "null,".repeat(10000));
    assert_eq!(scene::json::write(&value).unwrap(), expected);
}
