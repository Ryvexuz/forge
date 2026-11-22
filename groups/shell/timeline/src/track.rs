//! Plain timeline data without validation or evaluation.

use crate::time::Time;

/// An ordered collection of tracks.
#[derive(Clone, PartialEq, Debug)]
pub struct Timeline {
    pub tracks: Vec<Track>,
}

/// Clips and channels belonging to a driver kind.
#[derive(Clone, PartialEq, Debug)]
pub struct Track {
    pub id: u64,
    pub kind: String,
    pub name: String,
    pub clips: Vec<Clip>,
    pub channels: Vec<Channel>,
}

/// A source interval placed on a track.
#[derive(Clone, PartialEq, Debug)]
pub struct Clip {
    pub start: Time,
    pub len: Time,
    pub source: String,
    pub source_offset: Time,
    pub name: String,
}

/// Keyframes for a named target.
#[derive(Clone, PartialEq, Debug)]
pub struct Channel {
    pub target: String,
    pub keys: Vec<Keyframe>,
}

/// A scalar keyframe with optional incoming and outgoing handle pairs.
#[derive(Clone, PartialEq, Debug)]
pub struct Keyframe {
    pub time: Time,
    pub value: f64,
    pub interp: Interp,
    /// Incoming and outgoing `(time_offset_in_ticks, value_offset)` handles.
    pub handles: Option<[(f64, f64); 2]>,
}

/// Interpolation mode for a keyframe.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interp {
    Step,
    Linear,
    Bezier,
    Hermite,
}
