//! Versioned, tab-separated timeline persistence. See `docs/FORMAT.md`.

use crate::time::Time;
use crate::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};

/// A malformed file is rejected as a whole.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormatError {
    MissingHeader,
    UnknownVersion,
    WrongColumnCount,
    InvalidNumber,
    NonFinite,
    InvalidText,
    InvalidPath,
    MissingTrack,
    MissingChannel,
    DuplicateTrack,
    UnknownRecord,
    TruncatedRecord,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FormatError {}

fn text(value: &str) -> Result<(), FormatError> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(FormatError::InvalidText);
    }
    Ok(())
}

fn path(value: &str) -> Result<(), FormatError> {
    text(value)?;
    if value.contains('\\') || value.contains(':') {
        return Err(FormatError::InvalidPath);
    }
    Ok(())
}

fn number<T: std::str::FromStr>(value: &str) -> Result<T, FormatError> {
    value.parse().map_err(|_| FormatError::InvalidNumber)
}

fn scalar(value: &str) -> Result<f64, FormatError> {
    let value: f64 = number(value)?;
    if !value.is_finite() {
        return Err(FormatError::NonFinite);
    }
    Ok(value)
}

/// Load a supported historical schema. Future versions dispatch here.
pub fn migrate(version: u32, input: &str) -> Result<Timeline, FormatError> {
    if version != 1 {
        return Err(FormatError::UnknownVersion);
    }
    parse(input)
}

/// Parse atomically; no partial timeline is returned on error.
pub fn parse(input: &str) -> Result<Timeline, FormatError> {
    let mut lines = input.split_terminator('\n');
    let header = lines.next().ok_or(FormatError::MissingHeader)?;
    let version = header
        .strip_prefix("timeline\t")
        .ok_or(FormatError::MissingHeader)?;
    if number::<u32>(version)? != 1 {
        return Err(FormatError::UnknownVersion);
    }
    if !input.ends_with('\n') {
        return Err(FormatError::TruncatedRecord);
    }
    let mut timeline = Timeline { tracks: Vec::new() };
    for line in lines {
        let cols: Vec<_> = line.split('\t').collect();
        let count = match cols[0] {
            "track" => 4,
            "clip" => 7,
            "channel" => 3,
            "key" => 10,
            _ => return Err(FormatError::UnknownRecord),
        };
        if cols.len() != count {
            return Err(FormatError::WrongColumnCount);
        }
        let id = number::<u64>(cols[1])?;
        if cols[0] == "track" {
            if timeline.tracks.iter().any(|track| track.id == id) {
                return Err(FormatError::DuplicateTrack);
            }
            text(cols[2])?;
            text(cols[3])?;
            timeline.tracks.push(Track {
                id,
                kind: cols[2].into(),
                name: cols[3].into(),
                clips: Vec::new(),
                channels: Vec::new(),
            });
            continue;
        }
        let track = timeline
            .tracks
            .iter_mut()
            .find(|track| track.id == id)
            .ok_or(FormatError::MissingTrack)?;
        match cols[0] {
            "clip" => {
                path(cols[4])?;
                text(cols[6])?;
                let len = number::<i64>(cols[3])?;
                if len < 0 {
                    return Err(FormatError::InvalidNumber);
                }
                track.clips.push(Clip {
                    start: Time(number(cols[2])?),
                    len: Time(len),
                    source: cols[4].into(),
                    source_offset: Time(number(cols[5])?),
                    name: cols[6].into(),
                });
            }
            "channel" => {
                path(cols[2])?;
                track.channels.push(Channel {
                    target: cols[2].into(),
                    keys: Vec::new(),
                });
            }
            "key" => {
                let channel = track
                    .channels
                    .get_mut(number::<usize>(cols[2])?)
                    .ok_or(FormatError::MissingChannel)?;
                let interp = match cols[5] {
                    "step" => Interp::Step,
                    "linear" => Interp::Linear,
                    "bezier" => Interp::Bezier,
                    "hermite" => Interp::Hermite,
                    _ => return Err(FormatError::InvalidText),
                };
                let handles = if cols[6..].iter().all(|value| *value == "-") {
                    None
                } else {
                    Some([
                        (scalar(cols[6])?, scalar(cols[7])?),
                        (scalar(cols[8])?, scalar(cols[9])?),
                    ])
                };
                channel.keys.push(Keyframe {
                    time: Time(number(cols[3])?),
                    value: scalar(cols[4])?,
                    interp,
                    handles,
                });
            }
            _ => unreachable!(),
        }
    }
    Ok(timeline)
}

/// Write canonical v1 text. Callers supply valid timeline text fields and finite scalars.
/// Floats use Rust's shortest round-trippable decimal representation.
pub fn write(timeline: &Timeline) -> String {
    let mut out = String::from("timeline\t1\n");
    for track in &timeline.tracks {
        out.push_str(&format!(
            "track\t{}\t{}\t{}\n",
            track.id, track.kind, track.name
        ));
        for clip in &track.clips {
            out.push_str(&format!(
                "clip\t{}\t{}\t{}\t{}\t{}\t{}\n",
                track.id, clip.start.0, clip.len.0, clip.source, clip.source_offset.0, clip.name
            ));
        }
        for (index, channel) in track.channels.iter().enumerate() {
            out.push_str(&format!("channel\t{}\t{}\n", track.id, channel.target));
            for key in &channel.keys {
                let interp = match key.interp {
                    Interp::Step => "step",
                    Interp::Linear => "linear",
                    Interp::Bezier => "bezier",
                    Interp::Hermite => "hermite",
                };
                let handles = match key.handles {
                    None => String::from("-\t-\t-\t-"),
                    Some([(a, b), (c, d)]) => format!("{a}\t{b}\t{c}\t{d}"),
                };
                out.push_str(&format!(
                    "key\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    track.id, index, key.time.0, key.value, interp, handles
                ));
            }
        }
    }
    out
}
