use timeline::time::{Rate, Time, TimeError, TICKS_PER_SECOND};

#[test]
fn time_conversions_exact_for_all_rates() {
    // Independent integer tick counts, not values produced by the conversion.
    let frames = [
        (24, 1, 10_584_000_000),
        (25, 1, 10_160_640_000),
        (30, 1, 8_467_200_000),
        (50, 1, 5_080_320_000),
        (60, 1, 4_233_600_000),
        (24_000, 1_001, 10_594_584_000),
        (30_000, 1_001, 8_475_667_200),
        (60_000, 1_001, 4_237_833_600),
    ];
    assert_eq!(TICKS_PER_SECOND, 254_016_000_000);
    for (num, den, ticks) in frames {
        let rate = Rate::new(num, den).unwrap();
        for index in 0..=600 * num / den {
            let expected = Time(index * ticks);
            assert_eq!(Time::from_frames(index, rate), Ok(expected));
            assert_eq!(expected.to_frames(rate), Ok(index));
            assert_eq!(Time::from_frames(-index, rate), Ok(Time(-expected.0)));
            assert_eq!(Time(-expected.0).to_frames(rate), Ok(-index));
        }
        // Off-grid ticks must truncate, not round to the closest frame.
        assert_eq!(Time(ticks - 1).to_frames(rate), Ok(0));
        assert_eq!(Time(1 - ticks).to_frames(rate), Ok(0));
    }
    for (rate, ticks) in [(44_100, 5_760_000), (48_000, 5_292_000)] {
        for index in 0..=600 * rate {
            let expected = Time(index * ticks);
            assert_eq!(Time::from_samples(index, rate), Ok(expected));
            assert_eq!(expected.to_samples(rate), Ok(index));
            assert_eq!(Time::from_samples(-index, rate), Ok(Time(-expected.0)));
            assert_eq!(Time(-expected.0).to_samples(rate), Ok(-index));
        }
        assert_eq!(Time(ticks - 1).to_samples(rate), Ok(0));
        assert_eq!(Time(1 - ticks).to_samples(rate), Ok(0));
    }
}

#[test]
fn time_overflow_is_named_error() {
    let overflow = Err(TimeError::Overflow);
    assert_eq!(Time(i64::MAX).checked_add(Time(1)), overflow);
    assert_eq!(Time(i64::MIN).checked_add(Time(-1)), overflow);
    assert_eq!(Time(i64::MIN).checked_sub(Time(1)), overflow);
    assert_eq!(Time(i64::MAX).checked_sub(Time(-1)), overflow);
    assert_eq!(Time(i64::MAX).checked_mul(2), overflow);
    assert_eq!(Time(i64::MIN).checked_mul(-1), overflow);
    assert_eq!(Time(i64::MAX).checked_add(Time(0)), Ok(Time(i64::MAX)));
    assert_eq!(Time(i64::MIN).checked_sub(Time(0)), Ok(Time(i64::MIN)));
    assert_eq!(Time(i64::MIN).checked_mul(1), Ok(Time(i64::MIN)));
    assert_eq!(Time(i64::MAX).checked_mul(0), Ok(Time(0)));
    assert_eq!(Time(7).checked_sub(Time(9)), Ok(Time(-2)));
    assert_eq!(Time(-7).checked_mul(-3), Ok(Time(21)));

    for (num, den) in [
        (0, 1),
        (-1, 1),
        (1, 0),
        (1, -1),
        (i64::MAX, 1),
        (1, i64::MAX),
        (11, 1),
        (1, i64::from(u32::MAX)),
    ] {
        assert_eq!(Rate::new(num, den), Err(TimeError::BadRate));
    }
    // Public fields cannot bypass validation at either conversion entry point.
    for rate in [
        Rate { num: 0, den: 1 },
        Rate { num: 1, den: 0 },
        Rate { num: 11, den: 1 },
        Rate {
            num: 1,
            den: u32::MAX,
        },
    ] {
        assert_eq!(Time::from_frames(0, rate), Err(TimeError::BadRate));
        assert_eq!(Time(0).to_frames(rate), Err(TimeError::BadRate));
    }
    for rate in [0, -1, 11, i64::MAX] {
        assert_eq!(Time::from_samples(0, rate), Err(TimeError::BadRate));
        assert_eq!(Time(0).to_samples(rate), Err(TimeError::BadRate));
    }
    let rate = Rate::new(24, 1).unwrap();
    for index in [i64::MIN, i64::MAX] {
        assert_eq!(Time::from_frames(index, rate), overflow);
        assert_eq!(Time::from_samples(index, 48_000), overflow);
    }
    // Independent i128 arithmetic checks the last representable frame on each side.
    let fine_rate = Rate::new(2_032_128_000, 1).unwrap();
    for limit in [i64::MIN, i64::MAX] {
        let index = limit / 125;
        let expected = i64::try_from(i128::from(index) * 125).unwrap();
        assert_eq!(Time::from_frames(index, fine_rate), Ok(Time(expected)));
        assert_eq!(Time(limit).to_frames(fine_rate), Ok(index));
        let beyond = index + if limit < 0 { -1 } else { 1 };
        assert_eq!(Time::from_frames(beyond, fine_rate), overflow);
    }
    let equivalent = Rate::new(48, 2).unwrap();
    assert_eq!(Time::from_frames(1, equivalent), Ok(Time(10_584_000_000)));
}
