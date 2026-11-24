use reflect::change::{Change, ChangeSink};
use reflect::desc::{FieldDesc, Kind, Reflect, SetError, TypeDesc};
use reflect::limits::Limits;
use reflect::path::{
    get_path, parse_path, set_path, AccessError as E, Path, PathError as P, Segment,
};
use reflect::value::Value as V;

const L: Limits = Limits {
    max_path_length: 256,
    max_text_file_bytes: 1048576,
    max_list_length: 4096,
    max_nesting_depth: 16,
};

fn bounded(name: &str, kind: Kind, min: Option<f64>, max: Option<f64>) -> FieldDesc {
    let mut field = FieldDesc::new(name, kind);
    field.min = min;
    field.max = max;
    field
}

fn readonly(name: &str, kind: Kind) -> FieldDesc {
    let mut field = FieldDesc::new(name, kind);
    field.readonly = true;
    field
}

fn wheel_desc() -> TypeDesc {
    TypeDesc::new(
        "Wheel",
        1,
        vec![
            bounded("radius", Kind::F64, Some(0.1), Some(1.0)),
            readonly("id", Kind::U64),
        ],
    )
    .unwrap()
}

fn wheel(radius: f64, id: u64) -> V {
    V::Struct(vec![V::F64(radius), V::U64(id)])
}

fn wheels() -> V {
    V::List(vec![wheel(0.3, 1), wheel(0.4, 2)])
}

struct Obj {
    fields: Vec<V>,
}

impl Obj {
    fn new() -> Self {
        Self {
            fields: vec![
                V::Text("rig".into()),
                V::F64(1200.0),
                V::I64(0),
                V::U64(7),
                wheels(),
                V::List(vec![V::F64(0.0); 3]),
                V::Option(None),
                V::Enum("A".into()),
            ],
        }
    }
}

fn assign(fields: &mut [V], desc: &TypeDesc, index: usize, value: V) -> Result<(), SetError> {
    let count = fields.len();
    let slot = fields
        .get_mut(index)
        .ok_or(SetError::NoField { index, count })?;
    let field = desc.field(index).ok_or(SetError::NoField { index, count })?;
    if !field.kind.accepts(&value) {
        return Err(SetError::WrongKind {
            field: field.name.clone(),
            expected: field.kind.name(),
            got: value.kind_name(),
        });
    }
    *slot = value;
    Ok(())
}

impl Reflect for Obj {
    fn desc() -> TypeDesc {
        TypeDesc::new(
            "Rig",
            1,
            vec![
                FieldDesc::new("name", Kind::Text),
                bounded("mass_kg", Kind::F64, Some(100.0), Some(5000.0)),
                bounded("count", Kind::I64, Some(-3.0), Some(3.0)),
                readonly("serial", Kind::U64),
                FieldDesc::new(
                    "wheels",
                    Kind::List(Box::new(Kind::Struct(Box::new(wheel_desc())))),
                ),
                FieldDesc::new("pos", Kind::Array(Box::new(Kind::F64), 3)),
                FieldDesc::new("note", Kind::Option(Box::new(Kind::Text))),
                FieldDesc::new(
                    "mode",
                    Kind::Enum {
                        name: "Mode".into(),
                        variants: vec!["A".into(), "B".into()],
                    },
                ),
            ],
        )
        .unwrap()
    }

    fn dyn_desc(&self) -> TypeDesc {
        Self::desc()
    }

    fn get(&self, index: usize) -> Option<V> {
        self.fields.get(index).cloned()
    }

    fn set(&mut self, index: usize, value: V) -> Result<(), SetError> {
        assign(&mut self.fields, &Self::desc(), index, value)
    }
}

struct Custom {
    desc: TypeDesc,
    fields: Vec<V>,
    refuse: bool,
}

impl Reflect for Custom {
    fn desc() -> TypeDesc {
        TypeDesc::new("Other", 1, vec![]).unwrap()
    }

    fn dyn_desc(&self) -> TypeDesc {
        self.desc.clone()
    }

    fn get(&self, index: usize) -> Option<V> {
        self.fields.get(index).cloned()
    }

    fn set(&mut self, index: usize, value: V) -> Result<(), SetError> {
        if self.refuse {
            return Err(SetError::NoField {
                index,
                count: self.fields.len(),
            });
        }
        assign(&mut self.fields, &self.desc, index, value)
    }
}

fn custom(field: FieldDesc, value: V) -> Custom {
    Custom {
        desc: TypeDesc::new("Other", 1, vec![field]).unwrap(),
        fields: vec![value],
        refuse: false,
    }
}

#[derive(Default)]
struct Recording(Vec<Change>);

impl ChangeSink for Recording {
    fn changed(&mut self, change: &Change) {
        self.0.push(change.clone());
    }
}

fn path(text: &str) -> Path {
    parse_path(text, &L).unwrap()
}

fn rejected(root: &mut dyn Reflect, text: &str, value: V, limits: &Limits, error: E) {
    let before: Vec<_> = (0..root.dyn_desc().fields().len())
        .map(|i| root.get(i))
        .collect();
    let mut sink = Recording::default();
    let parsed = path(text);
    if matches!(
        error,
        E::NoField { .. }
            | E::IndexOutOfRange { .. }
            | E::NotIndexable { .. }
            | E::NotAStruct { .. }
    ) {
        assert_eq!(get_path(root, &parsed), Err(error.clone()), "{text}");
    }
    assert_eq!(
        set_path(root, &parsed, value, limits, &mut sink),
        Err(error),
        "{text}"
    );
    for (i, old) in before.iter().enumerate() {
        assert_eq!(&root.get(i), old, "{text}: field {i} changed");
    }
    assert!(sink.0.is_empty(), "{text}: refusal emitted a change");
}

fn refused(text: &str, value: V, error: E) {
    rejected(&mut Obj::new(), text, value, &L, error);
}

#[test]
fn path_parse_known_paths() {
    use Segment::{Field as F, Index as I};
    let cases = [
        ("name", Some(vec![F("name".into())])),
        (
            "wheels[1].radius",
            Some(vec![F("wheels".into()), I(1), F("radius".into())]),
        ),
        ("pos[2]", Some(vec![F("pos".into()), I(2)])),
        ("a.b.c", None),
        ("x[0][1]", None),
        ("_a1", None),
        ("wheels[0]", None),
        (
            "w[18446744073709551615]",
            Some(vec![F("w".into()), I(usize::MAX)]),
        ),
    ];
    for (text, expected) in cases {
        let parsed = path(text);
        assert_eq!(parsed.to_string(), text);
        if let Some(expected) = expected {
            assert_eq!(parsed.segments(), expected);
        }
    }
}

#[test]
fn path_parse_errors_named() {
    let bad = |text: &str| P::BadIndex {
        at: 2,
        text: text.into(),
    };
    let cases = [
        ("", P::Empty),
        (".a", P::EmptySegment { at: 0 }),
        ("a.", P::EmptySegment { at: 2 }),
        ("a..b", P::EmptySegment { at: 2 }),
        ("[0]", P::EmptySegment { at: 0 }),
        ("a.[0]", P::EmptySegment { at: 2 }),
        ("1a", P::BadName { at: 0 }),
        ("é", P::BadName { at: 0 }),
        ("a-b", P::BadChar { at: 1 }),
        ("a[1]b", P::BadChar { at: 4 }),
        ("a[1", P::UnclosedIndex { at: 1 }),
        ("a[]", bad("")),
        ("a[-1]", bad("-1")),
        ("a[x]", bad("x")),
        ("a[01]", bad("01")),
        ("a[18446744073709551616]", bad("18446744073709551616")),
        ("a[é]", bad("é")),
        ("a[+1]", bad("+1")),
        ("a[1.0]", bad("1.0")),
        ("a[ 1]", bad(" 1")),
        ("a[[1]", bad("[1")),
        ("a\\b", P::BadChar { at: 1 }),
        ("C:\\a", P::BadChar { at: 1 }),
        ("a\r.b", P::BadChar { at: 1 }),
        ("a\nb", P::BadChar { at: 1 }),
    ];
    for (text, expected) in cases {
        assert_eq!(parse_path(text, &L), Err(expected), "{text:?}");
    }
    let too_long = P::TooLong {
        len: 257,
        max: 256,
    };
    assert_eq!(parse_path(&"a".repeat(257), &L), Err(too_long.clone()));
    assert!(parse_path(&"a".repeat(256), &L).is_ok());
    assert_eq!(P::EmptySegment { at: 2 }.to_string(), "empty name at byte 2");
    assert_eq!(bad("-1").to_string(), "bad index \"-1\" at byte 2");
    assert_eq!(too_long.to_string(), "path is 257 bytes, over the limit 256");
    assert_eq!(parse_path(&format!(".{}", "x".repeat(256)), &L), Err(too_long));
    let huge = "[".repeat(1048576);
    assert_eq!(
        parse_path(&huge, &L),
        Err(P::TooLong {
            len: huge.len(),
            max: L.max_path_length,
        })
    );
    let zero = Limits {
        max_path_length: 0,
        ..L
    };
    assert_eq!(parse_path("?", &zero), Err(P::TooLong { len: 1, max: 0 }));
    assert_eq!(parse_path("", &zero), Err(P::Empty));
}

#[test]
fn path_get_set_known_values() {
    let mut root = Obj::new();
    let mut sink = Recording::default();
    assert_eq!(get_path(&root, &path("wheels[1].radius")), Ok(V::F64(0.4)));
    assert_eq!(
        get_path(&root, &path("pos")),
        Ok(V::List(vec![V::F64(0.0); 3]))
    );
    let cases = [
        ("name", V::Text("rig".into()), V::Text("r2".into())),
        ("wheels[1].radius", V::F64(0.4), V::F64(0.34)),
        ("pos[2]", V::F64(0.0), V::F64(-1.5)),
        (
            "note",
            V::Option(None),
            V::Option(Some(Box::new(V::Text("hi".into())))),
        ),
        ("mode", V::Enum("A".into()), V::Enum("B".into())),
        ("mass_kg", V::F64(1200.0), V::F64(1200.0)),
    ];
    for (i, (text, old, new)) in cases.into_iter().enumerate() {
        let parsed = path(text);
        assert_eq!(
            set_path(&mut root, &parsed, new.clone(), &L, &mut sink),
            Ok(())
        );
        assert_eq!(sink.0.len(), i + 1);
        let change = sink.0.last().unwrap();
        assert_eq!(change.type_name(), "Rig");
        assert_eq!(change.path(), text);
        assert_eq!(change.old(), &old);
        assert_eq!(change.new_value(), &new);
        assert_eq!(get_path(&root, &parsed), Ok(new));
    }
    let mut nested = custom(
        FieldDesc::new(
            "x",
            Kind::List(Box::new(Kind::Array(Box::new(Kind::Bool), 1))),
        ),
        V::List(vec![V::List(vec![V::Bool(false)])]),
    );
    assert_eq!(
        set_path(&mut nested, &path("x[0][0]"), V::Bool(true), &L, &mut sink),
        Ok(())
    );
    assert_eq!(get_path(&nested, &path("x[0][0]")), Ok(V::Bool(true)));
}

#[test]
fn range_violation_rejected() {
    let range = |text: &str| E::OutOfRange { path: text.into() };
    for (text, value) in [
        ("mass_kg", V::F64(99.0)),
        ("mass_kg", V::F64(5000.5)),
        ("count", V::I64(-4)),
        ("count", V::I64(4)),
        ("wheels[0].radius", V::F64(1.5)),
        ("wheels[0]", wheel(0.05, 1)),
        ("wheels", V::List(vec![wheel(0.3, 1), wheel(2.0, 2)])),
    ] {
        let broken = match text {
            "wheels[0]" => "wheels[0].radius",
            "wheels" => "wheels[1].radius",
            _ => text,
        };
        refused(text, value, range(broken));
    }
    for (text, value) in [
        ("mass_kg", V::F64(100.0)),
        ("mass_kg", V::F64(5000.0)),
        ("count", V::I64(-3)),
        ("count", V::I64(3)),
    ] {
        let mut root = Obj::new();
        let mut sink = Recording::default();
        assert_eq!(
            set_path(&mut root, &path(text), value.clone(), &L, &mut sink),
            Ok(())
        );
        assert_eq!(get_path(&root, &path(text)), Ok(value));
        assert_eq!(sink.0.len(), 1);
    }
    let mut big = custom(bounded("big", Kind::U64, None, Some(1e300)), V::U64(0));
    let mut sink = Recording::default();
    assert_eq!(
        set_path(&mut big, &path("big"), V::U64(u64::MAX), &L, &mut sink),
        Ok(())
    );
    let mut half = custom(bounded("half", Kind::I64, Some(0.5), None), V::I64(1));
    rejected(&mut half, "half", V::I64(0), &L, range("half"));
    let mut exact = custom(
        bounded("exact", Kind::U64, None, Some(9007199254740992.0)),
        V::U64(0),
    );
    rejected(&mut exact, "exact", V::U64(9007199254740993), &L, range("exact"));
    let mut signed = custom(bounded("n", Kind::I64, Some(-1e300), Some(1e300)), V::I64(0));
    for n in [i64::MIN, i64::MAX] {
        assert_eq!(
            set_path(&mut signed, &path("n"), V::I64(n), &L, &mut sink),
            Ok(())
        );
    }
    let mut tiny = custom(bounded("n", Kind::F64, Some(0.0), Some(f64::MAX)), V::F64(0.0));
    for n in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX, -0.0] {
        assert_eq!(
            set_path(&mut tiny, &path("n"), V::F64(n), &L, &mut sink),
            Ok(())
        );
        assert_eq!(get_path(&tiny, &path("n")), Ok(V::F64(n)));
    }
    let mut optional = custom(
        FieldDesc::new(
            "x",
            Kind::Option(Box::new(Kind::Struct(Box::new(wheel_desc())))),
        ),
        V::Option(None),
    );
    rejected(
        &mut optional,
        "x",
        V::Option(Some(Box::new(wheel(2.0, 1)))),
        &L,
        range("x.radius"),
    );
}

#[test]
fn path_refused_sets_change_nothing() {
    let ro = |text: &str| E::ReadOnly { path: text.into() };
    let finite = |text: &str| E::NonFinite { path: text.into() };
    let wrong = |text: &str, expected: &'static str, got: &'static str| E::WrongKind {
        path: text.into(),
        expected,
        got,
    };
    let missing = |text: &str, name: &str| E::NoField {
        path: text.into(),
        name: name.into(),
    };
    let no_struct = |text: &str, kind: &'static str| E::NotAStruct {
        path: text.into(),
        kind,
    };
    let no_index = |text: &str, kind: &'static str| E::NotIndexable {
        path: text.into(),
        kind,
    };
    let cases = [
        ("serial", V::U64(8), ro("serial")),
        ("serial", V::U64(7), ro("serial")),
        ("wheels[0].id", V::U64(9), ro("wheels[0].id")),
        (
            "wheels",
            V::List(vec![wheel(0.3, 5), wheel(0.4, 2)]),
            ro("wheels"),
        ),
        ("name", V::I64(1), wrong("name", "text", "i64")),
        ("mass_kg", V::F64(f64::NAN), finite("mass_kg")),
        ("mass_kg", V::F64(f64::NEG_INFINITY), finite("mass_kg")),
        ("wheels[0]", wheel(f64::NAN, 1), finite("wheels[0]")),
        ("pos[0]", V::F64(f64::INFINITY), finite("pos[0]")),
        (
            "pos",
            V::List(vec![V::F64(0.0), V::F64(f64::NAN), V::F64(0.0)]),
            finite("pos"),
        ),
        (
            "wheels[5]",
            wheel(0.3, 1),
            E::IndexOutOfRange {
                path: "wheels[5]".into(),
                index: 5,
                len: 2,
            },
        ),
        ("name[0]", V::Text("x".into()), no_index("name[0]", "text")),
        ("mass_kg.x", V::F64(1.0), no_struct("mass_kg.x", "f64")),
        ("nope", V::Bool(true), missing("nope", "nope")),
        (
            "wheels[0].spin",
            V::F64(1.0),
            missing("wheels[0].spin", "spin"),
        ),
        ("note.x", V::Text("x".into()), no_struct("note.x", "option")),
        ("note[0]", V::Text("x".into()), no_index("note[0]", "option")),
        ("mode", V::Enum("C".into()), wrong("mode", "enum", "enum")),
        ("pos", V::List(vec![]), wrong("pos", "array", "list")),
    ];
    for (text, value, error) in cases {
        refused(text, value, error);
    }
    let short = Limits {
        max_list_length: 2,
        ..L
    };
    rejected(
        &mut Obj::new(),
        "pos",
        V::List(vec![V::F64(0.0); 3]),
        &short,
        E::TooLong {
            path: "pos".into(),
            max: 2,
        },
    );
    rejected(
        &mut Obj::new(),
        "pos",
        V::List(vec![V::F64(f64::NAN); 3]),
        &short,
        finite("pos"),
    );
    let mut root = Obj::new();
    let mut sink = Recording::default();
    assert_eq!(
        set_path(&mut root, &path("wheels"), wheels(), &L, &mut sink),
        Ok(())
    );
    assert_eq!(sink.0.len(), 1);
    assert_eq!(
        sink.0[0],
        Change::new("Rig", "wheels", wheels(), wheels()).unwrap()
    );
    let mut ancestor = custom(
        readonly("x", Kind::Struct(Box::new(wheel_desc()))),
        wheel(0.3, 1),
    );
    rejected(&mut ancestor, "x.radius", V::Text("bad".into()), &L, ro("x"));
    for kind in [
        Kind::Option(Box::new(Kind::Struct(Box::new(wheel_desc())))),
        Kind::Array(Box::new(Kind::Struct(Box::new(wheel_desc()))), 1),
    ] {
        let (old, new) = if matches!(kind, Kind::Option(_)) {
            (V::Option(None), V::Option(Some(Box::new(wheel(0.3, 1)))))
        } else {
            (V::List(vec![wheel(0.3, 1)]), V::List(vec![wheel(0.4, 1)]))
        };
        let mut root = custom(FieldDesc::new("x", kind), old);
        rejected(&mut root, "x", new, &L, ro("x"));
    }
    let mut broken = custom(FieldDesc::new("x", Kind::F64), V::F64(1.0));
    broken.refuse = true;
    let error = SetError::NoField { index: 0, count: 1 };
    rejected(
        &mut broken,
        "x",
        V::F64(2.0),
        &L,
        E::Set {
            path: "x".into(),
            error,
        },
    );
    let mut malformed = custom(
        FieldDesc::new("x", Kind::Struct(Box::new(wheel_desc()))),
        V::Struct(vec![]),
    );
    rejected(
        &mut malformed,
        "x.radius",
        V::F64(0.3),
        &L,
        missing("x.radius", "radius"),
    );
    let mut old_kind = custom(FieldDesc::new("x", Kind::F64), V::Text("bad".into()));
    rejected(
        &mut old_kind,
        "x",
        V::F64(2.0),
        &L,
        E::Change {
            path: "x".into(),
            error: reflect::change::ChangeError::KindChanged {
                old: "text",
                new: "f64",
            },
        },
    );
}
