//! Strict v1 event-log validation and deterministic pressure/stabilizer/dab replay.

use crate::dab::{DabError, RoundDab};
use crate::pressure::{DynamicsError, PressureCurve, PressureResponse, TiltEllipse};
use crate::stabilizer::{Point, Stabilizer, StabilizerError};

pub const SCHEMA_LINE: &str = "schema=1";
pub const HEADER_LINE: &str = "seq,t_ns,window,device,kind,a,b,c,d,e";
/// Raw pen pressure at full force; pressure = raw / 1024.
pub const MAX_PRESSURE_RAW: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PenKind {
    Down,
    Move,
    Up,
}

/// One pen row. `line` is the 1-based line number in the file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenEvent {
    pub line: usize,
    pub seq: u64,
    pub t_ns: u64,
    pub window: u32,
    pub kind: PenKind,
    pub x: f64,
    pub y: f64,
    pub pressure_raw: u32,
    pub tilt_x: i32,
    pub tilt_y: i32,
}

impl PenEvent {
    /// pressure_raw as f32 / 1024.0
    pub fn pressure(&self) -> f32 {
        self.pressure_raw as f32 / MAX_PRESSURE_RAW as f32
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogError {
    Empty,
    MissingFinalNewline,
    BadLine { line: usize },
    MissingSchema,
    UnknownVersion { line: usize, version: u64 },
    BadHeader { line: usize },
    WrongFieldCount { line: usize },
    BadNumber { line: usize },
    BadSequence { line: usize },
    TimeBackwards { line: usize },
    BadToken { line: usize },
    UnknownPenKind { line: usize },
    PressureOutOfRange { line: usize },
    BadTilt { line: usize },
}

impl std::fmt::Display for LogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for LogError {}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn unsigned<T: std::str::FromStr>(text: &str, line: usize) -> Result<T, LogError> {
    if !digits(text) || (text.len() > 1 && text.starts_with('0')) {
        return Err(LogError::BadNumber { line });
    }
    text.parse().map_err(|_| LogError::BadNumber { line })
}

fn coordinate(text: &str, line: usize) -> Result<f64, LogError> {
    let magnitude = text.strip_prefix('-').unwrap_or(text);
    let Some((whole, fraction)) = magnitude.split_once('.') else {
        return Err(LogError::BadNumber { line });
    };
    if !digits(whole) || fraction.len() != 3 || !digits(fraction) {
        return Err(LogError::BadNumber { line });
    }
    let value: f64 = text.parse().map_err(|_| LogError::BadNumber { line })?;
    if !value.is_finite() {
        return Err(LogError::BadNumber { line });
    }
    Ok(value)
}

fn tilt(text: &str, line: usize) -> Result<i32, LogError> {
    if !digits(text.strip_prefix('-').unwrap_or(text)) {
        return Err(LogError::BadNumber { line });
    }
    text.parse().map_err(|_| LogError::BadNumber { line })
}

fn token(text: &str, payload: bool) -> bool {
    !text.is_empty()
        && text.bytes().all(|b| {
            b.is_ascii_lowercase()
                || b.is_ascii_digit()
                || b == b'_'
                || (payload && (b.is_ascii_uppercase() || b == b'.' || b == b'-'))
        })
}

/// Validate every row of a v1 log; return only the pen rows, in file order.
pub fn parse_pen_events(text: &str) -> Result<Vec<PenEvent>, LogError> {
    if text.is_empty() {
        return Err(LogError::Empty);
    }
    if !text.ends_with('\n') {
        return Err(LogError::MissingFinalNewline);
    }
    let mut events = Vec::new();
    let mut previous_seq: Option<u64> = None;
    let mut previous_time = 0;
    let mut saw_header = false;
    for (index, row) in text.split_terminator('\n').enumerate() {
        let line = index + 1;
        if row.is_empty() || row.contains('\r') {
            return Err(LogError::BadLine { line });
        }
        if line == 1 {
            let version = row
                .strip_prefix("schema=")
                .filter(|value| digits(value))
                .and_then(|value| value.parse::<u64>().ok())
                .ok_or(LogError::MissingSchema)?;
            if version != 1 {
                return Err(LogError::UnknownVersion { line, version });
            }
            continue;
        }
        if line == 2 {
            if row != HEADER_LINE {
                return Err(LogError::BadHeader { line });
            }
            saw_header = true;
            continue;
        }
        let mut fields = [""; 10];
        let mut parts = row.split(',');
        for field in &mut fields {
            *field = parts.next().ok_or(LogError::WrongFieldCount { line })?;
        }
        if parts.next().is_some() {
            return Err(LogError::WrongFieldCount { line });
        }
        let seq = unsigned::<u64>(fields[0], line)?;
        let expected = match previous_seq {
            None => Some(0),
            Some(previous) => previous.checked_add(1),
        };
        if Some(seq) != expected {
            return Err(LogError::BadSequence { line });
        }
        let t_ns = unsigned::<u64>(fields[1], line)?;
        if t_ns < previous_time {
            return Err(LogError::TimeBackwards { line });
        }
        let window = unsigned::<u32>(fields[2], line)?;
        if !token(fields[3], false) {
            return Err(LogError::BadToken { line });
        }
        if !token(fields[4], false) {
            return Err(LogError::BadToken { line });
        }
        if fields[3] == "pen" {
            let kind = match fields[4] {
                "down" => PenKind::Down,
                "move" => PenKind::Move,
                "up" => PenKind::Up,
                _ => return Err(LogError::UnknownPenKind { line }),
            };
            let x = coordinate(fields[5], line)?;
            let y = coordinate(fields[6], line)?;
            if !digits(fields[7]) {
                return Err(LogError::BadNumber { line });
            }
            let pressure_raw = fields[7]
                .parse::<u32>()
                .map_err(|_| LogError::BadNumber { line })?;
            if pressure_raw > MAX_PRESSURE_RAW {
                return Err(LogError::PressureOutOfRange { line });
            }
            let tilt_x = tilt(fields[8], line)?;
            let tilt_y = tilt(fields[9], line)?;
            TiltEllipse::from_degrees(tilt_x as f32, tilt_y as f32, 0.0)
                .map_err(|_| LogError::BadTilt { line })?;
            events.push(PenEvent {
                line,
                seq,
                t_ns,
                window,
                kind,
                x,
                y,
                pressure_raw,
                tilt_x,
                tilt_y,
            });
        } else if fields[5..].iter().any(|field| !token(field, true)) {
            return Err(LogError::BadToken { line });
        }
        previous_seq = Some(seq);
        previous_time = t_ns;
    }
    if !saw_header {
        return Err(LogError::BadHeader { line: 2 });
    }
    Ok(events)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplaySettings {
    pub width: usize,
    pub height: usize,
    pub color: [u8; 3],
    pub hardness: f64,
}

/// The settings the stroke_v1 golden was made with.
pub const STROKE_V1_SETTINGS: ReplaySettings = ReplaySettings {
    width: 160,
    height: 64,
    color: [0, 0, 0],
    hardness: 0.5,
};

#[derive(Clone, Debug, PartialEq)]
pub enum ReplayError {
    BadSettings,
    NoPenDown { line: usize },
    DoubleDown { line: usize },
    Unterminated { line: usize },
    Pressure { line: usize, error: DynamicsError },
    Stabilizer { line: usize, error: StabilizerError },
    Dab { line: usize, error: DabError },
}

impl std::fmt::Display for ReplayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ReplayError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Replay {
    /// RGBA8, width * height * 4 bytes.
    pub pixels: Vec<u8>,
    pub width: usize,
    pub height: usize,
    /// Pen rows replayed.
    pub frames: usize,
}

fn make_dab(
    response: PressureResponse,
    settings: &ReplaySettings,
    line: usize,
) -> Result<RoundDab, ReplayError> {
    RoundDab::new(
        f64::from(response.size),
        1.0,
        settings.hardness,
        f64::from(response.opacity),
    )
    .map_err(|error| ReplayError::Dab { line, error })
}

pub fn replay(events: &[PenEvent], settings: &ReplaySettings) -> Result<Replay, ReplayError> {
    let length = settings
        .width
        .checked_mul(settings.height)
        .and_then(|n| n.checked_mul(4))
        .filter(|n| *n != 0)
        .ok_or(ReplayError::BadSettings)?;
    RoundDab::new(1.0, 1.0, settings.hardness, 1.0).map_err(|_| ReplayError::BadSettings)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(length)
        .map_err(|_| ReplayError::BadSettings)?;
    pixels.resize(length, 0);
    let mut stabilizer = Stabilizer::new();
    let mut last_pressure = None;
    let sample = |pressure, line| {
        PressureCurve
            .sample(pressure)
            .map_err(|error| ReplayError::Pressure { line, error })
    };
    for event in events {
        let line = event.line;
        let (point, dab) = match event.kind {
            PenKind::Down | PenKind::Move => {
                match (event.kind, last_pressure) {
                    (PenKind::Down, Some(_)) => return Err(ReplayError::DoubleDown { line }),
                    (PenKind::Move, None) => return Err(ReplayError::NoPenDown { line }),
                    _ => {}
                }
                let response = sample(event.pressure(), line)?;
                let point = stabilizer
                    .push(Point {
                        x: event.x,
                        y: event.y,
                    })
                    .map_err(|error| ReplayError::Stabilizer { line, error })?;
                let dab = make_dab(response, settings, line)?;
                last_pressure = Some(event.pressure());
                (Some(point), dab)
            }
            PenKind::Up => {
                let pressure = last_pressure.ok_or(ReplayError::NoPenDown { line })?;
                let point = stabilizer.pen_up();
                let dab = make_dab(sample(pressure, line)?, settings, line)?;
                last_pressure = None;
                (point, dab)
            }
        };
        if let Some(point) = point {
            dab.stamp(
                &mut pixels,
                settings.width,
                settings.height,
                [point.x, point.y],
                settings.color,
            )
            .map_err(|error| ReplayError::Dab { line, error })?;
        }
    }
    if let Some(event) = events.last().filter(|_| last_pressure.is_some()) {
        return Err(ReplayError::Unterminated { line: event.line });
    }
    Ok(Replay {
        pixels,
        width: settings.width,
        height: settings.height,
        frames: events.len(),
    })
}

/// Composite a valid replay's RGBA8 buffer over opaque white, integer-exact.
/// Manually constructed replays must preserve the width * height * 4 invariant.
pub fn flatten_over_white(replay: &Replay) -> crate::ppm::Image {
    let rgb = replay
        .pixels
        .chunks_exact(4)
        .flat_map(|pixel| {
            let alpha = u32::from(pixel[3]);
            [pixel[0], pixel[1], pixel[2]].map(|channel| {
                ((u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
            })
        })
        .collect();
    crate::ppm::Image {
        width: replay.width,
        height: replay.height,
        rgb,
    }
}
