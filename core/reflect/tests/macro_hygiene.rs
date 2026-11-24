use reflect::desc::{FieldDesc, Kind, Reflect, SetError, TypeDesc};
use reflect::macros::Field;
use reflect::value::Value;

mod shadowed {
    #![allow(dead_code)]

    pub enum Option {
        Unused,
    }
    pub enum Result {
        Unused,
    }
    pub struct Some;
    pub struct None;
    pub struct Ok;
    pub struct Err;

    reflect::reflect! {
        pub enum Mode { A, B }
    }

    reflect::reflect! {
        #[reflect(version = 2)]
        pub struct Probe {
            #[reflect(label = "Mass", unit = "kg", min = 1.0, max = 9.0, renamed_from = "mass")]
            pub mass_kg: f64,
            #[reflect(readonly)]
            pub flag: bool,
            #[reflect(hidden)]
            pub note: ::core::option::Option<::std::string::String>,
            #[reflect(since = 2, default = Mode::B)]
            pub mode: Mode,
            pub list: ::std::vec::Vec<i64>,
            pub pos: [f64; 2],
        }
    }
}

#[test]
fn reflect_macro_expands_where_prelude_names_are_shadowed() {
    use shadowed::{Mode, Probe};

    let mut mass = FieldDesc::new("mass_kg", Kind::F64);
    mass.label = Some("Mass".into());
    mass.unit = Some("kg".into());
    mass.min = Some(1.0);
    mass.max = Some(9.0);
    mass.renamed_from = Some("mass".into());
    let mut flag = FieldDesc::new("flag", Kind::Bool);
    flag.readonly = true;
    let mut note = FieldDesc::new("note", Kind::Option(Box::new(Kind::Text)));
    note.hidden = true;
    let mut mode = FieldDesc::new(
        "mode",
        Kind::Enum {
            name: "Mode".into(),
            variants: vec!["A".into(), "B".into()],
        },
    );
    mode.since = 2;
    mode.default = Some(Value::Enum("B".into()));
    let list = FieldDesc::new("list", Kind::List(Box::new(Kind::I64)));
    let pos = FieldDesc::new("pos", Kind::Array(Box::new(Kind::F64), 2));
    let expected = TypeDesc::new("Probe", 2, vec![mass, flag, note, mode, list, pos]).unwrap();
    assert_eq!(<Probe as Reflect>::desc(), expected);

    let mut p = Probe {
        mass_kg: 1.5,
        flag: true,
        note: Some("x".into()),
        mode: Mode::A,
        list: vec![1, 2],
        pos: [0.5, -0.5],
    };
    assert_eq!(p.get(0), Some(Value::F64(1.5)));
    assert_eq!(p.get(6), None);
    assert_eq!(p.set(0, Value::F64(2.5)), Ok(()));
    assert_eq!(p.mass_kg, 2.5);
    assert!(matches!(
        p.set(0, Value::Bool(true)),
        Err(SetError::WrongKind { .. })
    ));
    assert_eq!(
        p.set(6, Value::F64(0.0)),
        Err(SetError::NoField { index: 6, count: 6 })
    );
    assert_eq!(
        <Probe as Field>::from_value(<Probe as Field>::to_value(&p)),
        Some(p.clone())
    );
    assert_eq!(
        <Mode as Field>::from_value(Value::Enum("B".into())),
        Some(Mode::B)
    );
}
