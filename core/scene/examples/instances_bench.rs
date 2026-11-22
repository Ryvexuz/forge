//! Headless instance-update benchmark. Timing excludes setup and verification.

use scene::instance::Instances;
use std::hint::black_box;
use std::time::{Duration, Instant};

#[derive(Debug, PartialEq, Eq)]
struct Options {
    instances: usize,
    frames: u32,
    budget_us: u64,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let mut values = [None; 3];
    while let Some(flag) = args.next() {
        let index = match flag.as_str() {
            "--instances" => 0,
            "--frames" => 1,
            "--budget-us" => 2,
            _ => return Err(format!("arguments: unknown flag {flag:?}")),
        };
        if values[index].is_some() {
            return Err(format!("arguments: duplicate {flag}"));
        }
        let text = args
            .next()
            .ok_or_else(|| format!("arguments: missing value for {flag}"))?;
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("arguments: invalid positive integer for {flag}"));
        }
        let value = text
            .parse::<u64>()
            .ok()
            .filter(|&v| v != 0)
            .ok_or_else(|| format!("arguments: out-of-range value for {flag}"))?;
        values[index] = Some(value);
    }
    let [Some(instances), Some(frames), Some(budget_us)] = values else {
        return Err("arguments: require --instances, --frames and --budget-us".into());
    };
    let instances = usize::try_from(instances)
        .ok()
        .filter(|&n| n <= isize::MAX as usize / std::mem::size_of::<[f32; 12]>())
        .ok_or("arguments: instance allocation size overflow")?;
    let frames = u32::try_from(frames).map_err(|_| "arguments: frame count overflow")?;
    Ok(Options {
        instances,
        frames,
        budget_us,
    })
}

fn checksum(matrices: impl IntoIterator<Item = [f32; 12]>) -> u64 {
    // FNV-1a over every component, in insertion order, including signed zero.
    let mut hash = 0xcbf29ce484222325_u64;
    for matrix in matrices {
        for value in matrix {
            for byte in value.to_bits().to_le_bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
    }
    hash
}

fn scalar_checksum(copy: &mut [(f32, f32)], frames: u32) -> u64 {
    // Deliberately independent of Instances and matrix_from_angle. Repeated f32
    // addition is part of the update contract; angle + frames * speed is not.
    for (angle, speed) in copy.iter_mut() {
        for _ in 0..frames {
            *angle += *speed;
        }
    }
    checksum(copy.iter().map(|&(angle, _)| {
        let mut matrix = [0.0; 12];
        matrix[0] = angle.cos();
        matrix[2] = angle.sin();
        matrix[5] = 1.0;
        matrix[8] = -angle.sin();
        matrix[10] = angle.cos();
        matrix
    }))
}

fn statistics(times: &mut [Duration]) -> Result<(Duration, u128), String> {
    if times.is_empty() {
        return Err("timing: empty sample".into());
    }
    times.sort_unstable();
    let count = times.len() as u128;
    let total = times.iter().try_fold(0_u128, |sum, time| {
        sum.checked_add(time.as_nanos())
            .ok_or("timing: duration sum overflow")
    })?;
    // Nearest rank: ceil(0.99 * n), expressed without usize multiplication.
    let rank = (99 * count).div_ceil(100) as usize;
    Ok((times[rank - 1], total.div_ceil(count)))
}

fn within_budget(p99: Duration, mean_ns: u128, budget_us: u64) -> bool {
    let budget_ns = u128::from(budget_us) * 1_000;
    p99.as_nanos() <= budget_ns && mean_ns <= budget_ns
}

fn run(options: Options) -> Result<(), String> {
    let mut reference = Vec::new();
    reference
        .try_reserve_exact(options.instances)
        .map_err(|e| format!("allocation: reference: {e}"))?;
    let mut times = Vec::new();
    times
        .try_reserve_exact(options.frames as usize)
        .map_err(|e| format!("allocation: timings: {e}"))?;
    let mut instances = Instances::with_capacity(options.instances);
    for i in 0..options.instances {
        // Exactly representable bounded inputs. Even u32::MAX updates cannot
        // overflow f32; no user-supplied floating-point arithmetic is involved.
        let angle = (i % 256) as f32 / 256.0;
        let speed = ((i % 17) as f32 - 8.0) / 1024.0;
        instances
            .push(0, 0, angle, speed)
            .map_err(|e| format!("initialization: {e:?}"))?;
        reference.push((angle, speed));
    }
    for frame in 0..options.frames {
        let start = Instant::now();
        instances
            .update_frame(black_box(frame))
            .map_err(|e| format!("update: {e:?}"))?;
        times.push(start.elapsed());
        black_box(instances.matrices());
    }
    let actual = checksum(instances.matrices().iter().copied());
    if actual != scalar_checksum(&mut reference, options.frames) {
        return Err("checksum".into());
    }
    let (p99, mean_ns) = statistics(&mut times)?;
    if !within_budget(p99, mean_ns, options.budget_us) {
        return Err("budget".into());
    }
    println!(
        "instances_bench: ok instances={} frames={} p99_us={} mean_us={} budget_us={} checksum={actual:016x}",
        options.instances,
        options.frames,
        p99.as_nanos().div_ceil(1_000),
        mean_ns.div_ceil(1_000),
        options.budget_us,
    );
    Ok(())
}

fn main() -> std::process::ExitCode {
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("instances_bench: FAIL {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_fail_closed() {
        for invalid in [
            "",
            "0",
            "-1",
            "+1",
            "NaN",
            "inf",
            "1\r",
            "1\n",
            "C:\\x",
            "18446744073709551616",
        ] {
            assert!(parse(
                ["--instances", invalid, "--frames", "1", "--budget-us", "1"].map(str::to_owned)
            )
            .is_err());
        }
        assert!(parse(Vec::<String>::new()).is_err());
        assert!(parse(["--unknown"].map(str::to_owned)).is_err());
        assert!(parse(["--frames", "1", "--frames", "2"].map(str::to_owned)).is_err());
        assert!(parse(["--instances"].map(str::to_owned)).is_err());
        assert!(parse(
            [
                "--instances",
                "1",
                "--frames",
                "4294967296",
                "--budget-us",
                "1"
            ]
            .map(str::to_owned)
        )
        .is_err());
    }

    #[test]
    fn both_timing_guards_are_required() {
        assert!(within_budget(Duration::from_micros(1), 1_000, 1));
        assert!(!within_budget(Duration::from_nanos(1_001), 1_000, 1));
        assert!(!within_budget(Duration::from_micros(1), 1_001, 1));
        assert!(statistics(&mut []).is_err());
        let mut times = vec![Duration::from_micros(1); 100];
        times[99] = Duration::from_micros(201);
        let (p99, mean) = statistics(&mut times).unwrap();
        assert_eq!(p99, Duration::from_micros(1));
        assert_eq!(mean, 3_000);
        assert!(!within_budget(p99, mean, 1));
    }

    #[test]
    fn scalar_reference_detects_wrong_updates_and_transforms() {
        let mut instances = Instances::with_capacity(2);
        instances.push(0, 0, 0.25, -0.125).unwrap();
        instances.push(0, 0, -0.5, 0.0625).unwrap();
        let initial = vec![(0.25, -0.125), (-0.5, 0.0625)];
        let expected = scalar_checksum(&mut initial.clone(), 3);
        for frame in 0..3 {
            instances.update_frame(frame).unwrap();
        }
        assert_eq!(checksum(instances.matrices().iter().copied()), expected);
        assert_ne!(scalar_checksum(&mut initial.clone(), 2), expected);
        let mut damaged = instances.matrices().to_vec();
        damaged[0][3] = 1.0;
        assert_ne!(checksum(damaged), expected);
    }
}
