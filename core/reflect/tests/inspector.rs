mod support;

use reflect::desc::{FieldDesc, Kind, Reflect, TypeDesc};
use reflect::inspector::{to_text, widgets, Widget};

fn desc(name: &str, fields: Vec<FieldDesc>) -> TypeDesc {
    TypeDesc::new(name, 1, fields).expect("valid test descriptor")
}

fn expected(path: &str, kind: &'static str) -> Widget {
    Widget {
        path: path.into(),
        kind,
        label: path.rsplit('.').next().unwrap().into(),
        unit: None,
        min: None,
        max: None,
        of: None,
        len: None,
        variants: Vec::new(),
        readonly: false,
    }
}

fn drive() -> Kind {
    Kind::Enum {
        name: "Drive".into(),
        variants: vec!["Rwd".into(), "Fwd".into(), "Awd".into()],
    }
}

#[test]
fn inspector_widget_kinds() {
    let mut gear = FieldDesc::new("gear", Kind::I64);
    gear.min = Some(-1.0);
    gear.max = Some(6.0);
    let mut mass = FieldDesc::new("mass", Kind::F64);
    mass.label = Some("Mass".into());
    mass.unit = Some("kg".into());
    mass.min = Some(500.0);
    mass.max = Some(3000.0);
    let mut radius = FieldDesc::new("radius", Kind::F64);
    radius.unit = Some("m".into());
    radius.min = Some(0.1);
    radius.max = Some(2.0);
    let wheel = desc(
        "Wheel",
        vec![
            radius,
            FieldDesc::new("grip", Kind::F64),
            FieldDesc::new("tag", Kind::Text),
        ],
    );
    let seat = desc(
        "Seat",
        vec![
            FieldDesc::new("height", Kind::F64),
            FieldDesc::new("belt", Kind::Bool),
        ],
    );
    let mut tint = FieldDesc::new("tint", Kind::Array(Box::new(Kind::F64), 3));
    tint.unit = Some("u".into());
    let car = desc(
        "Car",
        vec![
            FieldDesc::new("on", Kind::Bool),
            gear,
            FieldDesc::new("odo", Kind::U64),
            mass,
            FieldDesc::new("title", Kind::Text),
            FieldDesc::new("drive", drive()),
            FieldDesc::new("wheels", Kind::List(Box::new(Kind::Struct(Box::new(wheel))))),
            tint,
            FieldDesc::new("boost", Kind::Option(Box::new(Kind::I64))),
            FieldDesc::new("seat", Kind::Struct(Box::new(seat))),
            FieldDesc::new("grid", Kind::List(Box::new(Kind::List(Box::new(Kind::U64))))),
        ],
    );
    let mut want = vec![
        expected("on", "bool"),
        expected("gear", "i64"),
        expected("odo", "u64"),
        expected("mass", "f64"),
        expected("title", "text"),
        expected("drive", "enum"),
        expected("wheels", "list"),
        expected("wheels[0].radius", "f64"),
        expected("wheels[0].grip", "f64"),
        expected("wheels[0].tag", "text"),
        expected("tint", "array"),
        expected("tint[0]", "f64"),
        expected("boost", "option"),
        expected("seat", "struct"),
        expected("seat.height", "f64"),
        expected("seat.belt", "bool"),
        expected("grid", "list"),
        expected("grid[0]", "list"),
    ];
    want[1].min = Some(-1.0);
    want[1].max = Some(6.0);
    want[3].label = "Mass".into();
    want[3].unit = Some("kg".into());
    want[3].min = Some(500.0);
    want[3].max = Some(3000.0);
    want[5].variants = vec!["Rwd".into(), "Fwd".into(), "Awd".into()];
    want[6].of = Some("struct");
    want[7].unit = Some("m".into());
    want[7].min = Some(0.1);
    want[7].max = Some(2.0);
    want[10].of = Some("f64");
    want[10].len = Some(3);
    want[10].unit = Some("u".into());
    want[11].label = "tint item".into();
    want[11].unit = Some("u".into());
    want[12].of = Some("i64");
    want[16].of = Some("list");
    want[17].of = Some("u64");
    want[17].label = "grid item".into();
    assert_eq!(widgets(&car), want);

    let mut deep = desc("End", vec![FieldDesc::new("leaf", Kind::Bool)]);
    for i in (0..200).rev() {
        deep = desc(
            "Node",
            vec![FieldDesc::new(&format!("s{i}"), Kind::Struct(Box::new(deep)))],
        );
    }
    let rows = widgets(&deep);
    assert_eq!(rows.len(), 201);
    let path = (0..200)
        .map(|i| format!("s{i}"))
        .chain(std::iter::once("leaf".into()))
        .collect::<Vec<_>>()
        .join(".");
    assert_eq!(rows.last().unwrap().path, path);

    // Synthetic elements carry their own kind's metadata, but never expand.
    let nested = desc(
        "Nested",
        vec![
            FieldDesc::new(
                "arrays",
                Kind::List(Box::new(Kind::Array(Box::new(Kind::Bool), 2))),
            ),
            FieldDesc::new("enums", Kind::List(Box::new(drive()))),
            FieldDesc::new(
                "options",
                Kind::List(Box::new(Kind::Option(Box::new(Kind::Text)))),
            ),
        ],
    );
    let rows = widgets(&nested);
    assert_eq!(rows.len(), 6);
    assert_eq!((rows[1].of, rows[1].len), (Some("bool"), Some(2)));
    assert_eq!(rows[3].variants, ["Rwd", "Fwd", "Awd"]);
    assert_eq!(rows[5].of, Some("text"));
}

#[test]
fn inspector_respects_hidden_readonly() {
    let mut hidden = FieldDesc::new("secret", Kind::Text);
    hidden.hidden = true;
    let child = desc(
        "Child",
        vec![hidden.clone(), FieldDesc::new("visible", Kind::Bool)],
    );
    let mut hidden_list = FieldDesc::new(
        "concealed",
        Kind::List(Box::new(Kind::Struct(Box::new(child.clone())))),
    );
    hidden_list.hidden = true;
    let mut locked = FieldDesc::new("locked", Kind::Struct(Box::new(child.clone())));
    locked.readonly = true;
    let mut scalar = FieldDesc::new("fixed", Kind::U64);
    scalar.readonly = true;
    let mut list = FieldDesc::new("rows", Kind::List(Box::new(Kind::Bool)));
    list.readonly = true;
    let mut option = FieldDesc::new(
        "maybe",
        Kind::Option(Box::new(Kind::Struct(Box::new(child.clone())))),
    );
    option.readonly = true;
    let root = desc(
        "Root",
        vec![
            hidden,
            FieldDesc::new("open", Kind::Struct(Box::new(child))),
            hidden_list,
            scalar,
            locked,
            list,
            option,
            FieldDesc::new("plain", Kind::Text),
        ],
    );
    let rows = widgets(&root);
    let actual: Vec<_> = rows
        .iter()
        .map(|w| (w.path.as_str(), w.readonly))
        .collect();
    assert_eq!(
        actual,
        vec![
            ("open", false),
            ("open.visible", false),
            ("fixed", true),
            ("locked", true),
            ("locked.visible", true),
            ("rows", true),
            ("rows[0]", true),
            ("maybe", true),
            ("maybe.visible", true),
            ("plain", false),
        ]
    );
    assert!(!to_text(&root).contains("secret"));
    assert!(!to_text(&root).contains("concealed"));
}

#[test]
fn inspector_text_exact() {
    assert_eq!(
        to_text(&support::Tiny::desc()),
        concat!(
            "widget path=title kind=text label=\"Title\"\n",
            "widget path=scale kind=f64 label=\"Scale\" unit=\"m\" min=0 max=10\n",
            "widget path=count kind=u64 label=\"count\" readonly\n",
        )
    );
    let mut label = FieldDesc::new("message", Kind::Text);
    label.label = Some("Say \"hi\" \\ now".into());
    label.unit = Some("a\"b\\c".into());
    let mut range = FieldDesc::new("range", Kind::F64);
    range.min = Some(-0.0);
    range.max = Some(f64::MAX);
    let text = to_text(&desc(
        "Text",
        vec![
            label,
            FieldDesc::new("drive", drive()),
            FieldDesc::new("array", Kind::Array(Box::new(Kind::F64), 3)),
            range,
        ],
    ));
    assert_eq!(
        text,
        format!(
            concat!(
                "widget path=message kind=text label=\"Say \\\"hi\\\" \\\\ now\" unit=\"a\\\"b\\\\c\"\n",
                "widget path=drive kind=enum label=\"drive\" variants=Rwd|Fwd|Awd\n",
                "widget path=array kind=array label=\"array\" of=f64 len=3\n",
                "widget path=array[0] kind=f64 label=\"array item\"\n",
                "widget path=range kind=f64 label=\"range\" min=-0 max={}\n",
            ),
            f64::MAX
        )
    );
    assert_eq!(to_text(&desc("Empty", vec![])), "");
}
