//! In-process C boundary: handles are generation-tagged table indices, not pointers.
//! Every entry catches unwinding and returns TL_OK or a named negative error code.
//! Audio is fixed synthetic 440 Hz, amplitude 0.5, mono at 48 kHz; video is the
//! synthetic frame-index source at 24 fps. No media files are opened.
//! Boundary unsafe code is confined to this file; callers must provide valid,
//! aligned, live storage for all non-null pointers, with no concurrent mutation
//! or overlapping input/output storage during a call. Strings are NUL-terminated.
//! TL10b owns Python/ctypes and the cdylib-location spike under Broadsword.

use std::ffi::{c_char, CStr};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Mutex;

use crate::anim::AnimationDriver;
use crate::audio::{tone, AudioDriver};
use crate::driver::TrackDriver;
use crate::edit::{Edit, UndoStack};
use crate::format;
use crate::time::{Rate, Time};
use crate::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};
use crate::video::VideoDriver;

pub const TL_OK: i32 = 0;
pub const TL_ERR_NULL_HANDLE: i32 = -1;
pub const TL_ERR_STALE_HANDLE: i32 = -2;
pub const TL_ERR_NULL_POINTER: i32 = -3;
pub const TL_ERR_BUFFER_TOO_SMALL: i32 = -4;
pub const TL_ERR_BAD_TRACK: i32 = -5;
pub const TL_ERR_BAD_TIME: i32 = -6;
pub const TL_ERR_BAD_VALUE: i32 = -7;
pub const TL_ERR_BAD_TEXT: i32 = -8;
pub const TL_ERR_EDIT_REFUSED: i32 = -9;
pub const TL_ERR_NOT_IN_CLIP: i32 = -10;
pub const TL_ERR_FORMAT: i32 = -11;
pub const TL_ERR_EVAL: i32 = -12;
pub const TL_ERR_PANIC: i32 = -13;
pub const TL_ERR_POISONED: i32 = -14;
pub const TL_ERR_LIMIT: i32 = -15;
pub const TL_INTERP_STEP: i32 = 0;
pub const TL_INTERP_LINEAR: i32 = 1;

// Fixed API capacities, not tunable engine settings; also documented in the header.
const MAX_HANDLES: usize = 4096;
const MAX_AUDIO_BLOCK: usize = 1_048_576;

struct State {
    timeline: Timeline,
    undo: UndoStack,
    playhead: Time,
}

struct Slot {
    generation: u32,
    state: Option<State>,
}

struct Table {
    slots: Vec<Slot>,
}

static TABLE: Mutex<Table> = Mutex::new(Table { slots: Vec::new() });

impl Table {
    fn index(&self, handle: u64) -> Result<usize, i32> {
        if handle == 0 {
            return Err(TL_ERR_NULL_HANDLE);
        }
        let low = handle as u32;
        let index = low.checked_sub(1).ok_or(TL_ERR_STALE_HANDLE)? as usize;
        let slot = self.slots.get(index).ok_or(TL_ERR_STALE_HANDLE)?;
        if slot.generation != (handle >> 32) as u32 || slot.state.is_none() {
            return Err(TL_ERR_STALE_HANDLE);
        }
        Ok(index)
    }
}

fn guard(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(TL_ERR_PANIC)
}

fn with_table(f: impl FnOnce(&mut Table) -> Result<(), i32>) -> i32 {
    match TABLE.lock() {
        Ok(mut table) => match f(&mut table) {
            Ok(()) => TL_OK,
            Err(code) => code,
        },
        Err(_) => TL_ERR_POISONED,
    }
}

fn with_state(handle: u64, f: impl FnOnce(&mut State) -> Result<(), i32>) -> i32 {
    with_table(|table| {
        let index = table.index(handle)?;
        f(table.slots[index]
            .state
            .as_mut()
            .ok_or(TL_ERR_STALE_HANDLE)?)
    })
}

fn non_null<T>(ptr: *const T) -> Result<(), i32> {
    if ptr.is_null() {
        Err(TL_ERR_NULL_POINTER)
    } else {
        Ok(())
    }
}

unsafe fn text(ptr: *const c_char, path: bool) -> Result<String, i32> {
    // SAFETY: caller checked non-null and promises a live NUL-terminated string.
    let value = unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| TL_ERR_BAD_TEXT)?;
    if value.is_empty()
        || value.chars().any(char::is_control)
        || (path && (value.contains('\\') || value.contains(':')))
    {
        return Err(TL_ERR_BAD_TEXT);
    }
    Ok(value.to_owned())
}

fn track_index(state: &State, id: u64, kind: Option<&str>) -> Result<usize, i32> {
    state
        .timeline
        .tracks
        .iter()
        .position(|track| track.id == id && kind.is_none_or(|kind| track.kind == kind))
        .ok_or(TL_ERR_BAD_TRACK)
}

fn clip_at(track: &Track, playhead: Time) -> Result<&Clip, i32> {
    track
        .clips
        .iter()
        .find(|clip| {
            clip.start.0 <= playhead.0
                && clip
                    .start
                    .0
                    .checked_add(clip.len.0)
                    .is_some_and(|end| playhead.0 < end)
        })
        .ok_or(TL_ERR_NOT_IN_CLIP)
}

fn apply(state: &mut State, edit: Edit) -> Result<(), i32> {
    state
        .undo
        .apply(&mut state.timeline, edit)
        .map_err(|_| TL_ERR_EDIT_REFUSED)
}

/// Create a handle.
/// # Safety
/// `out_handle` must be writable for one u64 when non-null.
#[no_mangle]
pub unsafe extern "C" fn tl_create(out_handle: *mut u64) -> i32 {
    guard(|| {
        with_table(|table| {
            non_null(out_handle)?;
            let index = match table
                .slots
                .iter()
                .position(|slot| slot.state.is_none() && slot.generation != 0)
            {
                Some(index) => index,
                None => {
                    if table.slots.len() == MAX_HANDLES {
                        return Err(TL_ERR_LIMIT);
                    }
                    table.slots.push(Slot {
                        generation: 1,
                        state: None,
                    });
                    table.slots.len() - 1
                }
            };
            let slot = &mut table.slots[index];
            slot.state = Some(State {
                timeline: Timeline { tracks: Vec::new() },
                undo: UndoStack::default(),
                playhead: Time(0),
            });
            let handle = (u64::from(slot.generation) << 32) | (index as u64 + 1);
            // SAFETY: non-null checked; caller provides writable u64 storage.
            unsafe { out_handle.write(handle) };
            Ok(())
        })
    })
}

/// Free a handle; generation MAX retires a slot instead of wrapping.
#[no_mangle]
pub extern "C" fn tl_free(handle: u64) -> i32 {
    guard(|| {
        with_table(|table| {
            let index = table.index(handle)?;
            let slot = &mut table.slots[index];
            slot.state = None;
            if slot.generation == u32::MAX {
                // Zero is never issued and marks a permanently retired slot.
                slot.generation = 0;
            } else {
                slot.generation += 1;
            }
            Ok(())
        })
    })
}

/// Add a track, with exactly one channel for animation.
/// # Safety
/// Non-null text pointers must satisfy the module's C string contract.
#[no_mangle]
pub unsafe extern "C" fn tl_add_track(
    handle: u64,
    track_id: u64,
    kind: *const c_char,
    name: *const c_char,
    channel_target: *const c_char,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(kind)?;
            non_null(name)?;
            // SAFETY: required pointers checked; optional target read only if non-null.
            let (kind, name, target) = unsafe {
                (
                    text(kind, false)?,
                    text(name, false)?,
                    if channel_target.is_null() {
                        None
                    } else {
                        Some(text(channel_target, true)?)
                    },
                )
            };
            if !matches!(kind.as_str(), "animation" | "audio" | "video")
                || (kind == "animation") != target.is_some()
            {
                return Err(TL_ERR_BAD_VALUE);
            }
            let channels = target
                .into_iter()
                .map(|target| Channel {
                    target,
                    keys: Vec::new(),
                })
                .collect();
            let index = state.timeline.tracks.len();
            apply(
                state,
                Edit::AddTrack {
                    index,
                    track: Track {
                        id: track_id,
                        kind,
                        name,
                        clips: Vec::new(),
                        channels,
                    },
                },
            )
        })
    })
}

/// Add a clip.
/// # Safety
/// Non-null text pointers must satisfy the module's C string contract.
#[no_mangle]
pub unsafe extern "C" fn tl_add_clip(
    handle: u64,
    track_id: u64,
    start: i64,
    len: i64,
    source_offset: i64,
    source: *const c_char,
    name: *const c_char,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(source)?;
            non_null(name)?;
            // SAFETY: both pointers checked; caller supplies live C strings.
            let (source, name) = unsafe { (text(source, true)?, text(name, false)?) };
            if start < 0 || len <= 0 || source_offset < 0 || start.checked_add(len).is_none() {
                return Err(TL_ERR_BAD_TIME);
            }
            let source_len = Time(source_offset.checked_add(len).ok_or(TL_ERR_BAD_TIME)?);
            let track = track_index(state, track_id, None)?;
            apply(
                state,
                Edit::AddClip {
                    track,
                    clip: Clip {
                        start: Time(start),
                        len: Time(len),
                        source_offset: Time(source_offset),
                        source,
                        name,
                    },
                    source_len,
                },
            )
        })
    })
}

/// Insert an animation key.
#[no_mangle]
pub extern "C" fn tl_add_key(
    handle: u64,
    track_id: u64,
    channel: u32,
    time: i64,
    value: f64,
    interp: u32,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            if time < 0 {
                return Err(TL_ERR_BAD_TIME);
            }
            let interp = match interp {
                0 => Interp::Step,
                1 => Interp::Linear,
                _ => return Err(TL_ERR_BAD_VALUE),
            };
            let track = track_index(state, track_id, Some("animation"))?;
            let channels = state.timeline.tracks[track].channels.len();
            if !value.is_finite() || channel as usize >= channels {
                return Err(TL_ERR_BAD_VALUE);
            }
            apply(
                state,
                Edit::InsertKey {
                    track,
                    channel: channel as usize,
                    key: Keyframe {
                        time: Time(time),
                        value,
                        interp,
                        handles: None,
                    },
                },
            )
        })
    })
}

/// Set the playhead without editing history.
#[no_mangle]
pub extern "C" fn tl_scrub(handle: u64, time: i64) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            if time < 0 {
                return Err(TL_ERR_BAD_TIME);
            }
            state.playhead = Time(time);
            Ok(())
        })
    })
}

/// Read an animation channel at the playhead.
/// # Safety
/// Non-null `out_value` must be writable for one f64.
#[no_mangle]
pub unsafe extern "C" fn tl_read_value(
    handle: u64,
    track_id: u64,
    channel: u32,
    out_value: *mut f64,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_value)?;
            let track = &state.timeline.tracks[track_index(state, track_id, Some("animation"))?];
            if channel as usize >= track.channels.len() {
                return Err(TL_ERR_BAD_VALUE);
            }
            let clip = clip_at(track, state.playhead)?;
            let driver = AnimationDriver::new(clip.clone(), track.channels.clone())
                .map_err(|_| TL_ERR_EVAL)?;
            let end = state
                .playhead
                .checked_add(Time(1))
                .map_err(|_| TL_ERR_EVAL)?;
            let mut values = vec![0.0; track.channels.len()];
            driver
                .evaluate(state.playhead..end, &mut values)
                .map_err(|_| TL_ERR_EVAL)?;
            if values.iter().any(|value| !value.is_finite()) {
                return Err(TL_ERR_EVAL);
            }
            // SAFETY: checked non-null; caller supplies writable f64 storage.
            unsafe { out_value.write(values[channel as usize]) };
            Ok(())
        })
    })
}

/// Read fixed synthetic audio, including silence outside clips.
/// # Safety
/// Non-null `out_samples` must be aligned and writable for `len` f64 values.
#[no_mangle]
pub unsafe extern "C" fn tl_read_audio(
    handle: u64,
    track_id: u64,
    out_samples: *mut f64,
    len: usize,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_samples)?;
            if len > MAX_AUDIO_BLOCK {
                return Err(TL_ERR_BAD_VALUE);
            }
            let track = &state.timeline.tracks[track_index(state, track_id, Some("audio"))?];
            if len == 0 {
                return Ok(());
            }
            let duration = Time::from_samples(len as i64, 48_000).map_err(|_| TL_ERR_EVAL)?;
            let end = state
                .playhead
                .checked_add(duration)
                .map_err(|_| TL_ERR_EVAL)?;
            let mut driver = AudioDriver::new(48_000).map_err(|_| TL_ERR_EVAL)?;
            let source = tone(440.0, 0.5).map_err(|_| TL_ERR_EVAL)?;
            for clip in &track.clips {
                let end = clip.start.checked_add(clip.len).map_err(|_| TL_ERR_EVAL)?;
                driver
                    .add_clip(clip.start..end, source, 0, 0)
                    .map_err(|_| TL_ERR_EVAL)?;
            }
            let mut values = vec![0.0; len];
            driver
                .evaluate(state.playhead..end, &mut values)
                .map_err(|_| TL_ERR_EVAL)?;
            if values.iter().any(|value| !value.is_finite()) {
                return Err(TL_ERR_EVAL);
            }
            // SAFETY: checked pointer and capped length; caller supplies len writable f64s.
            unsafe { std::ptr::copy_nonoverlapping(values.as_ptr(), out_samples, len) };
            Ok(())
        })
    })
}

/// Read the synthetic video frame index at the playhead.
/// # Safety
/// Non-null `out_frame` must be writable for one u64.
#[no_mangle]
pub unsafe extern "C" fn tl_read_video_frame(
    handle: u64,
    track_id: u64,
    out_frame: *mut u64,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_frame)?;
            let track = &state.timeline.tracks[track_index(state, track_id, Some("video"))?];
            let clip = clip_at(track, state.playhead)?;
            let rate = Rate::new(24, 1).map_err(|_| TL_ERR_EVAL)?;
            let driver = VideoDriver::new(rate, clip.start, clip.len, clip.source_offset)
                .map_err(|_| TL_ERR_EVAL)?;
            let frame = driver.frame_at(state.playhead).map_err(|_| TL_ERR_EVAL)?;
            // SAFETY: checked non-null; caller supplies writable u64 storage.
            unsafe { out_frame.write(u64::from_le_bytes(frame.pixels)) };
            Ok(())
        })
    })
}

/// Undo one edit.
/// # Safety
/// Non-null `out_done` must be writable for one i32.
#[no_mangle]
pub unsafe extern "C" fn tl_undo(handle: u64, out_done: *mut i32) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_done)?;
            let done = state
                .undo
                .undo(&mut state.timeline)
                .map_err(|_| TL_ERR_EDIT_REFUSED)?;
            // SAFETY: checked non-null; caller supplies writable i32 storage.
            unsafe { out_done.write(i32::from(done)) };
            Ok(())
        })
    })
}

/// Redo one edit.
/// # Safety
/// Non-null `out_done` must be writable for one i32.
#[no_mangle]
pub unsafe extern "C" fn tl_redo(handle: u64, out_done: *mut i32) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_done)?;
            let done = state
                .undo
                .redo(&mut state.timeline)
                .map_err(|_| TL_ERR_EDIT_REFUSED)?;
            // SAFETY: checked non-null; caller supplies writable i32 storage.
            unsafe { out_done.write(i32::from(done)) };
            Ok(())
        })
    })
}

/// Save canonical bytes without a NUL; out_len is also written on insufficient capacity.
/// # Safety
/// Non-null `out_len` is writable for one usize; non-null `buf` for `cap` bytes.
/// These output regions must not overlap.
#[no_mangle]
pub unsafe extern "C" fn tl_save(
    handle: u64,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(out_len)?;
            let text = format::write(&state.timeline);
            let n = text.len();
            // SAFETY: checked non-null; caller supplies writable usize storage.
            unsafe { out_len.write(n) };
            if cap < n {
                return Err(TL_ERR_BUFFER_TOO_SMALL);
            }
            non_null(buf)?;
            // SAFETY: cap >= n and non-null checked; caller's buffer is writable and disjoint.
            unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buf, n) };
            Ok(())
        })
    })
}

/// Load atomically, resetting history and playhead on success.
/// # Safety
/// Non-null `buf` must be readable for `len` bytes in one allocation.
#[no_mangle]
pub unsafe extern "C" fn tl_load(handle: u64, buf: *const u8, len: usize) -> i32 {
    guard(|| {
        with_state(handle, |state| {
            non_null(buf)?;
            if len > isize::MAX as usize {
                return Err(TL_ERR_FORMAT);
            }
            // SAFETY: checked non-null and slice size; caller provides len readable bytes.
            let bytes = unsafe { std::slice::from_raw_parts(buf, len) };
            let text = std::str::from_utf8(bytes).map_err(|_| TL_ERR_FORMAT)?;
            let parsed = format::parse(text).map_err(|_| TL_ERR_FORMAT)?;
            Edit::Restore(parsed.clone())
                .apply(&mut Timeline { tracks: Vec::new() })
                .map_err(|_| TL_ERR_FORMAT)?;
            state.timeline = parsed;
            state.undo = UndoStack::default();
            state.playhead = Time(0);
            Ok(())
        })
    })
}

/// Boundary self-test: deliberately panic without acquiring or poisoning TABLE.
#[no_mangle]
pub extern "C" fn tl_selftest_panic() -> i32 {
    guard(|| panic!("timeline C boundary self-test"))
}
