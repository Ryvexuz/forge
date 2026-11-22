use timeline::audio::{tone, AudioDriver};
use timeline::driver::{DriverError, TrackDriver};
use timeline::time::{Time, TICKS_PER_SECOND};

const SAMPLE_TICKS: i64 = TICKS_PER_SECOND / 48_000;

fn at(sample: i64) -> Time {
    Time(sample * SAMPLE_TICKS)
}

fn fading_timeline() -> AudioDriver {
    let mut driver = AudioDriver::new(48_000).unwrap();
    driver
        .add_clip(at(0)..at(220), tone(1000.0, 0.5).unwrap(), 16, 32)
        .unwrap();
    driver
        .add_clip(at(37)..at(256), tone(375.0, -0.25).unwrap(), 23, 41)
        .unwrap();
    driver
}

#[test]
fn audio_clip_boundaries_sample_accurate() {
    let mut driver = AudioDriver::new(48_000).unwrap();
    driver
        .add_clip(at(12_000)..at(24_000), tone(1000.0, 1.0).unwrap(), 0, 0)
        .unwrap();
    let mut output = vec![0.0; 48_000];
    driver.evaluate(at(0)..at(48_000), &mut output).unwrap();
    assert_eq!(output[11_999], 0.0);
    assert_eq!(output[12_000], 0.0); // First sine sample.
    assert!((output[12_001] - (std::f64::consts::TAU / 48.0).sin()).abs() <= 1e-6);
    assert!(output[23_999].abs() > 1e-6);
    assert_eq!(output[24_000], 0.0);

    // Round half down, including negative ties. A fade-free sine clip
    // starts with zero, so inspect its second sample to observe placement.
    for base in [-10, 10] {
        for (fraction, rounded) in [
            (SAMPLE_TICKS / 2, base),
            (SAMPLE_TICKS / 2 + 1, base + 1),
            (1, base),
        ] {
            let mut driver = AudioDriver::new(48_000).unwrap();
            driver
                .add_clip(
                    Time(at(base).0 + fraction)..Time(at(base + 3).0 + fraction),
                    tone(12_000.0, 1.0).unwrap(),
                    0,
                    0,
                )
                .unwrap();
            let mut buffer = [0.0; 5];
            driver.render(rounded - 1, &mut buffer).unwrap();
            assert_eq!(buffer[0], 0.0);
            assert_eq!(buffer[1], 0.0);
            assert!((buffer[2] - 1.0).abs() <= 1e-6);
            assert_eq!(buffer[4], 0.0);
        }
    }
}

#[test]
fn audio_block_size_independent() {
    let driver = fading_timeline();
    let mut reference = vec![0.0; 4800];
    driver.render(-50, &mut reference).unwrap();
    for size in [1, 64, 480, 1000, 4096] {
        let mut output = vec![0.0; reference.len()];
        for (block, buffer) in output.chunks_mut(size).enumerate() {
            driver.render(-50 + (block * size) as i64, buffer).unwrap();
        }
        for (index, (actual, expected)) in output.iter().zip(&reference).enumerate() {
            assert_eq!(
                (*actual as f32).to_bits(),
                (*expected as f32).to_bits(),
                "block {size}, sample {index}"
            );
            assert_eq!(actual.to_bits(), expected.to_bits());
        }
    }
}

#[test]
fn audio_matches_independent_golden() {
    let bytes = include_bytes!("fixtures/audio_golden.bin");
    assert_eq!(bytes.len(), 256 * 4);
    let mut output = [0.0; 256];
    fading_timeline().render(0, &mut output).unwrap();
    for (index, (actual, bytes)) in output.iter().zip(bytes.as_chunks::<4>().0).enumerate() {
        let expected = f32::from_le_bytes(*bytes);
        assert!(
            (actual - f64::from(expected)).abs() <= 1e-6,
            "sample {index}: actual={actual}, golden={expected}"
        );
    }
}

#[test]
fn audio_rejects_invalid_inputs_without_writes() {
    assert!(AudioDriver::new(0).is_err());
    for frequency in [f64::NAN, f64::INFINITY, -1.0, f64::NEG_INFINITY] {
        assert!(tone(frequency, 1.0).is_err());
    }
    for amplitude in [f64::NAN, f64::INFINITY, -1.01, 1.01] {
        assert!(tone(1.0, amplitude).is_err());
    }
    let mut driver = AudioDriver::new(48_000).unwrap();
    assert!(driver
        .add_clip(at(0)..at(2), tone(24_001.0, 1.0).unwrap(), 0, 0)
        .is_err());
    assert!(driver
        .add_clip(at(0)..at(2), tone(1.0, 1.0).unwrap(), 3, 0)
        .is_err());
    let mut buffer = [42.0; 2];
    assert_eq!(
        driver.evaluate(at(2)..at(1), &mut buffer),
        Err(DriverError::InvalidRange)
    );
    assert_eq!(
        driver.evaluate(at(0)..at(3), &mut buffer),
        Err(DriverError::BufferTooSmall)
    );
    assert_eq!(
        driver.render(i64::MAX, &mut buffer),
        Err(DriverError::InvalidRange)
    );
    assert_eq!(buffer, [42.0; 2]);
    for amplitude in [f64::from_bits(1), f64::MIN_POSITIVE, -1.0, 1.0] {
        let mut driver = AudioDriver::new(u32::MAX).unwrap();
        driver
            .add_clip(
                Time(i64::MIN)..Time(i64::MAX),
                tone(f64::from(u32::MAX) / 2.0, amplitude).unwrap(),
                0,
                0,
            )
            .unwrap();
        driver.render(0, &mut buffer).unwrap();
        assert!(buffer.iter().all(|value| value.is_finite()));
    }
}
