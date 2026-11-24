use brush_engine::ppm;
use brush_engine::replay::{flatten_over_white, parse_pen_events, replay, STROKE_V1_SETTINGS};

fn run() -> (String, i32) {
    let mut arguments = std::env::args_os().skip(1);
    let mut corrupt = false;
    if let Some(argument) = arguments.next() {
        if argument != "--corrupt-dab" {
            return (
                format!(
                    "stroke_replay: refused {}",
                    argument.to_string_lossy().escape_debug()
                ),
                2,
            );
        }
        corrupt = true;
    }
    if let Some(argument) = arguments.next() {
        return (
            format!(
                "stroke_replay: refused {}",
                argument.to_string_lossy().escape_debug()
            ),
            2,
        );
    }
    if let Err(error) = parse_pen_events(include_str!("../tests/fixtures/event_log_v1.csv")) {
        return (format!("stroke_replay: FAIL eventlog {error}"), 1);
    }
    let events = match parse_pen_events(include_str!("../tests/fixtures/stroke_v1.csv")) {
        Ok(events) => events,
        Err(error) => return (format!("stroke_replay: FAIL replay {error}"), 1),
    };
    let mut settings = STROKE_V1_SETTINGS;
    if corrupt {
        settings.color[0] = settings.color[0].saturating_add(1);
    }
    let result = match replay(&events, &settings) {
        Ok(result) => result,
        Err(error) => return (format!("stroke_replay: FAIL replay {error}"), 1),
    };
    let mut bytes = Vec::new();
    if let Err(error) = ppm::write(&mut bytes, &flatten_over_white(&result)) {
        return (format!("stroke_replay: FAIL replay {error}"), 1);
    }
    let golden = include_bytes!("../tests/golden/stroke_v1.ppm");
    let differing = bytes
        .iter()
        .zip(golden.iter())
        .filter(|(a, b)| a != b)
        .count()
        + bytes.len().abs_diff(golden.len());
    if differing == 0 {
        (format!("stroke_replay: ok frames={}", result.frames), 0)
    } else {
        (
            format!("stroke_replay: FAIL golden differing_bytes={differing}"),
            1,
        )
    }
}

fn main() {
    let (message, code) = run();
    println!("{message}");
    std::process::exit(code);
}
