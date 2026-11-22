use timeline::driver::{DriverError, TrackDriver};
use timeline::time::{Rate, Time, TimeError, TICKS_PER_SECOND};
use timeline::video::VideoDriver;

#[test]
fn video_frame_selection_exact_all_rates() {
    let ticks = TICKS_PER_SECOND;
    for (num, den) in [(24, 1), (25, 1), (30000, 1001), (60, 1)] {
        let period = ticks * i64::from(den) / i64::from(num);
        assert_eq!(period * i64::from(num), ticks * i64::from(den));
        for trim in [0, period - 1, period, period + 1, 7 * period + 13] {
            let start = 11 * ticks;
            let len = 600 * ticks;
            let driver =
                VideoDriver::new(Rate { num, den }, Time(start), Time(len), Time(trim)).unwrap();
            // All supported source rates differ from this display rate.
            let display_period = ticks / 30;
            for frame in 0..18000 {
                let offset = frame * display_period;
                // Independent reference: interval division by exact source period.
                let expected = u64::try_from((trim + offset) / period).unwrap();
                let image = driver.frame_at(Time(start + offset)).unwrap();
                assert_eq!(u64::from_le_bytes(image.pixels), expected);
            }
            for offset in [0, len - 1] {
                assert_eq!(
                    u64::from_le_bytes(driver.frame_at(Time(start + offset)).unwrap().pixels),
                    u64::try_from((trim + offset) / period).unwrap()
                );
            }
            let boundary = 10 * period - trim;
            if boundary > 0 && boundary + 1 < len {
                for delta in [-1, 0, 1] {
                    let image = driver.frame_at(Time(start + boundary + delta)).unwrap();
                    assert_eq!(
                        u64::from_le_bytes(image.pixels),
                        if delta < 0 { 9 } else { 10 }
                    );
                }
            }
            assert_eq!(
                driver.frame_at(Time(start + len)),
                Err(DriverError::InvalidRange)
            );
        }
    }
}

#[test]
fn video_driver_validates_before_writing() {
    let rate = Rate { num: 25, den: 1 };
    let driver = VideoDriver::new(rate, Time(0), Time(100), Time(0)).unwrap();
    let mut output = [123.0; 8];
    assert_eq!(
        driver.evaluate(Time(2)..Time(1), &mut output),
        Err(DriverError::InvalidRange)
    );
    assert_eq!(
        driver.evaluate(Time(0)..Time(2), &mut output),
        Err(DriverError::BufferTooSmall)
    );
    assert_eq!(output, [123.0; 8]);
    driver.evaluate(Time(0)..Time(1), &mut output).unwrap();
    assert_eq!(output, [0.0; 8]);
    assert_eq!(driver.kind(), "video");
    for (num, den) in [(0, 1), (1, 0)] {
        assert_eq!(
            VideoDriver::new(Rate { num, den }, Time(0), Time(1), Time(0)).unwrap_err(),
            TimeError::BadRate
        );
    }
    for (start, len, trim) in [
        (-1, 1, 0),
        (0, 0, 0),
        (0, 1, -1),
        (i64::MAX, 1, 0),
        (0, 2, i64::MAX),
    ] {
        assert!(VideoDriver::new(rate, Time(start), Time(len), Time(trim)).is_err());
    }
    let driver = VideoDriver::new(
        Rate {
            num: u32::MAX,
            den: 1,
        },
        Time(0),
        Time(i64::MAX),
        Time(0),
    )
    .unwrap();
    let expected =
        ((i128::from(i64::MAX) - 1) * i128::from(u32::MAX)) / i128::from(TICKS_PER_SECOND);
    assert_eq!(
        u64::from_le_bytes(driver.frame_at(Time(i64::MAX - 1)).unwrap().pixels),
        u64::try_from(expected).unwrap()
    );
}
