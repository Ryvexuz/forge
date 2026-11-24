//! Times full playhead evaluation of every track, including clip lookup.
//! Timing on a busy board means nothing: use a quiet board for budget checks.
//! The plan's 2,000 us budget is a placeholder, not a published benchmark.
//! R5 requires binary search over sorted clips, not a timed linear scan.
//! AnimationDriver::evaluate allocates one Vec per call; this is measured and
//! allowed. Named follow-up: allocation-free evaluate path. The benchmark itself
//! allocates no storage during successful timed evaluations.

use std::hint::black_box;
use std::time::Instant;
use timeline::anim::AnimationDriver;
use timeline::audio::{tone, AudioDriver};
use timeline::driver::{DriverError, TrackDriver};
use timeline::time::{Rate, Time, TICKS_PER_SECOND};
use timeline::track::{Channel, Clip, Interp, Keyframe};
use timeline::video::VideoDriver;

const TRACKS: usize = 100; // Plan stress workload, cycling through three kinds.
const STEPS: u64 = 10_000; // Plan's evenly spaced scrub samples.
const L: i64 = TICKS_PER_SECOND / 4; // Quarter-second contiguous clips.
const SAMPLE_RATE: u32 = 48_000; // Plan's audio sampling cadence.
const AUDIO_SAMPLES: i64 = 480; // One 10 ms audio block.
const AUDIO_BUFFER: usize = AUDIO_SAMPLES as usize + 2; // Plan's reused buffer.
const TONE_HZ: f64 = 440.0; // Deterministic synthetic audio source.
const AMPLITUDE: f64 = 0.5; // Plan's bounded sine amplitude.
const VIDEO_FPS: i64 = 24; // Plan's synthetic video frame rate.
const MAX_CLIPS: u64 = 10_000_000; // Plan's accepted stress-workload ceiling.
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

#[derive(Debug, PartialEq, Eq)]
struct Args {
    clips: u64,
    p99_max_us: u64,
}

fn validate_clips(clips: u64) -> Result<(), String> {
    if !(TRACKS as u64..=MAX_CLIPS).contains(&clips) || !clips.is_multiple_of(TRACKS as u64) {
        return Err(format!(
            "clips must be a multiple of {TRACKS} in {TRACKS}..={MAX_CLIPS}"
        ));
    }
    Ok(())
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut clips, mut budget) = (None, None);
    let mut i = 0;
    while i < args.len() {
        let flag = &args[i];
        let slot = match flag.as_str() {
            "--clips" => &mut clips,
            "--p99-max-us" => &mut budget,
            _ => return Err(format!("unknown flag {flag:?}")),
        };
        if slot.is_some() {
            return Err(format!("repeated flag {flag}"));
        }
        let value = args
            .get(i + 1)
            .ok_or_else(|| format!("missing value for {flag}"))?;
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("{flag} requires ASCII decimal digits"));
        }
        *slot = Some(
            value
                .parse::<u64>()
                .map_err(|_| format!("{flag} overflows u64"))?,
        );
        i += 2;
    }
    let clips = clips.ok_or("missing --clips")?;
    let p99_max_us = budget.ok_or("missing --p99-max-us")?;
    validate_clips(clips)?;
    if p99_max_us == 0 {
        return Err("--p99-max-us must be positive".into());
    }
    Ok(Args { clips, p99_max_us })
}

enum Driver {
    Animation(AnimationDriver),
    Audio(AudioDriver),
    Video(VideoDriver),
}

struct Track {
    clips: Vec<Clip>,
    drivers: Vec<Driver>,
}

struct Bench {
    tracks: Vec<Track>,
    span: i64,
    audio_ticks: i64,
}

#[derive(Debug)]
enum Values<'a> {
    Animation(f64),
    Audio(&'a [f64]),
    Video(u64),
}

fn clip_at(clips: &[Clip], p: Time) -> Option<usize> {
    let i = clips.partition_point(|c| c.start.0 <= p.0).checked_sub(1)?;
    let clip = &clips[i];
    (i128::from(p.0) < i128::from(clip.start.0) + i128::from(clip.len.0)).then_some(i)
}

fn hash_word(mut hash: u64, word: u64) -> u64 {
    for byte in word.to_le_bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn build_error(error: impl std::fmt::Debug) -> String {
    format!("scrub_bench: FAIL build {error:?}")
}

impl Bench {
    fn build(clips: u64) -> Result<Self, String> {
        validate_clips(clips)?;
        // The validated clip ceiling keeps the span and all clip ends in i64.
        let c = (clips / TRACKS as u64) as i64;
        let audio_ticks = Time::from_samples(AUDIO_SAMPLES, i64::from(SAMPLE_RATE))
            .map_err(build_error)?
            .0;
        let rate = Rate::new(VIDEO_FPS, 1).map_err(build_error)?;
        let mut tracks = Vec::with_capacity(TRACKS);
        for k in 0..TRACKS {
            let mut track = Track {
                clips: Vec::with_capacity(c as usize),
                drivers: Vec::with_capacity(c as usize),
            };
            for j in 0..c {
                let clip = Clip {
                    start: Time(j * L),
                    len: Time(L),
                    source: "gen".into(),
                    source_offset: Time(0),
                    name: format!("c{j}"),
                };
                let driver = match k % 3 {
                    0 => {
                        let keys = [(0, 0.0), (L, 1.0)]
                            .map(|(time, value)| Keyframe {
                                time: Time(time),
                                value,
                                interp: Interp::Linear,
                                handles: None,
                            })
                            .to_vec();
                        let channel = Channel {
                            target: format!("track{k}/value"),
                            keys,
                        };
                        Driver::Animation(
                            AnimationDriver::new(clip.clone(), vec![channel])
                                .map_err(build_error)?,
                        )
                    }
                    1 => {
                        let mut audio = AudioDriver::new(SAMPLE_RATE).map_err(build_error)?;
                        let source = tone(TONE_HZ, AMPLITUDE).map_err(build_error)?;
                        audio
                            .add_clip(clip.start..Time(clip.start.0 + L), source, 0, 0)
                            .map_err(build_error)?;
                        Driver::Audio(audio)
                    }
                    _ => Driver::Video(
                        VideoDriver::new(rate, clip.start, clip.len, clip.source_offset)
                            .map_err(build_error)?,
                    ),
                };
                track.clips.push(clip);
                track.drivers.push(driver);
            }
            tracks.push(track);
        }
        Ok(Self {
            tracks,
            span: c * L,
            audio_ticks,
        })
    }

    fn eval_track<'a>(&self, k: usize, p: Time, buf: &'a mut [f64]) -> Result<Values<'a>, String> {
        let evaluate = |buf: &'a mut [f64]| -> Result<Values<'a>, DriverError> {
            let track = self.tracks.get(k).ok_or(DriverError::InvalidRange)?;
            let j = clip_at(&track.clips, p).ok_or(DriverError::InvalidRange)?;
            match &track.drivers[j] {
                Driver::Animation(driver) => {
                    let mut output = [0.0];
                    driver.evaluate(p..Time(p.0 + 1), &mut output)?;
                    Ok(Values::Animation(output[0]))
                }
                Driver::Audio(driver) => {
                    let buf = buf
                        .get_mut(..AUDIO_BUFFER)
                        .ok_or(DriverError::BufferTooSmall)?;
                    buf.fill(0.0);
                    driver.evaluate(p..Time(p.0 + self.audio_ticks), buf)?;
                    // Include the two zeroed spare slots in the checksum too.
                    Ok(Values::Audio(buf))
                }
                Driver::Video(driver) => Ok(Values::Video(u64::from_le_bytes(
                    driver.frame_at(p)?.pixels,
                ))),
            }
        };
        evaluate(buf).map_err(|e| format!("scrub_bench: FAIL eval track={k} t={} {e:?}", p.0))
    }

    fn evaluate_at(&self, p: Time, buf: &mut [f64]) -> Result<u64, String> {
        let mut hash = FNV_OFFSET;
        for k in 0..self.tracks.len() {
            match self.eval_track(k, p, buf)? {
                Values::Animation(value) => hash = hash_word(hash, value.to_bits()),
                Values::Audio(values) => {
                    for value in values {
                        hash = hash_word(hash, value.to_bits());
                    }
                }
                Values::Video(index) => hash = hash_word(hash, index),
            }
        }
        Ok(black_box(hash))
    }

    fn scrub(&self, steps: u64) -> Result<(Vec<u128>, u64), String> {
        if steps == 0 {
            return Err("scrub_bench: FAIL steps must be positive".into());
        }
        let count = usize::try_from(steps).map_err(|_| "scrub_bench: FAIL steps overflow")?;
        let mut durations = Vec::new();
        durations
            .try_reserve_exact(count)
            .map_err(|e| format!("scrub_bench: FAIL durations {e:?}"))?;
        let mut buf = [0.0; AUDIO_BUFFER];
        let mut checksum = FNV_OFFSET;
        for i in 0..steps {
            let p = Time((i128::from(i) * i128::from(self.span) / i128::from(steps)) as i64);
            let start = Instant::now();
            let result = self.evaluate_at(p, &mut buf);
            let elapsed = start.elapsed().as_nanos();
            let hash = result?;
            durations.push(elapsed);
            checksum = hash_word(checksum, hash);
        }
        Ok((durations, black_box(checksum)))
    }
}

// Empty input and unrepresentable microseconds saturate conservatively;
// scrub rejects zero steps, so real runs always have a nonempty sample.
fn percentiles(durations: &mut [u128]) -> (u64, u64, u64) {
    if durations.is_empty() {
        return (u64::MAX, u64::MAX, u64::MAX);
    }
    durations.sort_unstable();
    let n = durations.len() as u128;
    let at = |rank: u128| {
        let ns = durations[((n * rank).div_ceil(100) - 1) as usize];
        u64::try_from(ns.div_ceil(1000)).unwrap_or(u64::MAX)
    };
    (at(50), at(99), at(100))
}

fn verdict(clips: u64, p50: u64, p99: u64, max: u64, budget: u64) -> (String, i32) {
    if p99 <= budget {
        (
            format!(
                "scrub_bench: ok clips={clips} tracks={TRACKS} \
                 p50_us={p50} p99_us={p99} max_us={max}"
            ),
            0,
        )
    } else {
        (
            format!(
                "scrub_bench: FAIL p99_us={p99} p99_max_us={budget} \
                 clips={clips} tracks={TRACKS} p50_us={p50} max_us={max}"
            ),
            1,
        )
    }
}

fn main() {
    let raw = std::env::args_os()
        .skip(1)
        .map(|s| s.into_string())
        .collect::<Result<Vec<_>, _>>();
    let args = match raw
        .map_err(|_| "argument is not Unicode".into())
        .and_then(|a| parse_args(&a))
    {
        Ok(args) => args,
        Err(reason) => {
            eprintln!("scrub_bench: refused {reason}");
            std::process::exit(2);
        }
    };
    let (mut durations, checksum) = match Bench::build(args.clips).and_then(|b| b.scrub(STEPS)) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!(
        "scrub_bench: run clips={} tracks={TRACKS} steps={STEPS} checksum={checksum:016x}",
        args.clips
    );
    let (p50, p99, max) = percentiles(&mut durations);
    let (line, code) = verdict(args.clips, p50, p99, max, args.p99_max_us);
    println!("{line}");
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parse_args_refuses_bad_input() {
        let expected = Args {
            clips: 100000,
            p99_max_us: 2000,
        };
        for input in [
            "--clips 100000 --p99-max-us 2000",
            "--p99-max-us 2000 --clips 100000",
        ] {
            assert_eq!(parse_args(&args(input)).unwrap(), expected);
        }
        for input in [
            "",
            "--clips 100",
            "--clips 100 --clips 100 --p99-max-us 5",
            "--clips 150 --p99-max-us 5",
            "--clips 0 --p99-max-us 5",
            "--clips 10000100 --p99-max-us 5",
            "--clips -100 --p99-max-us 5",
            "--clips +100 --p99-max-us 5",
            "--clips 100 --p99-max-us 0",
            "--clips 100 --p99-max-us 18446744073709551616",
            "--clips 100 --p99-max-us",
            "--clips 100 --p99-max-us 5 --steps 3",
            "--clips 100 --p99-max-us 5 --p99-max-us 6",
            "--clips 18446744073709551616 --p99-max-us 5",
        ] {
            assert!(parse_args(&args(input)).is_err(), "{input:?}");
        }
        let mut input = args("--clips 100 --p99-max-us 5");
        for value in ["", " 100", "100\r", "100\n", "\u{ff11}", "x100"] {
            input[1] = value.into();
            assert!(parse_args(&input).is_err(), "{value:?}");
        }
        input[1] = "9".repeat(100_000);
        assert!(parse_args(&input).is_err());
        for clips in [0, 99, 150, MAX_CLIPS + 100, u64::MAX] {
            assert!(Bench::build(clips).is_err(), "clips={clips}");
        }
        assert!(parse_args(&args("--clips 10000000 --p99-max-us 18446744073709551615")).is_ok());
    }

    #[test]
    fn clip_lookup_matches_linear_scan() {
        let bench = Bench::build(1000).unwrap();
        let clips = &bench.tracks[0].clips;
        for (p, expected) in [
            (0, Some(0)),
            (L - 1, Some(0)),
            (L, Some(1)),
            (10 * L - 1, Some(9)),
            (10 * L, None),
            (-1, None),
        ] {
            assert_eq!(clip_at(clips, Time(p)), expected, "t={p}");
        }
        for m in 0..80 {
            let p = Time(m * L / 7);
            let linear = clips
                .iter()
                .position(|c| c.start.0 <= p.0 && p.0 < c.start.0 + c.len.0);
            assert_eq!(clip_at(clips, p), linear, "t={}", p.0);
        }
        let gap = vec![clips[0].clone(), clips[2].clone()];
        assert_eq!(clip_at(&gap, Time(L + 1)), None);
        assert_eq!(clip_at(&[], Time(0)), None);
        assert_eq!(clip_at(clips, Time(i64::MAX)), None);
        assert_eq!(clip_at(clips, Time(i64::MIN)), None);
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mut ns: Vec<u128> = (1..=100).rev().map(|n| n * 1000).collect();
        assert_eq!(percentiles(&mut ns), (50, 99, 100));
        assert_eq!(percentiles(&mut [1]), (1, 1, 1));
        assert_eq!(percentiles(&mut [1001]), (2, 2, 2));
        let mut ten: Vec<u128> = (1..=10).map(|n| n * 1000).collect();
        assert_eq!(percentiles(&mut ten), (5, 10, 10));
        let saturated = (u64::MAX, u64::MAX, u64::MAX);
        assert_eq!(percentiles(&mut []), saturated);
        assert_eq!(percentiles(&mut [u128::MAX]), saturated);
    }

    #[test]
    fn verdict_line_and_exit_code() {
        let ok = "scrub_bench: ok clips=100000 tracks=100 p50_us=40 p99_us=900 max_us=1500";
        assert_eq!(verdict(100000, 40, 900, 1500, 2000), (ok.into(), 0));
        let (line, code) = verdict(100000, 40, 900, 1500, 900);
        assert_eq!(code, 0);
        assert!(line.starts_with("scrub_bench: ok"));
        let (line, code) = verdict(100000, 40, 900, 1500, 899);
        assert_eq!(code, 1);
        assert!(line.starts_with("scrub_bench: FAIL p99_us=900 p99_max_us=899"));
        let (line, code) = verdict(100000, 1, 1, 1, 1);
        assert_eq!(code, 0);
        assert!(line.starts_with("scrub_bench: ok"));
    }

    #[test]
    fn scrub_evaluates_every_track_by_its_rule() {
        let mut b = Bench::build(300).unwrap();
        assert_eq!(b.tracks.len(), 100);
        assert!(b.tracks.iter().all(|t| t.clips.len() == 3));
        let (a, ca) = b.scrub(50).unwrap();
        let (d, cd) = b.scrub(50).unwrap();
        assert_eq!((a.len(), d.len()), (50, 50));
        assert_eq!(ca, cd);
        assert!(b.scrub(0).is_err());
        assert!(b.scrub(u64::MAX).is_err());
        let mut buf = [42.0; AUDIO_BUFFER];
        let error = b.evaluate_at(Time(3 * L), &mut buf).unwrap_err();
        let outside = format!("scrub_bench: FAIL eval track=0 t={} InvalidRange", 3 * L);
        assert_eq!(error, outside);
        assert!(b.evaluate_at(Time(i64::MIN), &mut buf).is_err());
        assert!(b.evaluate_at(Time(i64::MAX), &mut buf).is_err());
        let p = Time(L + L / 2);
        match b.eval_track(0, p, &mut buf).unwrap() {
            Values::Animation(value) => assert!((value - 0.5).abs() <= 1e-12),
            other => panic!("animation expected, got {other:?}"),
        }
        match b.eval_track(2, p, &mut buf).unwrap() {
            Values::Video(frame) => assert_eq!(frame, 3),
            other => panic!("video expected, got {other:?}"),
        }
        assert_eq!(p.to_samples(48_000).unwrap(), 18_000);
        let sample =
            |local: i64| (TAU * (local as f64 * (440.0 / 48_000.0)).rem_euclid(1.0)).sin() * 0.5;
        match b.eval_track(1, p, &mut buf).unwrap() {
            Values::Audio(values) => {
                assert!((values[1] - sample(6001)).abs() <= 1e-12);
                assert_eq!(&values[480..], &[0.0, 0.0]);
            }
            other => panic!("audio expected, got {other:?}"),
        }
        // Independent, hand-derived values and byte-wise FNV reference. Omitting
        // any video track from evaluate_at changes this expected step hash.
        let mut bytes = Vec::new();
        for k in 0..100 {
            match k % 3 {
                0 => bytes.extend(0.5_f64.to_bits().to_le_bytes()),
                1 => {
                    for local in 6000..6480 {
                        bytes.extend(sample(local).to_bits().to_le_bytes());
                    }
                    bytes.extend([0; 16]);
                }
                _ => bytes.extend(3_u64.to_le_bytes()),
            }
        }
        let expected = bytes.into_iter().fold(0xcbf29ce484222325_u64, |h, byte| {
            (h ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
        assert_eq!(b.evaluate_at(p, &mut buf).unwrap(), expected);
        let error = b.eval_track(1, p, &mut []).unwrap_err();
        let too_small = format!("scrub_bench: FAIL eval track=1 t={} BufferTooSmall", p.0);
        assert_eq!(error, too_small);
        // Lookup succeeds here, but the actual animation driver refuses its range.
        b.tracks[0].clips[2].len = Time(2 * L);
        assert_eq!(b.evaluate_at(Time(3 * L), &mut buf).unwrap_err(), outside);
    }
}
