use reflect::desc::{Reflect, TypeDesc};
use reflect::sample::Vehicle;
use reflect::text::{load_text, save_text};
use reflect::value::Value;
use std::io::Write;
use std::path::Path;

/// The one field the planted fault removes.
pub const DROPPED: &str = "mass_kg";

/// Vehicle's descriptor without DROPPED, preserving name, version and order.
pub fn broken_descriptor() -> Result<TypeDesc, String> {
    let desc = Vehicle::desc();
    if desc.field_index(DROPPED).is_none() {
        return Err("FAIL inject-fault field mass_kg not in Vehicle".into());
    }
    let fields = desc
        .fields()
        .iter()
        .filter(|field| field.name != DROPPED)
        .cloned()
        .collect();
    TypeDesc::new(desc.name(), desc.version(), fields).map_err(|e| format!("FAIL inject-fault {e}"))
}

pub struct Outcome {
    pub bytes: usize,
    pub golden: bool,
    pub text_identical: bool,
    pub value_equal: bool,
    pub diff_line: usize,
}

/// Round trip through the broken descriptor; root is the crate folder.
pub fn outcome(root: &Path) -> Result<Outcome, String> {
    let limits = super::limits()?;
    let original =
        super::snapshot(&super::vehicle()).map_err(|e| format!("FAIL inject-fault {e}"))?;
    let Value::Struct(items) = &original else {
        return Err("FAIL inject-fault Vehicle snapshot is not a struct".into());
    };
    let desc = Vehicle::desc();
    let projected = Value::Struct(
        desc.fields()
            .iter()
            .zip(items)
            .filter(|(field, _)| field.name != DROPPED)
            .map(|(_, value)| value.clone())
            .collect(),
    );
    let broken = broken_descriptor()?;
    let first =
        save_text(&broken, &projected, &limits).map_err(|e| format!("FAIL inject-fault {e}"))?;
    let loaded = load_text(&broken, &first, &limits).map_err(|e| format!("FAIL inject-fault {e}"))?;
    let second =
        save_text(&broken, &loaded.value, &limits).map_err(|e| format!("FAIL inject-fault {e}"))?;
    let expected = super::read(root, "fixtures/vehicle.v2.txt")?;
    Ok(Outcome {
        bytes: first.len(),
        golden: first.as_bytes() == expected,
        text_identical: first == second,
        value_equal: original == loaded.value,
        diff_line: super::diff_line(first.as_bytes(), &expected),
    })
}

/// Print all comparisons; exit 1 when the fault is caught, 0 if not.
pub fn run(root: &Path, out: &mut dyn Write) -> Result<u8, String> {
    let o = outcome(root)?;
    let pass = o.golden && o.text_identical && o.value_equal;
    super::line(
        out,
        "fault injected: Vehicle field mass_kg dropped from the descriptor",
    )?;
    super::line(
        out,
        format!(
            "roundtrip Vehicle bytes={} golden={} text_identical={} value_equal={} result={}",
            o.bytes,
            o.golden,
            o.text_identical,
            o.value_equal,
            if pass { "PASS" } else { "FAIL" }
        ),
    )?;
    if !o.golden {
        super::line(out, format!("diff line={}", o.diff_line))?;
    }
    Ok(u8::from(!pass))
}
