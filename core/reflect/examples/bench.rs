//! Check 1.6.13 measures reflective writes against direct writes and text saving.
//! There is no threshold: no number is judged here. Record values on a quiet
//! board. Text time is per top-level Vehicle field (six), not per nested field.

use reflect::change::{Change, ChangeSink};
use reflect::desc::Reflect;
use reflect::limits::Limits;
use reflect::path::{parse_path, set_path};
use reflect::sample::{DriveMode, Vehicle, Wheel};
use reflect::text::save_text;
use reflect::value::Value;
use std::hint::black_box;
use std::time::Instant;

/// Sorted middle value, or the mean of the two middle values for an even count.
/// Returns None for an empty slice or any non-finite value.
pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|v| !v.is_finite()) {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 {
        Some(values[middle])
    } else {
        let (a, b) = (values[middle - 1], values[middle]);
        Some(if a.is_sign_negative() == b.is_sign_negative() {
            a + (b - a) / 2.0
        } else {
            (a + b) / 2.0
        })
    }
}

struct Counter(u128);

impl ChangeSink for Counter {
    fn changed(&mut self, _: &Change) {
        self.0 += 1;
    }
}

/// Runs each measurement `runs` times with `ops` operations, returning two lines.
pub fn report(runs: usize, ops: u64) -> Result<[String; 2], String> {
    let limits = Limits::parse(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reflect.toml"
    )))
    .map_err(|e| format!("limits: {e}"))?;
    let mut vehicle = Vehicle {
        name: "Roadster".into(),
        mass_kg: 1200.0,
        position: [1.0, 2.0, 3.0],
        wheels: vec![
            Wheel {
                radius_m: 0.3,
                width_m: 0.2,
                steered: true,
            },
            Wheel {
                radius_m: 0.4,
                width_m: 0.25,
                steered: false,
            },
        ],
        note: None,
        drive: DriveMode::Front,
    };
    let path = parse_path("mass_kg", &limits).map_err(|e| format!("parse_path: {e}"))?;
    let desc = Vehicle::desc();
    let value = Value::Struct(
        (0..desc.fields().len())
            .map(|i| vehicle.get(i).ok_or_else(|| format!("vehicle.get({i})")))
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut sink = Counter(0);
    let (mut reflective, mut direct, mut text) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..runs {
        vehicle.mass_kg = 1200.0;
        let start = Instant::now();
        for i in 0..ops {
            let m = if i % 2 == 0 { 1000.0 } else { 1001.0 };
            set_path(&mut vehicle, &path, Value::F64(m), &limits, &mut sink)
                .map_err(|e| format!("set_path: {e}"))?;
        }
        reflective.push(start.elapsed().as_nanos() as f64 / ops as f64);
        let start = Instant::now();
        for i in 0..ops {
            let m = if i % 2 == 0 { 1000.0 } else { 1001.0 };
            vehicle.mass_kg = black_box(m);
            black_box(&vehicle);
        }
        direct.push(start.elapsed().as_nanos() as f64 / ops as f64);
        let start = Instant::now();
        for _ in 0..ops {
            black_box(
                save_text(&desc, black_box(&value), &limits)
                    .map_err(|e| format!("save_text: {e}"))?,
            );
        }
        text.push(start.elapsed().as_nanos() as f64 / ops as f64);
    }
    let expected = runs as u128 * u128::from(ops);
    if sink.0 != expected {
        return Err(format!(
            "set_path recorded {} changes, expected {expected}",
            sink.0
        ));
    }
    let x = median(&mut reflective).ok_or("no median")?;
    let y = median(&mut direct).ok_or("no median")?;
    let z = median(&mut text).ok_or("no median")? / desc.fields().len() as f64;
    if y == 0.0 {
        return Err("direct time is zero".into());
    }
    Ok([
        format!(
            "bench set_path ns_per_op={x:.1} direct ns_per_op={y:.1} ratio={:.2}",
            x / y
        ),
        format!("bench text_save ns_per_field={z:.1}"),
    ])
}

#[cfg(not(test))]
fn main() {
    match report(20, 100_000) {
        Ok(lines) => {
            for line in lines {
                println!("{line}");
            }
        }
        Err(reason) => {
            eprintln!("bench: FAIL {reason}");
            std::process::exit(1);
        }
    }
}
