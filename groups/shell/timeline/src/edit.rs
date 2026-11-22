//! Atomic, reversible timeline edits. Indices refer to the current timeline.

use crate::time::Time;
use crate::track::{Clip, Keyframe, Timeline, Track};

#[derive(Clone, Debug, PartialEq)]
pub enum Edit {
    InsertKey {
        track: usize,
        channel: usize,
        key: Keyframe,
    },
    MoveKey {
        track: usize,
        channel: usize,
        key: usize,
        time: Time,
    },
    DeleteKey {
        track: usize,
        channel: usize,
        key: usize,
    },
    AddClip {
        track: usize,
        clip: Clip,
        source_len: Time,
    },
    MoveClip {
        track: usize,
        clip: usize,
        delta: Time,
    },
    TrimClip {
        track: usize,
        clip: usize,
        start: Time,
        len: Time,
        source_offset: Time,
        source_len: Time,
    },
    SplitClip {
        track: usize,
        clip: usize,
        at: Time,
    },
    JoinClips {
        track: usize,
        left: usize,
        right: usize,
    },
    RemoveClip {
        track: usize,
        clip: usize,
    },
    AddTrack {
        index: usize,
        track: Track,
    },
    RemoveTrack {
        track: usize,
    },
    /// Exact state restoration, also useful for preserving vector ordering.
    Restore(Timeline),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditError {
    MissingItem,
    DuplicateTime,
    DuplicateTrack,
    InvalidKey,
    InvalidRange,
    SourceRange,
    Overflow,
    Overlap,
    InvalidSplit,
    IncompatibleClips,
}

fn end(clip: &Clip) -> Result<i64, EditError> {
    if clip.len.0 <= 0 || clip.start.0 < 0 || clip.source_offset.0 < 0 {
        return Err(EditError::InvalidRange);
    }
    clip.source_offset
        .0
        .checked_add(clip.len.0)
        .ok_or(EditError::Overflow)?;
    clip.start
        .0
        .checked_add(clip.len.0)
        .ok_or(EditError::Overflow)
}

fn source_range(clip: &Clip, source_len: Time) -> Result<(), EditError> {
    end(clip)?;
    if source_len.0 < 0
        || clip
            .source_offset
            .0
            .checked_add(clip.len.0)
            .ok_or(EditError::Overflow)?
            > source_len.0
    {
        return Err(EditError::SourceRange);
    }
    Ok(())
}

fn validate(timeline: &Timeline) -> Result<(), EditError> {
    for (i, track) in timeline.tracks.iter().enumerate() {
        if timeline.tracks[..i].iter().any(|t| t.id == track.id) {
            return Err(EditError::DuplicateTrack);
        }
        for channel in &track.channels {
            for key in &channel.keys {
                if !key.value.is_finite()
                    || key
                        .handles
                        .is_some_and(|h| h.iter().any(|(t, v)| !t.is_finite() || !v.is_finite()))
                {
                    return Err(EditError::InvalidKey);
                }
            }
            if channel.keys.windows(2).any(|w| w[0].time.0 >= w[1].time.0) {
                return Err(EditError::DuplicateTime);
            }
        }
        for (j, clip) in track.clips.iter().enumerate() {
            let finish = end(clip)?;
            for other in &track.clips[..j] {
                if clip.start.0 < end(other)? && other.start.0 < finish {
                    return Err(EditError::Overlap);
                }
            }
        }
    }
    Ok(())
}

impl Edit {
    /// Apply transactionally; failures leave the timeline byte-for-byte unchanged.
    pub fn apply(&self, timeline: &mut Timeline) -> Result<Edit, EditError> {
        let mut next = timeline.clone();
        let mut inverse = Edit::Restore(timeline.clone());
        match self {
            Edit::Restore(saved) => next = saved.clone(),
            Edit::AddTrack { index, track } => {
                if *index > next.tracks.len() {
                    return Err(EditError::MissingItem);
                }
                next.tracks.insert(*index, track.clone());
            }
            Edit::RemoveTrack { track } => {
                if *track >= next.tracks.len() {
                    return Err(EditError::MissingItem);
                }
                next.tracks.remove(*track);
            }
            Edit::InsertKey {
                track,
                channel,
                key,
            } => {
                let keys = &mut next
                    .tracks
                    .get_mut(*track)
                    .ok_or(EditError::MissingItem)?
                    .channels
                    .get_mut(*channel)
                    .ok_or(EditError::MissingItem)?
                    .keys;
                keys.push(key.clone());
                keys.sort_by_key(|k| k.time.0);
            }
            Edit::MoveKey {
                track,
                channel,
                key,
                time,
            } => {
                let keys = &mut next
                    .tracks
                    .get_mut(*track)
                    .ok_or(EditError::MissingItem)?
                    .channels
                    .get_mut(*channel)
                    .ok_or(EditError::MissingItem)?
                    .keys;
                keys.get_mut(*key).ok_or(EditError::MissingItem)?.time = *time;
                keys.sort_by_key(|k| k.time.0);
            }
            Edit::DeleteKey {
                track,
                channel,
                key,
            } => {
                let keys = &mut next
                    .tracks
                    .get_mut(*track)
                    .ok_or(EditError::MissingItem)?
                    .channels
                    .get_mut(*channel)
                    .ok_or(EditError::MissingItem)?
                    .keys;
                if *key >= keys.len() {
                    return Err(EditError::MissingItem);
                }
                keys.remove(*key);
            }
            edit => {
                let track = match edit {
                    Edit::AddClip { track, .. }
                    | Edit::MoveClip { track, .. }
                    | Edit::TrimClip { track, .. }
                    | Edit::SplitClip { track, .. }
                    | Edit::JoinClips { track, .. }
                    | Edit::RemoveClip { track, .. } => *track,
                    _ => unreachable!(),
                };
                let clips = &mut next
                    .tracks
                    .get_mut(track)
                    .ok_or(EditError::MissingItem)?
                    .clips;
                match edit {
                    Edit::AddClip {
                        clip, source_len, ..
                    } => {
                        source_range(clip, *source_len)?;
                        clips.push(clip.clone());
                    }
                    Edit::MoveClip { clip, delta, .. } => {
                        let item = clips.get_mut(*clip).ok_or(EditError::MissingItem)?;
                        item.start.0 = item
                            .start
                            .0
                            .checked_add(delta.0)
                            .ok_or(EditError::Overflow)?;
                        inverse = Edit::MoveClip {
                            track,
                            clip: *clip,
                            delta: Time(delta.0.checked_neg().ok_or(EditError::Overflow)?),
                        };
                    }
                    Edit::TrimClip {
                        clip,
                        start,
                        len,
                        source_offset,
                        source_len,
                        ..
                    } => {
                        let item = clips.get_mut(*clip).ok_or(EditError::MissingItem)?;
                        item.start = *start;
                        item.len = *len;
                        item.source_offset = *source_offset;
                        source_range(item, *source_len)?;
                    }
                    Edit::RemoveClip { clip, .. } => {
                        if *clip >= clips.len() {
                            return Err(EditError::MissingItem);
                        }
                        clips.remove(*clip);
                    }
                    Edit::SplitClip { clip, at, .. } => {
                        let item = clips.get_mut(*clip).ok_or(EditError::MissingItem)?;
                        let finish = end(item)?;
                        if at.0 <= item.start.0 || at.0 >= finish {
                            return Err(EditError::InvalidSplit);
                        }
                        let left_len = at.0 - item.start.0;
                        let mut right = item.clone();
                        right.start = *at;
                        right.len = Time(finish - at.0);
                        right.source_offset = Time(
                            item.source_offset
                                .0
                                .checked_add(left_len)
                                .ok_or(EditError::Overflow)?,
                        );
                        item.len = Time(left_len);
                        clips.insert(*clip + 1, right);
                    }
                    Edit::JoinClips { left, right, .. } => {
                        let a = clips.get(*left).ok_or(EditError::MissingItem)?;
                        let b = clips.get(*right).ok_or(EditError::MissingItem)?;
                        if left == right
                            || end(a)? != b.start.0
                            || a.source != b.source
                            || a.name != b.name
                            || a.source_offset.0.checked_add(a.len.0) != Some(b.source_offset.0)
                        {
                            return Err(EditError::IncompatibleClips);
                        }
                        let length = a.len.0.checked_add(b.len.0).ok_or(EditError::Overflow)?;
                        clips[*left].len = Time(length);
                        clips.remove(*right);
                    }
                    _ => unreachable!(),
                }
            }
        }
        validate(&next)?;
        *timeline = next;
        Ok(inverse)
    }
}

#[derive(Default, Debug)]
pub struct UndoStack {
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl UndoStack {
    pub fn apply(&mut self, timeline: &mut Timeline, edit: Edit) -> Result<(), EditError> {
        let inverse = edit.apply(timeline)?;
        self.undo.push(inverse);
        self.redo.clear();
        Ok(())
    }

    pub fn undo(&mut self, timeline: &mut Timeline) -> Result<bool, EditError> {
        let Some(edit) = self.undo.last() else {
            return Ok(false);
        };
        let inverse = edit.apply(timeline)?;
        self.undo.pop();
        self.redo.push(inverse);
        Ok(true)
    }

    pub fn redo(&mut self, timeline: &mut Timeline) -> Result<bool, EditError> {
        let Some(edit) = self.redo.last() else {
            return Ok(false);
        };
        let inverse = edit.apply(timeline)?;
        self.redo.pop();
        self.undo.push(inverse);
        Ok(true)
    }
}
