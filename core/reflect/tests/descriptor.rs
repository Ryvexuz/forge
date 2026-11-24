mod support;

use reflect::desc::{DescError, FieldDesc, Kind, Reflect, SetError, TypeDesc};
use reflect::value::Value;
use support::{Tiny, XorShift64};

fn enumeration() -> Kind {
    Kind::Enum {
        name: "Switch".into(),
        variants: vec!["On".into(), "Off".into()],
    }
}

fn every_kind() -> TypeDesc {
    let kinds = vec![
        Kind::Bool,
        Kind::I64,
        Kind::U64,
        Kind::F64,
        Kind::Text,
        Kind::List(Box::new(Kind::I64)),
        Kind::Array(Box::new(Kind::Bool), 2),
        Kind::Struct(Box::new(Tiny::desc())),
        Kind::Option(Box::new(Kind::Text)),
        enumeration(),
    ];
    let mut fields: Vec<_> = kinds
        .into_iter()
        .enumerate()
        .map(|(i, kind)| FieldDesc::new(&format!("f{i}"), kind))
        .collect();
    fields[0].since = 2;
    TypeDesc::new("AllKinds", 2, fields).unwrap()
}

#[test]
fn value_kind_names() {
    let values = [
        Value::Bool(true),
        Value::I64(1),
        Value::U64(1),
        Value::F64(1.0),
        Value::Text(String::new()),
        Value::List(vec![]),
        Value::Struct(vec![]),
        Value::Option(None),
        Value::Enum("On".into()),
    ];
    assert_eq!(
        values.map(|v| v.kind_name()),
        ["bool", "i64", "u64", "f64", "text", "list", "struct", "option", "enum"]
    );
    assert_eq!(
        every_kind()
            .fields()
            .iter()
            .map(|f| f.kind.name())
            .collect::<Vec<_>>(),
        ["bool", "i64", "u64", "f64", "text", "list", "array", "struct", "option", "enum"]
    );
}

#[test]
fn kind_accepts_matching_values_only() {
    let scalars = [
        (Kind::Bool, Value::Bool(true)),
        (Kind::I64, Value::I64(1)),
        (Kind::U64, Value::U64(1)),
        (Kind::F64, Value::F64(1.0)),
        (Kind::Text, Value::Text("On".into())),
    ];
    for (i, (kind, _)) in scalars.iter().enumerate() {
        for (j, (_, value)) in scalars.iter().enumerate() {
            assert_eq!(kind.accepts(value), i == j, "{kind:?}, {value:?}");
        }
    }
    let list = Kind::List(Box::new(Kind::I64));
    assert!(list.accepts(&Value::List(vec![])));
    assert!(list.accepts(&Value::List(vec![Value::I64(1)])));
    assert!(!list.accepts(&Value::List(vec![Value::U64(1)])));
    let array = Kind::Array(Box::new(Kind::Bool), 2);
    for len in 1..=3 {
        assert_eq!(
            array.accepts(&Value::List(vec![Value::Bool(true); len])),
            len == 2
        );
    }
    assert!(!array.accepts(&Value::List(vec![Value::Bool(true), Value::I64(1)])));
    let option = Kind::Option(Box::new(Kind::Text));
    assert!(option.accepts(&Value::Option(None)));
    assert!(option.accepts(&Value::Option(Some(Box::new(Value::Text("x".into()))))));
    assert!(!option.accepts(&Value::Option(Some(Box::new(Value::Bool(true))))));
    assert!(enumeration().accepts(&Value::Enum("On".into())));
    assert!(!enumeration().accepts(&Value::Enum("Other".into())));
    assert!(!enumeration().accepts(&Value::Text("On".into())));
    let nested = Kind::Struct(Box::new(Tiny::desc()));
    assert!(nested.accepts(&Value::Struct(vec![
        Value::Text("x".into()),
        Value::F64(1.0),
        Value::U64(1),
    ])));
    assert!(!nested.accepts(&Value::Struct(vec![Value::Text("x".into()), Value::F64(1.0)])));
    assert!(!nested.accepts(&Value::Struct(vec![
        Value::Text("x".into()),
        Value::I64(1),
        Value::U64(1),
    ])));
    // Cross-shape mismatches must not be accepted by container kinds either.
    for kind in every_kind().fields().iter().skip(5).map(|f| &f.kind) {
        assert!(!kind.accepts(&Value::Bool(true)));
    }
}

#[test]
fn type_desc_refuses_bad_declarations() {
    use DescError::*;
    let field = || FieldDesc::new("a", Kind::F64);
    let bad_field = |f: FieldDesc| TypeDesc::new("Test", 2, vec![f]).unwrap_err();
    for name in ["", "9x", "é", "a\\b", "C:a", "a\r"] {
        assert_eq!(
            TypeDesc::new(name, 1, vec![]),
            Err(BadName { name: name.into() })
        );
    }
    assert_eq!(
        bad_field(FieldDesc::new("a-b", Kind::Bool)),
        BadName { name: "a-b".into() }
    );
    assert_eq!(
        TypeDesc::new("Test", 1, vec![field(), field()]),
        Err(DuplicateField { name: "a".into() })
    );
    let duplicate = Kind::Enum {
        name: "Switch".into(),
        variants: vec!["On".into(), "On".into()],
    };
    assert_eq!(
        bad_field(FieldDesc::new("a", duplicate)),
        DuplicateVariant {
            enum_name: "Switch".into(),
            variant: "On".into()
        }
    );
    for kind in [
        Kind::Enum {
            name: "Switch".into(),
            variants: vec![],
        },
        Kind::Array(Box::new(Kind::Bool), 0),
    ] {
        assert_eq!(
            bad_field(FieldDesc::new("a", kind)),
            Empty { field: "a".into() }
        );
    }
    for kind in [
        Kind::List(Box::new(Kind::Enum {
            name: "bad-name".into(),
            variants: vec!["On".into()],
        })),
        Kind::Option(Box::new(Kind::Enum {
            name: "Switch".into(),
            variants: vec!["bad-name".into()],
        })),
    ] {
        assert_eq!(
            bad_field(FieldDesc::new("a", kind)),
            BadName {
                name: "bad-name".into()
            }
        );
    }
    assert_eq!(
        TypeDesc::new("Test", 0, vec![]),
        Err(BadVersion {
            field: None,
            version: 0
        })
    );
    for version in [0, 3] {
        let mut f = field();
        f.since = version;
        assert_eq!(
            bad_field(f),
            BadVersion {
                field: Some("a".into()),
                version
            }
        );
    }
    for (min, max) in [
        (Some(f64::NAN), None),
        (Some(5.0), Some(1.0)),
        (Some(f64::NEG_INFINITY), None),
        (None, Some(f64::INFINITY)),
        (None, Some(f64::NAN)),
    ] {
        let mut f = field();
        f.min = min;
        f.max = max;
        assert_eq!(bad_field(f), BadRange { field: "a".into() });
    }
    let mut f = FieldDesc::new("a", Kind::Text);
    f.min = Some(0.0);
    assert_eq!(bad_field(f), BadRange { field: "a".into() });
    let mut f = field();
    f.default = Some(Value::Text("x".into()));
    assert_eq!(bad_field(f), BadDefault { field: "a".into() });
    let mut f = field();
    f.renamed_from = Some("a".into());
    assert_eq!(bad_field(f), BadRename { field: "a".into() });
    let mut f = field();
    f.renamed_from = Some("b".into());
    assert_eq!(
        TypeDesc::new("Test", 2, vec![f, FieldDesc::new("b", Kind::Bool)]),
        Err(BadRename { field: "a".into() })
    );
    let mut f = field();
    f.renamed_from = Some("bad-name".into());
    assert_eq!(
        bad_field(f),
        BadName {
            name: "bad-name".into()
        }
    );
    for text in ["a\nb", "", "a\rb", "a\u{7f}b"] {
        let mut f = field();
        f.label = Some(text.into());
        assert_eq!(bad_field(f), BadText { field: "a".into() });
        let mut f = field();
        f.unit = Some(text.into());
        assert_eq!(bad_field(f), BadText { field: "a".into() });
    }
    // Field-local validation precedes duplicate checking; names precede version.
    let mut f = field();
    f.since = 0;
    assert_eq!(
        TypeDesc::new("Test", 1, vec![field(), f]),
        Err(BadVersion {
            field: Some("a".into()),
            version: 0
        })
    );
    assert_eq!(TypeDesc::new("", 0, vec![]), Err(BadName { name: "".into() }));
    let desc = Tiny::desc();
    assert_eq!(desc.name(), "Tiny");
    assert_eq!(desc.version(), 1);
    assert_eq!(desc.field_index("scale"), Some(1));
    assert_eq!(desc.field_index("nope"), None);
    assert_eq!(desc.field(3), None);
    assert_eq!(every_kind().fields().len(), 10);
    // Defaults are shape checked, not range checked.
    let mut f = field();
    f.min = Some(0.0);
    f.max = Some(1.0);
    f.default = Some(Value::F64(100.0));
    assert!(TypeDesc::new("_Test2", 1, vec![f]).is_ok());
}

#[test]
fn tiny_get_set_follows_descriptor() {
    let mut value = support::tiny();
    let desc = Tiny::desc();
    assert_eq!(value.get(0), Some(Value::Text(value.title.clone())));
    assert_eq!(value.get(1), Some(Value::F64(value.scale)));
    assert_eq!(value.get(2), Some(Value::U64(value.count)));
    for (i, field) in desc.fields().iter().enumerate() {
        assert!(field.kind.accepts(&value.get(i).unwrap()));
    }
    assert_eq!(value.get(3), None);
    value.set(1, Value::F64(4.0)).unwrap();
    assert_eq!((&*value.title, value.scale, value.count), ("probe", 4.0, 3));
    let before: Vec<_> = (0..3).map(|i| value.get(i)).collect();
    assert_eq!(
        value.set(1, Value::I64(4)),
        Err(SetError::WrongKind {
            field: "scale".into(),
            expected: "f64",
            got: "i64"
        })
    );
    assert_eq!((0..3).map(|i| value.get(i)).collect::<Vec<_>>(), before);
    assert_eq!(
        value.set(3, Value::Bool(true)),
        Err(SetError::NoField { index: 3, count: 3 })
    );
    assert_eq!((0..3).map(|i| value.get(i)).collect::<Vec<_>>(), before);
    let object: &mut dyn Reflect = &mut value;
    assert_eq!(object.dyn_desc(), desc);
    object.set(1, Value::F64(5.0)).unwrap();
    assert_eq!(object.get(1), Some(Value::F64(5.0)));
    object.set(2, Value::U64(9)).unwrap();
    assert_eq!(object.get(2), Some(Value::U64(9)));
    object.set(0, Value::Text("changed".into())).unwrap();
    assert_eq!(object.get(0), Some(Value::Text("changed".into())));
}

#[test]
fn xorshift_seeded_sequence_is_fixed() {
    let mut rng = XorShift64::new(1);
    for expected in [0x40822041, 0x100041060c011441, 0x9b1e842f6e862629] {
        assert_eq!(rng.next_u64(), expected);
    }
    let mut left = XorShift64::new(1);
    let mut right = XorShift64::new(1);
    for _ in 0..100 {
        assert_eq!(left.next_u64(), right.next_u64());
    }
    assert!(std::panic::catch_unwind(|| XorShift64::new(0)).is_err());
}

#[test]
fn random_values_fit_their_kind() {
    let kind = Kind::Struct(Box::new(every_kind()));
    let mut rng = XorShift64::new(7);
    let mut nonempty = false;
    let mut some = false;
    let mut none = false;
    for _ in 0..200 {
        let value = support::random_value(&kind, &mut rng, 3);
        assert!(kind.accepts(&value), "{value:?}");
        let Value::Struct(fields) = value else {
            panic!("expected struct")
        };
        nonempty |= matches!(&fields[5], Value::List(v) if !v.is_empty());
        some |= matches!(&fields[8], Value::Option(Some(_)));
        none |= matches!(&fields[8], Value::Option(None));
    }
    assert!(nonempty, "seed 7 must generate a nonempty list");
    assert!(some, "seed 7 must generate Some");
    assert!(none, "seed 7 must generate None");
    let array = Kind::Array(Box::new(Kind::Option(Box::new(Kind::Text))), 2);
    let value = support::random_value(&array, &mut rng, 0);
    assert_eq!(value, Value::List(vec![Value::Option(None); 2]));
    assert_eq!(
        support::random_value(&Kind::List(Box::new(Kind::Bool)), &mut rng, 0),
        Value::List(vec![])
    );
}
