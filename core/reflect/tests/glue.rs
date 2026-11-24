use reflect::desc::{Kind, Reflect};
use reflect::sample::{Vehicle, VehicleWide};
use reflect::{inspector, schema};

#[test]
fn wide_type_has_one_extra_field() {
    let base = Vehicle::desc();
    let wide = VehicleWide::desc();
    assert_eq!(wide.name(), "VehicleWide");
    assert_eq!(wide.version(), 2);
    assert_eq!(base.fields().len(), 6);
    assert_eq!(wide.fields().len(), 7);
    assert_eq!(&wide.fields()[..6], base.fields());
    let armor = &wide.fields()[6];
    assert_eq!(armor.name, "armor_rating");
    assert_eq!(armor.kind, Kind::F64);
    assert_eq!(armor.label, Some("Armor rating".into()));
    assert_eq!(armor.unit, None);
    assert_eq!(armor.min, Some(0.0));
    assert_eq!(armor.max, Some(10.0));
    assert!(!armor.readonly);
    assert!(!armor.hidden);

    let base_text = inspector::to_text(&base);
    let wide_text = inspector::to_text(&wide);
    let base_lines: Vec<_> = base_text.lines().collect();
    let wide_lines: Vec<_> = wide_text.lines().collect();
    assert!(!base_lines.is_empty());
    assert_eq!(wide_lines.len(), base_lines.len() + 1);
    assert_eq!(&wide_lines[..base_lines.len()], base_lines.as_slice());
    assert!(wide_lines
        .last()
        .unwrap()
        .starts_with("widget path=armor_rating kind=f64 label=\"Armor rating\""));

    let base_schema = schema::schema_text(&base).unwrap();
    let mut expected = String::new();
    let mut headers = 0;
    let mut insertions = 0;
    for line in base_schema.lines() {
        if line == "type Vehicle version=2 fields=6" {
            expected.push_str("type VehicleWide version=2 fields=7\n");
            headers += 1;
        } else {
            expected.push_str(line);
            expected.push('\n');
        }
        if line.starts_with("field drive ") {
            expected.push_str(concat!(
                "field armor_rating kind=f64 since=1 label=\"Armor rating\" ",
                "min=0x0000000000000000 max=0x4024000000000000\n"
            ));
            insertions += 1;
        }
    }
    assert_eq!(headers, 1, "expected one root type header");
    assert_eq!(insertions, 1, "expected one drive field");
    assert_eq!(schema::schema_text(&wide).unwrap(), expected);
}

#[test]
fn no_fixture_names_in_generic_modules() {
    let names = [
        "Vehicle",
        "VehicleWide",
        "Wheel",
        "DriveMode",
        "Roadster",
        "mass_kg",
        "radius_m",
        "width_m",
        "steered",
        "wheels",
        "armor_rating",
    ];
    let words = |text: &str| {
        text.split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|word| !word.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        words("Vehicle VehicleWide\rWheel"),
        ["Vehicle", "VehicleWide", "Wheel"]
    );
    assert!(!words("prefixVehicle Vehicle_suffix").contains(&"Vehicle".to_owned()));
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    for module in [
        "text",
        "path",
        "script",
        "inspector",
        "watch",
        "abi",
        "registry",
    ] {
        let relative = format!("src/{module}.rs");
        let text = std::fs::read_to_string(root.join(&relative))
            .unwrap_or_else(|error| panic!("cannot read {relative}: {error}"));
        assert!(!text.is_empty(), "{relative} is empty");
        for (index, line) in text.lines().enumerate() {
            for word in words(line) {
                assert!(
                    !names.contains(&word.as_str()),
                    "{relative}:{} contains fixture word {word}",
                    index + 1
                );
            }
        }
    }
}
