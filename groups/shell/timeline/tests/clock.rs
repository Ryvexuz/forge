use timeline::clock::{AudioClock, FakeAudioClock, Playhead};
use timeline::time::{Rate, Time, TimeError, TICKS_PER_SECOND};
use timeline::video::VideoDriver;

#[test]
fn playhead_audio_master_no_drift_one_hour() {
    let mut clock = FakeAudioClock::new(48000).unwrap();
    let mut playhead = Playhead::new();
    let trim = TICKS_PER_SECOND + 17;
    let video = VideoDriver::new(
        Rate {
            num: 30000,
            den: 1001,
        },
        Time(0),
        Time(3600 * TICKS_PER_SECOND + 1),
        Time(trim),
    )
    .unwrap();
    let period = TICKS_PER_SECOND / 30000 * 1001;
    let mut largest_gap = 0;
    for block in 1..=360000_i64 {
        clock.advance(480).unwrap();
        let expected = Time(block * (TICKS_PER_SECOND / 100));
        assert_eq!(clock.time().unwrap(), expected);
        assert_eq!(playhead.sync(&clock).unwrap(), expected);
        largest_gap = largest_gap.max((playhead.time().0 - clock.time().unwrap().0).abs());
        assert_eq!(
            u64::from_le_bytes(video.frame_at(playhead.time()).unwrap().pixels),
            u64::try_from((expected.0 + trim) / period).unwrap()
        );
    }
    assert_eq!(playhead.time(), Time(3600 * TICKS_PER_SECOND));
    assert_eq!(largest_gap, 0);
    println!(
        "audio-master final ticks: {}; largest gap: {largest_gap}",
        playhead.time().0
    );
}

#[test]
fn clock_errors_preserve_state_and_fractional_ticks_do_not_accumulate() {
    assert_eq!(FakeAudioClock::new(0).unwrap_err(), TimeError::BadRate);
    let mut clock = FakeAudioClock::new(44100).unwrap();
    for samples in 1..=44100_u64 {
        clock.advance(1).unwrap();
        let expected = u128::from(samples) * TICKS_PER_SECOND as u128 / 44100;
        assert_eq!(
            clock.time().unwrap(),
            Time(i64::try_from(expected).unwrap())
        );
    }
    let before = clock.time().unwrap();
    assert_eq!(clock.advance(u64::MAX), Err(TimeError::Overflow));
    assert_eq!(clock.time().unwrap(), before);
    let mut slow = FakeAudioClock::new(1).unwrap();
    assert_eq!(slow.advance(u64::MAX), Err(TimeError::Overflow));
    assert_eq!(slow.time().unwrap(), Time(0));

    struct BrokenClock(Result<Time, TimeError>);
    impl AudioClock for BrokenClock {
        fn time(&self) -> Result<Time, TimeError> {
            self.0
        }
    }
    let mut playhead = Playhead::default();
    playhead.sync(&clock).unwrap();
    for result in [Err(TimeError::BadRate), Ok(Time(-1))] {
        assert!(playhead.sync(&BrokenClock(result)).is_err());
        assert_eq!(playhead.time(), before);
    }
}
