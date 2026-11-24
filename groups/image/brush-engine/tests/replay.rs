use brush_engine::ppm;
use brush_engine::replay::{
    flatten_over_white, parse_pen_events, replay, LogError, PenKind, Replay, ReplayError,
    ReplaySettings, HEADER_LINE, SCHEMA_LINE, STROKE_V1_SETTINGS,
};

const EVENT_LOG: &str = include_str!("fixtures/event_log_v1.csv");
const STROKE: &str = include_str!("fixtures/stroke_v1.csv");
const GOLDEN: &[u8] = include_bytes!("golden/stroke_v1.ppm");
const DOWN: &str = "0,0,1,pen,down,16.000,32.000,512,0,0";

fn log(rows: &[&str]) -> String {
    let mut text = format!("{SCHEMA_LINE}\n{HEADER_LINE}\n");
    for row in rows {
        text.push_str(row);
        text.push('\n');
    }
    text
}

fn changed(column: usize, value: &str) -> String {
    let mut fields: Vec<_> = DOWN.split(',').collect();
    fields[column] = value;
    log(&[&fields.join(",")])
}

#[test]
fn event_log_fixture_keeps_only_pen_events() {
    let events = parse_pen_events(EVENT_LOG).unwrap();
    assert_eq!(events.len(), 9);
    assert_eq!(
        events.iter().map(|event| event.seq).collect::<Vec<_>>(),
        [5, 6, 7, 8, 9, 10, 13, 14, 15]
    );
    assert_eq!(
        events.len(),
        EVENT_LOG
            .lines()
            .filter(|row| row.split(',').nth(3) == Some("pen"))
            .count()
    );
    for event in &events {
        assert_eq!(event.line as u64, event.seq + 3);
    }
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            PenKind::Down,
            PenKind::Move,
            PenKind::Move,
            PenKind::Move,
            PenKind::Move,
            PenKind::Up,
            PenKind::Down,
            PenKind::Move,
            PenKind::Up,
        ]
    );
    assert_eq!(
        events[..5].iter().map(|e| e.pressure()).collect::<Vec<_>>(),
        [0.0, 0.25, 0.5, 0.75, 1.0]
    );
    let event = events[6];
    assert_eq!(event.seq, 13);
    assert_eq!((event.x, event.y), (8.125, 9.875));
    assert_eq!((event.tilt_x, event.window), (-90, 2));
    // Every valid raw pressure is exact in both precisions at this scale.
    let mut probe = events[0];
    for raw in 0..=1024 {
        probe.pressure_raw = raw;
        assert_eq!(f64::from(probe.pressure()), f64::from(raw) / 1024.0);
    }
}

#[test]
fn event_log_rejects_bad_input() {
    let reject = |text: String, error| {
        assert_eq!(parse_pen_events(&text), Err(error), "input: {text:?}");
    };
    reject(String::new(), LogError::Empty);
    reject("invalid".into(), LogError::MissingFinalNewline);
    reject(log(&[]).replace('\n', "\r\n"), LogError::BadLine { line: 1 });
    reject(log(&[""]), LogError::BadLine { line: 3 });
    reject(
        log(&[]).replace(SCHEMA_LINE, "schema=2"),
        LogError::UnknownVersion {
            line: 1,
            version: 2,
        },
    );
    for schema in ["schema 1", "schema=-1", "schema=+1", "schema=", "schema=1x"] {
        reject(log(&[]).replace(SCHEMA_LINE, schema), LogError::MissingSchema);
    }
    reject(log(&[]).replace("t_ns", "t"), LogError::BadHeader { line: 2 });
    reject("schema=1\n".into(), LogError::BadHeader { line: 2 });
    for row in [
        "0,0,1,pen,down,16.000,32.000,0,0",
        "0,0,1,pen,down,16.000,32.000,0,0,0,extra",
    ] {
        reject(log(&[row]), LogError::WrongFieldCount { line: 3 });
    }
    for column in 0..=2 {
        for value in ["01", "-1", "+1", "", "1.0", "18446744073709551616"] {
            reject(changed(column, value), LogError::BadNumber { line: 3 });
        }
    }
    reject(changed(2, "4294967296"), LogError::BadNumber { line: 3 });
    reject(changed(0, "1"), LogError::BadSequence { line: 3 });
    reject(
        log(&[DOWN, "2,0,1,pen,up,16.000,32.000,0,0,0"]),
        LogError::BadSequence { line: 4 },
    );
    reject(
        log(&[
            "0,5,1,pen,down,16.000,32.000,512,0,0",
            "1,4,1,pen,up,16.000,32.000,0,0,0",
        ]),
        LogError::TimeBackwards { line: 4 },
    );
    for (column, value) in [(3, "Pen"), (3, ""), (4, "Move"), (4, "a-b")] {
        reject(changed(column, value), LogError::BadToken { line: 3 });
    }
    for payload in ["x y", "", "x/y", "C:\\pen", "#comment", "x\u{2028}y"] {
        reject(
            log(&[&format!("0,0,1,mouse,move,{payload},0,0,0,0")]),
            LogError::BadToken { line: 3 },
        );
    }
    reject(changed(4, "hover"), LogError::UnknownPenKind { line: 3 });
    for column in [5, 6] {
        for number in [
            "NaN", "inf", "-inf", "1e3", "16.00", "16", ".000", "+1.000", "1.0000",
            " 1.000", "1.000 ", "--1.000",
        ] {
            reject(changed(column, number), LogError::BadNumber { line: 3 });
        }
        let huge = format!("{}.000", "9".repeat(100_000));
        reject(changed(column, &huge), LogError::BadNumber { line: 3 });
    }
    reject(changed(7, "1025"), LogError::PressureOutOfRange { line: 3 });
    for number in ["-1", "+1", "", "1.0", "4294967296"] {
        reject(changed(7, number), LogError::BadNumber { line: 3 });
    }
    for column in [8, 9] {
        for number in ["+1", "-", "1.0", "NaN", "2147483648", "-2147483649"] {
            reject(changed(column, number), LogError::BadNumber { line: 3 });
        }
    }
    reject(changed(8, "91"), LogError::BadTilt { line: 3 });
    reject(
        log(&["0,0,1,pen,down,16.000,32.000,0,70,70"]),
        LogError::BadTilt { line: 3 },
    );
    reject(changed(5, "1.000\r"), LogError::BadLine { line: 3 });
    // Earlier rows and columns win, even when later input is invalid too.
    reject(
        log(&["1,broken,1,pen,down,NaN,0.000,0,0,0", ""]),
        LogError::BadSequence { line: 3 },
    );
    reject(changed(0, "1").trim_end().into(), LogError::MissingFinalNewline);
    assert_eq!(parse_pen_events(&log(&[])), Ok(vec![]));
    assert!(parse_pen_events(&changed(5, "-0.125")).is_ok());
    assert!(parse_pen_events(&changed(1, "18446744073709551615")).is_ok());
    assert!(parse_pen_events(&changed(2, "4294967295")).is_ok());
    assert!(parse_pen_events(&log(&["0,0,0,mouse,move,A.z_-09,0,0,0,0"])).is_ok());
}

#[test]
fn replay_refuses_broken_strokes() {
    let cases = [
        (
            vec!["0,0,1,pen,move,16.000,32.000,0,0,0"],
            ReplayError::NoPenDown { line: 3 },
        ),
        (
            vec!["0,0,1,pen,up,16.000,32.000,0,0,0"],
            ReplayError::NoPenDown { line: 3 },
        ),
        (
            vec![DOWN, "1,0,1,pen,down,16.000,32.000,512,0,0"],
            ReplayError::DoubleDown { line: 4 },
        ),
        (
            vec![DOWN, "1,0,1,pen,move,16.000,32.000,512,0,0"],
            ReplayError::Unterminated { line: 4 },
        ),
    ];
    for (rows, error) in cases {
        let events = parse_pen_events(&log(&rows)).unwrap();
        assert_eq!(replay(&events, &STROKE_V1_SETTINGS), Err(error));
    }
    for (width, height, hardness) in [
        (0, 64, 0.5),
        (160, 0, 0.5),
        (usize::MAX, 2, 0.5),
        (usize::MAX / 4, 1, 0.5),
        (160, 64, 1.5),
        (160, 64, f64::NAN),
        (160, 64, f64::INFINITY),
        (160, 64, -0.5),
    ] {
        let settings = ReplaySettings {
            width,
            height,
            hardness,
            ..STROKE_V1_SETTINGS
        };
        assert_eq!(replay(&[], &settings), Err(ReplayError::BadSettings));
    }
    let empty = parse_pen_events(&log(&[])).unwrap();
    let result = replay(&empty, &STROKE_V1_SETTINGS).unwrap();
    assert_eq!(result.frames, 0);
    assert_eq!(result.pixels.len(), 160 * 64 * 4);
    assert!(result.pixels.iter().all(|&value| value == 0));
    assert!(flatten_over_white(&result).rgb.iter().all(|&v| v == 255));

    let mut events = parse_pen_events(&log(&[DOWN, "1,0,1,pen,up,16.000,32.000,0,0,0"])).unwrap();
    events[0].pressure_raw = 1025;
    assert_eq!(
        replay(&events, &STROKE_V1_SETTINGS),
        Err(ReplayError::Pressure {
            line: 3,
            error: brush_engine::pressure::DynamicsError::InvalidPressure,
        })
    );
    events[0].pressure_raw = 512;
    for x in [f64::NAN, f64::INFINITY, f64::MAX] {
        events[0].x = x;
        let error = if x.is_finite() {
            brush_engine::stabilizer::StabilizerError::CoordinateOutOfRange
        } else {
            brush_engine::stabilizer::StabilizerError::NonFiniteCoordinate
        };
        assert_eq!(
            replay(&events, &STROKE_V1_SETTINGS),
            Err(ReplayError::Stabilizer { line: 3, error })
        );
    }
    // A flush uses the last down/move pressure, not the up row's pressure.
    events[0].x = 16.0;
    events[1].pressure_raw = 1024;
    let result = replay(&events, &STROKE_V1_SETTINGS).unwrap();
    let center = (32 * result.width + 16) * 4;
    // Two half-opacity dabs: first alpha 128, then round(127.5 + 64) = 192.
    assert_eq!(&result.pixels[center..center + 4], [0, 0, 0, 192]);
}

#[test]
fn stroke_fixture_follows_its_recipe() {
    let events = parse_pen_events(STROKE).unwrap();
    assert_eq!(events.len(), 50);
    for (i, event) in events.iter().enumerate() {
        assert_eq!(event.seq, i as u64);
        assert_eq!(event.line, i + 3);
        assert_eq!(event.window, 1);
        assert_eq!((event.tilt_x, event.tilt_y), (0, 0));
        assert_eq!(event.t_ns, i as u64 * 5_000_000);
        if i < 49 {
            assert_eq!(event.kind, if i == 0 { PenKind::Down } else { PenKind::Move });
            assert_eq!((event.x, event.y), ((16 + 2 * i) as f64, 32.0));
            assert_eq!(event.pressure_raw, ((512 * i + 24) / 48) as u32);
        } else {
            assert_eq!(event.kind, PenKind::Up);
            assert_eq!((event.x, event.y, event.pressure_raw), (112.0, 32.0, 0));
        }
    }
}

#[test]
fn stroke_replay_matches_golden() {
    let events = parse_pen_events(STROKE).unwrap();
    let mut previous = None;
    for _ in 0..2 {
        let result = replay(&events, &STROKE_V1_SETTINGS).unwrap();
        assert_eq!(result.frames, 50);
        let mut bytes = Vec::new();
        ppm::write(&mut bytes, &flatten_over_white(&result)).unwrap();
        assert_eq!(bytes.as_slice(), GOLDEN);
        if let Some(previous) = &previous {
            assert_eq!(&bytes, previous);
        }
        previous = Some(bytes);
    }
}

#[test]
fn stroke_golden_has_independent_properties() {
    let image = ppm::read(GOLDEN).unwrap();
    assert_eq!((image.width, image.height), (160, 64));
    let pixel = |x, y| &image.rgb[(y * image.width + x) * 3..][..3];
    // The recipe bounds diameter by 32.5 and fixes the line at y = 32.
    for y in 0..image.height {
        for x in 0..image.width {
            if y <= 15 || y >= 49 || x >= 129 {
                assert_eq!(pixel(x, y), [255; 3], "background ({x}, {y})");
            }
        }
    }
    for x in 32..=112 {
        assert_ne!(pixel(x, 32), [255; 3], "ink ({x}, 32)");
    }
    let brightness = |x| pixel(x, 32).iter().map(|&c| u32::from(c)).sum::<u32>();
    assert!(brightness(100) < brightness(40));
    for k in 1..=16 {
        for x in 0..image.width {
            for channel in 0..3 {
                assert!(
                    pixel(x, 32 - k)[channel].abs_diff(pixel(x, 32 + k)[channel]) <= 1,
                    "symmetry x={x} k={k} channel={channel}"
                );
            }
        }
    }
}

#[test]
fn flatten_is_exact_over_white() {
    let mut result = Replay {
        pixels: vec![0, 0, 0, 255, 10, 20, 30, 0],
        width: 2,
        height: 1,
        frames: 0,
    };
    assert_eq!(flatten_over_white(&result).rgb, [0, 0, 0, 255, 255, 255]);
    result.width = 1;
    result.pixels = vec![0, 100, 200, 128];
    assert_eq!(flatten_over_white(&result).rgb, [127, 177, 227]);
    // (1*128 + 255*127 + 127)/255 = 128; without rounding it is 127.
    result.pixels = vec![1, 1, 1, 128];
    assert_eq!(flatten_over_white(&result).rgb, [128; 3]);
}

#[test]
#[ignore]
fn write_stroke_golden() {
    let events = parse_pen_events(STROKE).unwrap();
    let result = replay(&events, &STROKE_V1_SETTINGS).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/stroke_v1.ppm");
    let output = std::fs::File::create(path).unwrap();
    ppm::write(output, &flatten_over_white(&result)).unwrap();
}
