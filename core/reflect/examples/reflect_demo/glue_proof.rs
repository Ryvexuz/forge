use reflect::desc::{Reflect, TypeDesc};
use reflect::limits::Limits;
use reflect::sample::{Vehicle, VehicleWide};
use reflect::text::save_text;
use reflect::value::Value;
use reflect::{inspector, schema, script::run_script};
use std::io::Write;
use std::path::Path;

/// The sample type and field names from tests/glue.rs, in the same order.
pub const NAMES: [&str; 11] = [
    "Vehicle",
    "VehicleWide",
    "Wheel",
    "DriveMode",
    "Roadster",
    "mass_kg",
    "radius_m",
    "width_m",
    "steered",
    "wheels",
    "armor_rating",
];

/// Lines added plus removed, excluding only each text's first type header.
pub fn diff_lines(base: &str, wide: &str) -> usize {
    fn lines(text: &str) -> Vec<&str> {
        let mut lines: Vec<_> = text.lines().collect();
        if let Some(index) = lines.iter().position(|line| line.starts_with("type ")) {
            lines.remove(index);
        }
        lines
    }

    let a = lines(base);
    let b = lines(wide);
    let mut table = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, left) in a.iter().enumerate() {
        for (j, right) in b.iter().enumerate() {
            table[i + 1][j + 1] = if left == right {
                table[i][j] + 1
            } else {
                table[i][j + 1].max(table[i + 1][j])
            };
        }
    }
    let common = table[a.len()][b.len()];
    (a.len() - common) + (b.len() - common)
}

/// The same generic path for both types: schema, inspector, then saved text.
pub fn texts(desc: &TypeDesc, value: &Value, limits: &Limits) -> Result<[String; 3], String> {
    Ok([
        schema::schema_text(desc).map_err(|e| format!("FAIL glue-proof {e}"))?,
        inspector::to_text(desc),
        save_text(desc, value, limits).map_err(|e| format!("FAIL glue-proof {e}"))?,
    ])
}

pub struct Outcome {
    pub types: String,
    pub extra_field: String,
    pub extra_count: usize,
    pub schema_diff_lines: usize,
    pub inspector_diff_lines: usize,
    pub save_diff_lines: usize,
    pub script_ok: bool,
    pub generic_modules_clean: bool,
}

/// Compare the types, exercise a wide-field script, and scan the crate root.
pub fn outcome(root: &Path) -> Result<Outcome, String> {
    let limits = super::limits()?;
    let base = Vehicle::desc();
    let wide = VehicleWide::desc();
    let wide_vehicle = || {
        let v = super::vehicle();
        VehicleWide {
            name: v.name,
            mass_kg: v.mass_kg,
            position: v.position,
            wheels: v.wheels,
            note: v.note,
            drive: v.drive,
            armor_rating: 2.5,
        }
    };
    let base_value =
        super::snapshot(&super::vehicle()).map_err(|e| format!("FAIL glue-proof {e}"))?;
    let w = wide_vehicle();
    let wide_value = super::snapshot(&w).map_err(|e| format!("FAIL glue-proof {e}"))?;
    let base_texts = texts(&base, &base_value, &limits)?;
    let wide_texts = texts(&wide, &wide_value, &limits)?;
    let extra: Vec<_> = wide
        .fields()
        .iter()
        .filter(|field| base.field_index(&field.name).is_none())
        .map(|field| field.name.as_str())
        .collect();

    let mut w2 = wide_vehicle();
    let script_ok = match run_script(
        "set vehicle.armor_rating 7.5\nset vehicle.armor_rating 11.0 # expect-error\n",
        "vehicle",
        &mut w2,
        &limits,
        &mut super::DropChanges,
    ) {
        Ok(report) => {
            report.commands == 2
                && report.ok == 1
                && report.errors.len() == 1
                && w2.armor_rating == 7.5
        }
        Err(_) => false,
    };
    let mut generic_modules_clean = true;
    for module in [
        "text",
        "path",
        "script",
        "inspector",
        "watch",
        "abi",
        "registry",
    ] {
        let relative = format!("src/{module}.rs");
        let text = std::fs::read_to_string(root.join(&relative))
            .map_err(|e| format!("FAIL glue-proof cannot read {relative}: {e}"))?;
        for line in text.lines() {
            for word in line
                .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .filter(|word| !word.is_empty())
            {
                if NAMES.contains(&word) {
                    generic_modules_clean = false;
                }
            }
        }
    }
    Ok(Outcome {
        types: format!("{},{}", base.name(), wide.name()),
        extra_field: if extra.is_empty() {
            "none".into()
        } else {
            extra.join(",")
        },
        extra_count: extra.len(),
        schema_diff_lines: diff_lines(&base_texts[0], &wide_texts[0]),
        inspector_diff_lines: diff_lines(&base_texts[1], &wide_texts[1]),
        save_diff_lines: diff_lines(&base_texts[2], &wide_texts[2]),
        script_ok,
        generic_modules_clean,
    })
}

/// Print one proof line; return 0 for PASS or 1 for FAIL.
pub fn run(root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let o = outcome(root)?;
    let pass = o.extra_count == 1
        && o.schema_diff_lines == 1
        && o.inspector_diff_lines == 1
        && o.save_diff_lines == 1
        && o.script_ok
        && o.generic_modules_clean;
    super::line(
        out,
        format!(
            "glue-proof types={} extra_field={} schema_diff_lines={} inspector_diff_lines={} save_diff_lines={} script_ok={} generic_modules_clean={} result={}",
            o.types,
            o.extra_field,
            o.schema_diff_lines,
            o.inspector_diff_lines,
            o.save_diff_lines,
            o.script_ok,
            o.generic_modules_clean,
            if pass { "PASS" } else { "FAIL" }
        ),
    )?;
    Ok(u8::from(!pass))
}
