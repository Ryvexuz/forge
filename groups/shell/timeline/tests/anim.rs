use timeline::anim::{AnimationDriver, AnimationSink};
use timeline::driver::{DriverError, TrackDriver};
use timeline::time::{Rate, Time, TICKS_PER_SECOND};
use timeline::track::{Channel, Clip, Interp, Keyframe};

fn key(ticks: i64, value: f64, interp: Interp) -> Keyframe {
    Keyframe {
        time: Time(ticks),
        value,
        interp,
        handles: Some([(1.0, 0.0), (1.0, 0.0)]),
    }
}

fn driver() -> AnimationDriver {
    let duration = 5 * TICKS_PER_SECOND;
    AnimationDriver::new(
        Clip {
            start: Time(0),
            len: Time(duration),
            source: String::new(),
            source_offset: Time(0),
            name: "car".into(),
        },
        vec![
            Channel {
                target: "car/position_x".into(),
                keys: vec![
                    key(0, 0.0, Interp::Linear),
                    key(duration, 10.0, Interp::Linear),
                ],
            },
            Channel {
                target: "car/wheel_fl/rotation_y".into(),
                keys: vec![
                    key(0, 0.0, Interp::Step),
                    key(duration / 2, 90.0, Interp::Step),
                    key(duration, 180.0, Interp::Step),
                ],
            },
            Channel {
                target: "car/scale".into(),
                keys: vec![
                    key(0, 1.0, Interp::Hermite),
                    key(duration, 2.0, Interp::Hermite),
                ],
            },
        ],
    )
    .unwrap()
}

#[derive(Default)]
struct Sink(Vec<(String, f64)>);

impl AnimationSink for Sink {
    fn write(&mut self, path: &str, value: f64) {
        self.0.push((path.to_owned(), value));
    }
}

#[test]
fn anim_track_matches_independent_golden() {
    let driver = driver();
    let rate = Rate::new(30, 1).unwrap();
    let mut lines = include_str!("fixtures/anim_golden.csv").lines();
    assert_eq!(lines.next(), Some("frame,position,rotation,scale"));
    let rows: Vec<_> = lines.collect();
    assert_eq!(rows.len(), 150);
    for (frame, row) in rows.iter().enumerate() {
        let fields: Vec<_> = row.split(',').collect();
        assert_eq!(fields.len(), 4);
        assert_eq!(fields[0].parse::<usize>().unwrap(), frame);
        let start = Time::from_frames(frame as i64, rate).unwrap();
        let end = Time::from_frames(frame as i64 + 1, rate).unwrap();
        let mut sink = Sink::default();
        driver.write_to(start..end, &mut sink).unwrap();
        assert_eq!(sink.0.len(), 3);
        for (index, path) in ["car/position_x", "car/wheel_fl/rotation_y", "car/scale"]
            .iter()
            .enumerate()
        {
            let expected = fields[index + 1].parse::<f64>().unwrap();
            let (actual_path, actual) = &sink.0[index];
            assert_eq!(actual_path.as_str(), *path);
            assert!(expected.is_finite());
            assert!(
                (actual - expected).abs() <= 1e-9,
                "frame={frame} path={path} actual={actual} expected={expected}"
            );
        }
    }
}

#[test]
fn lower_level_validation_preserves_output() {
    let driver = driver();
    let mut output = [42.0; 3];
    for range in [
        Time(2)..Time(1),
        Time(0)..Time(0),
        Time(-1)..Time(1),
        Time(0)..Time(i64::MAX),
    ] {
        assert_eq!(
            driver.evaluate(range, &mut output),
            Err(DriverError::InvalidRange)
        );
        assert_eq!(output, [42.0; 3]);
    }
    assert_eq!(
        driver.evaluate(Time(0)..Time(1), &mut output[..2]),
        Err(DriverError::BufferTooSmall)
    );
    assert_eq!(output, [42.0; 3]);
}
