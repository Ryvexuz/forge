use reflect::desc::{FieldDesc, Kind, Reflect, SetError, TypeDesc};
use reflect::macros::Field;
use reflect::sample::{DriveMode, Vehicle, Wheel};
use reflect::value::Value;

fn wheel_desc() -> TypeDesc {
    let mut radius = FieldDesc::new("radius_m", Kind::F64);
    radius.label = Some("Radius".into());
    radius.unit = Some("m".into());
    radius.min = Some(0.1);
    radius.max = Some(1.5);
    let mut width = FieldDesc::new("width_m", Kind::F64);
    width.label = Some("Width".into());
    width.unit = Some("m".into());
    width.min = Some(0.05);
    width.max = Some(0.6);
    let mut steered = FieldDesc::new("steered", Kind::Bool);
    steered.label = Some("Steered".into());
    TypeDesc::new("Wheel", 1, vec![radius, width, steered]).unwrap()
}

fn drive_kind() -> Kind {
    Kind::Enum {
        name: "DriveMode".into(),
        variants: vec!["Front".into(), "Rear".into(), "All".into()],
    }
}

fn vehicle_desc() -> TypeDesc {
    let mut name = FieldDesc::new("name", Kind::Text);
    name.label = Some("Name".into());
    let mut mass = FieldDesc::new("mass_kg", Kind::F64);
    mass.label = Some("Mass".into());
    mass.unit = Some("kg".into());
    mass.min = Some(500.0);
    mass.max = Some(5000.0);
    mass.renamed_from = Some("mass".into());
    let mut position = FieldDesc::new("position", Kind::Array(Box::new(Kind::F64), 3));
    position.label = Some("Position".into());
    position.unit = Some("m".into());
    let mut wheels = FieldDesc::new(
        "wheels",
        Kind::List(Box::new(Kind::Struct(Box::new(wheel_desc())))),
    );
    wheels.label = Some("Wheels".into());
    let mut note = FieldDesc::new("note", Kind::Option(Box::new(Kind::Text)));
    note.label = Some("Note".into());
    let mut drive = FieldDesc::new("drive", drive_kind());
    drive.label = Some("Drive".into());
    drive.since = 2;
    drive.default = Some(Value::Enum("Rear".into()));
    TypeDesc::new("Vehicle", 2, vec![name, mass, position, wheels, note, drive]).unwrap()
}

fn vehicle() -> Vehicle {
    Vehicle {
        name: "Roadster".into(),
        mass_kg: 1200.0,
        position: [1.0, 2.0, 3.0],
        wheels: vec![
            Wheel {
                radius_m: 0.3,
                width_m: 0.2,
                steered: true,
            },
            Wheel {
                radius_m: 0.4,
                width_m: 0.25,
                steered: false,
            },
        ],
        note: None,
        drive: DriveMode::Front,
    }
}

fn wheel_value(wheel: &Wheel) -> Value {
    Value::Struct(vec![
        Value::F64(wheel.radius_m),
        Value::F64(wheel.width_m),
        Value::Bool(wheel.steered),
    ])
}

fn vehicle_values(value: &Vehicle) -> Vec<Value> {
    vec![
        Value::Text(value.name.clone()),
        Value::F64(value.mass_kg),
        Value::List(value.position.iter().copied().map(Value::F64).collect()),
        Value::List(value.wheels.iter().map(wheel_value).collect()),
        Value::Option(value.note.as_ref().map(|s| Box::new(Value::Text(s.clone())))),
        Value::Enum(
            match value.drive {
                DriveMode::Front => "Front",
                DriveMode::Rear => "Rear",
                DriveMode::All => "All",
            }
            .into(),
        ),
    ]
}

#[test]
fn macro_declares_struct_and_desc() {
    assert_eq!(Wheel::desc(), wheel_desc());
    assert_eq!(Vehicle::desc(), vehicle_desc());
    let value = vehicle();
    let object: &dyn Reflect = &value;
    assert_eq!(object.dyn_desc(), vehicle_desc());
    assert_eq!(value.wheels[0].dyn_desc(), wheel_desc());
    assert_eq!(Vehicle::kind(), Kind::Struct(Box::new(vehicle_desc())));
    assert_eq!(
        Vehicle::from_value(Value::Struct(vehicle_values(&value))),
        Some(value)
    );
}

#[test]
fn macro_attributes_recorded() {
    reflect::reflect! {
        #[reflect(version = 3)]
        struct Attributes {
            #[reflect(label = "Caption")]
            label: String,
            #[reflect(unit = "m")]
            unit: f64,
            #[reflect(min = -2.0)]
            min: f64,
            #[reflect(max = 8.0)]
            max: f64,
            #[reflect(readonly)]
            readonly: bool,
            #[reflect(hidden)]
            hidden: bool,
            #[reflect(default = String::from("seed"))]
            default: String,
            #[reflect(renamed_from = "old")]
            renamed: i64,
            #[reflect(since = 3)]
            since: u64,
            bare: bool,
        }
    }
    let mut expected = vec![
        FieldDesc::new("label", Kind::Text),
        FieldDesc::new("unit", Kind::F64),
        FieldDesc::new("min", Kind::F64),
        FieldDesc::new("max", Kind::F64),
        FieldDesc::new("readonly", Kind::Bool),
        FieldDesc::new("hidden", Kind::Bool),
        FieldDesc::new("default", Kind::Text),
        FieldDesc::new("renamed", Kind::I64),
        FieldDesc::new("since", Kind::U64),
        FieldDesc::new("bare", Kind::Bool),
    ];
    expected[0].label = Some("Caption".into());
    expected[1].unit = Some("m".into());
    expected[2].min = Some(-2.0);
    expected[3].max = Some(8.0);
    expected[4].readonly = true;
    expected[5].hidden = true;
    expected[6].default = Some(Value::Text("seed".into()));
    expected[7].renamed_from = Some("old".into());
    expected[8].since = 3;
    assert_eq!(
        Attributes::desc(),
        TypeDesc::new("Attributes", 3, expected).unwrap()
    );

    reflect::reflect! {
        #[reflect(version = 3)]
        struct Reordered {
            #[reflect(since = 2, default = 4.0, hidden, max = 9.0, readonly,
                      renamed_from = "old", min = -1.0, unit = "m", label = "All",)]
            value: f64,
        }
    }
    let mut all = FieldDesc::new("value", Kind::F64);
    all.since = 2;
    all.default = Some(Value::F64(4.0));
    all.hidden = true;
    all.max = Some(9.0);
    all.readonly = true;
    all.renamed_from = Some("old".into());
    all.min = Some(-1.0);
    all.unit = Some("m".into());
    all.label = Some("All".into());
    assert_eq!(
        Reordered::desc(),
        TypeDesc::new("Reordered", 3, vec![all]).unwrap()
    );

    reflect::reflect! {
        #[reflect(version = 0)]
        struct Invalid { bare: bool }
    }
    let panic = std::panic::catch_unwind(Invalid::desc).unwrap_err();
    let message = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap();
    assert!(message.starts_with("reflect!: invalid descriptor for Invalid"));
}

#[test]
fn macro_nested_and_vec() {
    let kinds = vec![
        Kind::Text,
        Kind::F64,
        Kind::Array(Box::new(Kind::F64), 3),
        Kind::List(Box::new(Kind::Struct(Box::new(wheel_desc())))),
        Kind::Option(Box::new(Kind::Text)),
        drive_kind(),
    ];
    let actual: Vec<_> = Vehicle::desc()
        .fields()
        .iter()
        .map(|f| f.kind.clone())
        .collect();
    assert_eq!(actual, kinds);
    let mut value = vehicle();
    let wheels = vec![wheel_value(&value.wheels[0]), wheel_value(&value.wheels[1])];
    assert_eq!(value.get(3), Some(Value::List(wheels.clone())));
    let before = value.clone();
    let mut malformed = wheels.clone();
    malformed[1] = Value::Struct(vec![
        Value::F64(0.4),
        Value::Text("bad".into()),
        Value::Bool(false),
    ]);
    assert_eq!(Vec::<Wheel>::from_value(Value::List(malformed.clone())), None);
    let error = SetError::WrongKind {
        field: "wheels".into(),
        expected: "list",
        got: "list",
    };
    assert_eq!(value.set(3, Value::List(malformed)), Err(error));
    assert_eq!(value, before);
    assert_eq!(
        Vec::<Wheel>::from_value(Value::List(wheels)),
        Some(value.wheels.clone())
    );
    for fields in [
        vec![Value::F64(0.3), Value::F64(0.2)],
        vec![
            Value::F64(0.3),
            Value::F64(0.2),
            Value::Bool(true),
            Value::Bool(false),
        ],
    ] {
        assert_eq!(Wheel::from_value(Value::Struct(fields)), None);
    }
    let text_array = [String::from("a"), String::from("b")];
    assert_eq!(
        <[String; 2]>::from_value(text_array.to_value()),
        Some(text_array)
    );
    let too_long = Value::List(vec![Value::F64(0.0); 4]);
    assert_eq!(<[f64; 3]>::from_value(too_long), None);
    let wrong_element = Value::List(vec![Value::F64(0.0), Value::Bool(true), Value::F64(0.0)]);
    assert_eq!(<[f64; 3]>::from_value(wrong_element), None);
    let nested = Some(vec![Some(1_i64), None, Some(i64::MIN)]);
    assert_eq!(
        Option::<Vec<Option<i64>>>::from_value(nested.to_value()),
        Some(nested)
    );
    assert_eq!(Vec::<u64>::from_value(Value::List(vec![])), Some(vec![]));
    assert_eq!(bool::kind(), Kind::Bool);
    assert_eq!(i64::kind(), Kind::I64);
    assert_eq!(u64::kind(), Kind::U64);
    for number in [i64::MIN, 0, i64::MAX] {
        assert_eq!(i64::from_value(Value::I64(number)), Some(number));
    }
    for number in [0, u64::MAX] {
        assert_eq!(u64::from_value(Value::U64(number)), Some(number));
    }
    for number in [
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
        -f64::MAX,
        0.0,
    ] {
        assert_eq!(f64::from_value(Value::F64(number)), Some(number));
    }
    assert_eq!(bool::from_value(Value::I64(1)), None);
    assert_eq!(i64::from_value(Value::U64(1)), None);
    assert_eq!(u64::from_value(Value::I64(-1)), None);
    assert_eq!(String::from_value(Value::Bool(false)), None);
    assert_eq!(Vec::<bool>::from_value(Value::Bool(false)), None);
    assert_eq!(Option::<String>::from_value(Value::Text("x".into())), None);
    assert_eq!(Wheel::from_value(Value::Bool(false)), None);
    assert_eq!(DriveMode::from_value(Value::Text("Rear".into())), None);
    for mode in [DriveMode::Front, DriveMode::Rear, DriveMode::All] {
        assert_eq!(DriveMode::from_value(mode.to_value()), Some(mode));
    }
}

#[test]
fn macro_get_set_matches_field() {
    let original = vehicle();
    let mut changed = original.clone();
    changed.name = "New".into();
    changed.mass_kg = 1800.0;
    changed.position = [-3.0, 0.0, 8.0];
    changed.wheels = vec![Wheel {
        radius_m: 0.5,
        width_m: 0.3,
        steered: false,
    }];
    changed.note = Some("Ready".into());
    changed.drive = DriveMode::All;
    let old_values = vehicle_values(&original);
    let new_values = vehicle_values(&changed);
    for (index, (old, new)) in old_values.iter().zip(&new_values).enumerate() {
        assert_eq!(original.get(index).as_ref(), Some(old), "get field {index}");
        let mut value = original.clone();
        let mut expected = original.clone();
        match index {
            0 => expected.name = changed.name.clone(),
            1 => expected.mass_kg = changed.mass_kg,
            2 => expected.position = changed.position,
            3 => expected.wheels = changed.wheels.clone(),
            4 => expected.note = changed.note.clone(),
            5 => expected.drive = changed.drive,
            _ => unreachable!(),
        }
        assert_eq!(value.set(index, new.clone()), Ok(()), "set field {index}");
        assert_eq!(value, expected, "only field {index} changes");
        assert_eq!(value.get(index).as_ref(), Some(new));
        let before = value.clone();
        let names = ["name", "mass_kg", "position", "wheels", "note", "drive"];
        let kinds = ["text", "f64", "array", "list", "option", "enum"];
        let error = SetError::WrongKind {
            field: names[index].into(),
            expected: kinds[index],
            got: "bool",
        };
        assert_eq!(value.set(index, Value::Bool(true)), Err(error));
        assert_eq!(value, before, "wrong kind leaves field {index} unchanged");
    }
    let mut value = original.clone();
    assert_eq!(value.get(6), None);
    for index in [6, usize::MAX] {
        assert_eq!(value.get(index), None);
        let error = SetError::NoField { index, count: 6 };
        assert_eq!(value.set(index, Value::Bool(false)), Err(error));
        assert_eq!(value, original);
    }
    let short_position = Value::List(vec![Value::F64(1.0); 2]);
    let bad_note = Value::Option(Some(Box::new(Value::I64(1))));
    for (index, bad, field, expected, got) in [
        (2, short_position, "position", "array", "list"),
        (5, Value::Enum("Unknown".into()), "drive", "enum", "enum"),
        (4, bad_note, "note", "option", "option"),
    ] {
        let error = SetError::WrongKind {
            field: field.into(),
            expected,
            got,
        };
        assert_eq!(value.set(index, bad), Err(error));
        assert_eq!(value, original);
    }
    let note = Value::Option(Some(Box::new(Value::Text("memo".into()))));
    assert_eq!(value.set(4, note.clone()), Ok(()));
    assert_eq!(value.note.as_deref(), Some("memo"));
    assert_eq!(value.get(4), Some(note));
    assert_eq!(value.set(4, Value::Option(None)), Ok(()));
    assert_eq!(value.get(4), Some(Value::Option(None)));
    assert_eq!(value, original);
}
