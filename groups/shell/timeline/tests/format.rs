use timeline::format::{migrate, parse, write, FormatError};

const GOLDEN: &str = include_str!("fixtures/timeline_v1.tl");

#[test]
fn format_v1_fixture_round_trip_byte_identical() {
    assert_eq!(write(&parse(GOLDEN).unwrap()), GOLDEN);
}

#[test]
fn format_old_version_loads_after_schema_change() {
    // Test-only v2 adds a muted flag with default false to the entire timeline.
    #[derive(Debug, PartialEq)]
    struct V2 {
        timeline: timeline::track::Timeline,
        muted: bool,
    }
    fn load_old(version: u32, text: &str) -> Result<V2, FormatError> {
        Ok(V2 {
            timeline: migrate(version, text)?,
            muted: false,
        })
    }
    let old = "timeline\t1\ntrack\t9\taudio\tVoice\n";
    let expected = V2 {
        timeline: timeline::track::Timeline {
            tracks: vec![timeline::track::Track {
                id: 9,
                kind: "audio".into(),
                name: "Voice".into(),
                clips: vec![],
                channels: vec![],
            }],
        },
        muted: false,
    };
    assert_eq!(load_old(1, old).unwrap(), expected);
    assert_eq!(migrate(1, GOLDEN), parse(GOLDEN));
    assert_eq!(migrate(2, old), Err(FormatError::UnknownVersion));
}

#[test]
fn format_rejects_bad_input() {
    let cases = [
        ("timeline\t2\n", FormatError::UnknownVersion),
        ("track\t1\taudio\tA\n", FormatError::MissingHeader),
        ("timeline\t1\ntrack\t1\n", FormatError::WrongColumnCount),
        (
            "timeline\t1\ntrack\t1\taudio\tA\r\n",
            FormatError::InvalidText,
        ),
        (
            "timeline\t1\ntrack\t1\taudio\tA\u{0001}\n",
            FormatError::InvalidText,
        ),
        (
            "timeline\t1\nclip\t9\t0\t1\ta.wav\t0\tA\n",
            FormatError::MissingTrack,
        ),
        (
            "timeline\t1\ntrack\t1\taudio\tA",
            FormatError::TruncatedRecord,
        ),
        ("timeline\t1\ntrack\t1\taudio\t\n", FormatError::InvalidText),
        (
            "timeline\t1\ntrack\t1\taudio\tA\ntrack\t1\taudio\tB\n",
            FormatError::DuplicateTrack,
        ),
    ];
    assert_eq!(
        parse(
            "timeline	1
track	1
foo"
        ),
        Err(FormatError::WrongColumnCount)
    );
    for (input, error) in cases {
        assert_eq!(parse(input), Err(error), "{input:?}");
    }
    for bad in ["NaN", "inf", "-inf", "1e999"] {
        let input = GOLDEN.replace("1.2345678901234567", bad);
        assert_eq!(parse(&input), Err(FormatError::NonFinite));
    }
    for bad in [
        format!("{}:/music.wav", 'C'),
        "media\\music.wav".to_string(),
    ] {
        let input = GOLDEN.replace("media/music.wav", &bad);
        assert_eq!(parse(&input), Err(FormatError::InvalidPath));
    }
    for bad in ["9223372036854775808", "not-a-number"] {
        assert_eq!(
            parse(&GOLDEN.replace("\t-10\t", &format!("\t{bad}\t"))),
            Err(FormatError::InvalidNumber)
        );
    }
    for value in [f64::from_bits(1), f64::MIN_POSITIVE, f64::MAX, -f64::MAX] {
        let input = GOLDEN.replace("1.2345678901234567", &value.to_string());
        let parsed = parse(&input).unwrap();
        assert_eq!(parsed.tracks[2].channels[0].keys[0].value, value);
        assert_eq!(parse(&write(&parsed)).unwrap(), parsed);
    }
}
