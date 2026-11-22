use raster::blend;

type Mode = fn(f32, f32) -> f32;

const MODES: [(&str, Mode); 12] = [
    ("normal", blend::normal),
    ("multiply", blend::multiply),
    ("screen", blend::screen),
    ("overlay", blend::overlay),
    ("darken", blend::darken),
    ("lighten", blend::lighten),
    ("color-dodge", blend::color_dodge),
    ("color-burn", blend::color_burn),
    ("hard-light", blend::hard_light),
    ("soft-light", blend::soft_light),
    ("difference", blend::difference),
    ("exclusion", blend::exclusion),
];

const TOLERANCE: f64 = 1e-6;

#[test]
fn blend_modes_match_w3c_reference() {
    let mut seen = [false; 12];
    for (line_index, line) in include_str!("fixtures/blend_reference.txt")
        .lines()
        .enumerate()
    {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        assert_eq!(fields.len(), 4, "fixture line {}", line_index + 1);
        let index = MODES
            .iter()
            .position(|(name, _)| *name == fields[0])
            .expect("unknown fixture mode");
        let parse = |field: &str| {
            let value: f32 = field.parse().expect("invalid fixture number");
            assert!(value.is_finite() && (0.0..=1.0).contains(&value));
            value
        };
        let backdrop = parse(fields[1]);
        let source = parse(fields[2]);
        let expected = parse(fields[3]);
        let actual = MODES[index].1(backdrop, source);
        assert!(
            actual.is_finite() && (f64::from(actual) - f64::from(expected)).abs() <= TOLERANCE,
            "{}({backdrop}, {source}): expected {expected}, actual {actual}",
            fields[0]
        );
        seen[index] = true;
    }
    assert!(seen.iter().all(|seen| *seen), "missing fixture mode");
}

#[test]
fn premultiplied_source_over() {
    let backdrop = [0.25, 0.125, 0.0, 0.5];
    let source = [0.125, 0.0, 0.25, 0.25];
    // source + backdrop * (1 - 1/4), for each RGBA component.
    assert_eq!(
        blend::source_over(backdrop, source),
        [0.3125, 0.09375, 0.25, 0.625]
    );
    assert_eq!(blend::source_over(backdrop, [0.0; 4]), backdrop);
    assert_eq!(blend::source_over(backdrop, [1.0; 4]), [1.0; 4]);
    assert_eq!(blend::source_over([0.0; 4], source), source);
}

// Evaluate the specification independently in f64, not through the f32 API.
fn reference(name: &str, b: f64, s: f64) -> f64 {
    match name {
        "normal" => s,
        "multiply" => b * s,
        "screen" => 1.0 - (1.0 - b) * (1.0 - s),
        "overlay" if b <= 0.5 => 2.0 * b * s,
        "overlay" => 1.0 - 2.0 * (1.0 - b) * (1.0 - s),
        "darken" => b.min(s),
        "lighten" => b.max(s),
        "color-dodge" if b == 0.0 => 0.0,
        "color-dodge" if s == 1.0 => 1.0,
        "color-dodge" => (b / (1.0 - s)).min(1.0),
        "color-burn" if b == 1.0 => 1.0,
        "color-burn" if s == 0.0 => 0.0,
        "color-burn" => 1.0 - ((1.0 - b) / s).min(1.0),
        "hard-light" if s <= 0.5 => 2.0 * b * s,
        "hard-light" => 1.0 - 2.0 * (1.0 - b) * (1.0 - s),
        "soft-light" if s <= 0.5 => b - (1.0 - 2.0 * s) * b * (1.0 - b),
        "soft-light" => {
            let d = if b <= 0.25 {
                16.0 * b.powi(3) - 12.0 * b.powi(2) + 4.0 * b
            } else {
                b.sqrt()
            };
            b + (2.0 * s - 1.0) * (d - b)
        }
        "difference" => (b - s).abs(),
        "exclusion" => b * (1.0 - s) + s * (1.0 - b),
        _ => panic!("unknown mode {name}"),
    }
}

#[test]
fn numeric_domain_edges_against_f64() {
    let values = [
        0.0,
        f32::from_bits(1),
        f32::MIN_POSITIVE,
        0.25,
        f32::from_bits(0.5_f32.to_bits() - 1),
        0.5,
        f32::from_bits(0.5_f32.to_bits() + 1),
        f32::from_bits(1.0_f32.to_bits() - 1),
        1.0,
    ];
    for (name, mode) in MODES {
        for b in values {
            for s in values {
                let expected = reference(name, f64::from(b), f64::from(s));
                let actual = mode(b, s);
                assert!(
                    actual.is_finite() && (f64::from(actual) - expected).abs() <= TOLERANCE,
                    "{name}({b}, {s}): expected {expected}, actual {actual}"
                );
            }
        }
    }
    for alpha in values {
        for channel in values {
            let b = channel.min(alpha);
            let backdrop = [b, b, b, alpha];
            let source = [b, b, b, alpha];
            let actual = blend::source_over(backdrop, source);
            for i in 0..4 {
                let expected =
                    f64::from(source[i]) + f64::from(backdrop[i]) * (1.0 - f64::from(alpha));
                assert!(
                    actual[i].is_finite() && (f64::from(actual[i]) - expected).abs() <= TOLERANCE
                );
            }
        }
    }
}
