use reflect::path::{get_path, parse_path, set_path};
use reflect::value::Value;
use reflect::watch::{FanOut, Journal};
use std::io::Write;

pub struct Scenario {
    pub sets: usize,
    pub accepted: usize,
    pub rejected: usize,
    pub mass_after: f64,
    pub a: Journal,
    pub b: Journal,
}

/// Runs the four watch sets; setup errors name the failing step.
pub fn scenario() -> Result<Scenario, String> {
    let limits = super::limits()?;
    let mut vehicle = super::vehicle();
    vehicle.mass_kg = 1200.0;
    let mut a = Journal::new();
    let mut b = Journal::new();
    let mut sets = 0;
    let mut accepted = 0;
    let mut rejected = 0;
    {
        let mut fan = FanOut::new();
        fan.add(&mut a);
        fan.add(&mut b);
        for (p, value) in [
            ("mass_kg", Value::F64(1350.0)),
            ("wheels[0].radius_m", Value::F64(0.33)),
            ("mass_kg", Value::F64(6000.0)),
            ("wheels[1].steered", Value::Bool(true)),
        ] {
            let path = parse_path(p, &limits).map_err(|e| format!("FAIL watch path {p}: {e}"))?;
            sets += 1;
            match set_path(&mut vehicle, &path, value, &limits, &mut fan) {
                Ok(()) => accepted += 1,
                Err(_) => rejected += 1,
            }
        }
    }
    let path =
        parse_path("mass_kg", &limits).map_err(|e| format!("FAIL watch path mass_kg: {e}"))?;
    let mass_after = match get_path(&vehicle, &path)
        .map_err(|e| format!("FAIL watch read mass_kg: {e}"))?
    {
        Value::F64(mass) => mass,
        value => return Err(format!("FAIL watch mass_kg kind {}", value.kind_name())),
    };
    Ok(Scenario {
        sets,
        accepted,
        rejected,
        mass_after,
        a,
        b,
    })
}

fn value_text(value: &Value) -> Result<String, String> {
    match value {
        Value::F64(x) => Ok(format!("{x:?}")),
        Value::Bool(b) => Ok(b.to_string()),
        _ => Err(format!("FAIL watch value kind {}", value.kind_name())),
    }
}

/// Prints the summary and journal deliveries; returns 0 on PASS, 1 on FAIL.
pub fn run(out: &mut dyn Write) -> Result<u8, String> {
    let s = scenario()?;
    let first = s.a.entries().first();
    let pass = s.accepted == 3
        && s.rejected == 1
        && s.a == s.b
        && s.a.len() == 3
        && s.mass_after == 1350.0
        && first.is_some_and(|entry| entry.path() == "mass_kg");
    let first_text = match first {
        Some(entry) => format!(
            "\"vehicle.{} {} -> {}\"",
            entry.path(),
            value_text(entry.old())?,
            value_text(entry.new_value())?
        ),
        None => "none".to_owned(),
    };
    super::line(
        out,
        format!(
            "watch Vehicle sets={} accepted={} rejected={} events={} first={} result={}",
            s.sets,
            s.accepted,
            s.rejected,
            s.a.len(),
            first_text,
            if pass { "PASS" } else { "FAIL" }
        ),
    )?;
    for i in 0..s.a.len().max(s.b.len()) {
        for (name, journal) in [("a", &s.a), ("b", &s.b)] {
            if let Some(entry) = journal.entries().get(i) {
                super::line(
                    out,
                    format!(
                        "event {} watcher={name} path=vehicle.{} old={} new={}",
                        i + 1,
                        entry.path(),
                        value_text(entry.old())?,
                        value_text(entry.new_value())?
                    ),
                )?;
            }
        }
    }
    Ok(u8::from(!pass))
}
