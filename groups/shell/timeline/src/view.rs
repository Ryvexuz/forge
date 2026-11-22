//! Integer-pixel timeline presentation. Zoom is integral pixels per second.

use crate::edit::Edit;
use crate::time::{Time, TICKS_PER_SECOND};
use crate::track::Timeline;

pub const RULER_HEIGHT: i32 = 24;
pub const TRACK_HEIGHT: i32 = 48;
const HANDLE: i32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewError {
    InvalidZoom,
    InvalidSize,
    Overflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Empty,
    Ruler,
    Clip {
        track: usize,
        clip: usize,
    },
    LeftHandle {
        track: usize,
        clip: usize,
    },
    RightHandle {
        track: usize,
        clip: usize,
    },
    Key {
        track: usize,
        channel: usize,
        key: usize,
    },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewInput {
    PointerDown {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    PointerMove {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    PointerUp {
        x: i32,
        y: i32,
        modifiers: Modifiers,
    },
    /// Horizontal scrolling, in pixels.
    Wheel {
        pixels: i32,
    },
    ScrubTo(Time),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    track: usize,
    clip: usize,
    anchor: Time,
    delta: Time,
}

/// Unclipped clip edges in i64 pixels relative to the scroll offset.
#[derive(Clone, Copy, Debug)]
struct Span {
    left: i64,
    right: i64,
    y: i32,
}

fn row_y(track: usize) -> Result<i32, ViewError> {
    i32::try_from(track)
        .ok()
        .and_then(|t| t.checked_mul(TRACK_HEIGHT))
        .and_then(|t| t.checked_add(RULER_HEIGHT))
        .ok_or(ViewError::Overflow)
}

pub struct TimelineView {
    pub timeline: Timeline,
    pub playhead: Time,
    zoom: u64,
    scroll: Time,
    pub selection: Target,
    size: (i32, i32),
    drag: Option<Drag>,
    scrubbing: bool,
}

impl TimelineView {
    pub fn new(
        timeline: Timeline,
        zoom: u64,
        scroll: Time,
        size: (i32, i32),
    ) -> Result<Self, ViewError> {
        if zoom == 0 {
            return Err(ViewError::InvalidZoom);
        }
        if size.0 <= 0 || size.1 <= 0 {
            return Err(ViewError::InvalidSize);
        }
        Ok(Self {
            timeline,
            playhead: Time(0),
            zoom,
            scroll,
            selection: Target::Empty,
            size,
            drag: None,
            scrubbing: false,
        })
    }

    pub fn zoom(&self) -> u64 {
        self.zoom
    }

    pub fn scroll(&self) -> Time {
        self.scroll
    }

    pub fn size(&self) -> (i32, i32) {
        self.size
    }

    /// Conversion truncates toward zero; intermediate products are checked.
    pub fn time_to_pixel(&self, time: Time) -> Result<i32, ViewError> {
        i32::try_from(self.time_to_px64(time)?).map_err(|_| ViewError::Overflow)
    }

    pub fn pixel_to_time(&self, x: i32) -> Result<Time, ViewError> {
        let ticks = i128::from(x) * i128::from(TICKS_PER_SECOND) / i128::from(self.zoom)
            + i128::from(self.scroll.0);
        i64::try_from(ticks)
            .map(Time)
            .map_err(|_| ViewError::Overflow)
    }

    /// The layout pixel space: i64 pixels relative to the scroll offset, never clipped.
    /// Overflow only when the pixel does not fit an i64.
    fn time_to_px64(&self, time: Time) -> Result<i64, ViewError> {
        let ticks = i128::from(time.0) - i128::from(self.scroll.0);
        let pixels = ticks
            .checked_mul(i128::from(self.zoom))
            .ok_or(ViewError::Overflow)?
            / i128::from(TICKS_PER_SECOND);
        i64::try_from(pixels).map_err(|_| ViewError::Overflow)
    }

    pub fn layout(&self, track: usize, clip: usize) -> Result<Option<Rect>, ViewError> {
        let Some(span) = self.span(track, clip)? else {
            return Ok(None);
        };
        let x = i32::try_from(span.left).map_err(|_| ViewError::Overflow)?;
        let width = span
            .right
            .checked_sub(span.left)
            .and_then(|w| i32::try_from(w).ok())
            .ok_or(ViewError::Overflow)?;
        Ok(Some(Rect {
            x,
            y: span.y,
            width,
            height: TRACK_HEIGHT,
        }))
    }

    /// A clip's unclipped edges in the i64 layout space.
    fn span(&self, track: usize, clip: usize) -> Result<Option<Span>, ViewError> {
        let Some(item) = self
            .timeline
            .tracks
            .get(track)
            .and_then(|t| t.clips.get(clip))
        else {
            return Ok(None);
        };
        let delta = self
            .drag
            .filter(|d| d.track == track && d.clip == clip)
            .map_or(0, |d| d.delta.0);
        let start = item.start.0.checked_add(delta).ok_or(ViewError::Overflow)?;
        let end = start.checked_add(item.len.0).ok_or(ViewError::Overflow)?;
        Ok(Some(Span {
            left: self.time_to_px64(Time(start))?,
            right: self.time_to_px64(Time(end))?,
            y: row_y(track)?,
        }))
    }

    pub fn hit_test(&self, x: i32, y: i32) -> Result<Target, ViewError> {
        if x < 0 || y < 0 || x >= self.size.0 || y >= self.size.1 {
            return Ok(Target::Empty);
        }
        if y < RULER_HEIGHT {
            return Ok(Target::Ruler);
        }
        let track = ((y - RULER_HEIGHT) / TRACK_HEIGHT) as usize;
        let Some(row) = self.timeline.tracks.get(track) else {
            return Ok(Target::Empty);
        };
        let x = i64::from(x);
        let near = |edge: i64| x.abs_diff(edge) <= HANDLE as u64;
        for (channel, data) in row.channels.iter().enumerate() {
            for (key, data) in data.keys.iter().enumerate() {
                if near(self.time_to_px64(data.time)?) {
                    return Ok(Target::Key {
                        track,
                        channel,
                        key,
                    });
                }
            }
        }
        for clip in 0..row.clips.len() {
            if let Some(span) = self.span(track, clip)? {
                if near(span.left) {
                    return Ok(Target::LeftHandle { track, clip });
                }
                if near(span.right) {
                    return Ok(Target::RightHandle { track, clip });
                }
                if x > span.left && x < span.right {
                    return Ok(Target::Clip { track, clip });
                }
            }
        }
        Ok(Target::Empty)
    }

    pub fn scrub_to(&mut self, time: Time) {
        self.playhead = time;
    }

    /// Dragging previews only; release returns one edit without applying it.
    pub fn input(&mut self, input: ViewInput) -> Result<Option<Edit>, ViewError> {
        match input {
            ViewInput::ScrubTo(time) => self.scrub_to(time),
            ViewInput::Wheel { pixels } => self.scroll = self.pixel_to_time(pixels)?,
            ViewInput::PointerDown { x, y, .. } => {
                let target = self.hit_test(x, y)?;
                let time = self.pixel_to_time(x)?;
                self.selection = target;
                self.drag = match target {
                    Target::Clip { track, clip } => Some(Drag {
                        track,
                        clip,
                        anchor: time,
                        delta: Time(0),
                    }),
                    _ => None,
                };
                self.scrubbing = target == Target::Ruler;
                if self.scrubbing {
                    self.scrub_to(time);
                }
            }
            ViewInput::PointerMove { x, .. } | ViewInput::PointerUp { x, .. } => {
                let time = self.pixel_to_time(x)?;
                let next = self
                    .drag
                    .map(|mut drag| {
                        drag.delta = Time(
                            time.0
                                .checked_sub(drag.anchor.0)
                                .ok_or(ViewError::Overflow)?,
                        );
                        Ok::<Drag, ViewError>(drag)
                    })
                    .transpose()?;
                if self.scrubbing {
                    self.scrub_to(time);
                }
                self.drag = next;
                if matches!(input, ViewInput::PointerUp { .. }) {
                    self.scrubbing = false;
                    // A click with no movement is a selection, not a drag: no edit.
                    return Ok(self.drag.take().filter(|d| d.delta.0 != 0).map(|d| {
                        Edit::MoveClip {
                            track: d.track,
                            clip: d.clip,
                            delta: d.delta,
                        }
                    }));
                }
            }
        }
        Ok(None)
    }

    /// Records use integer coordinates, symbolic colour IDs and debug-escaped text.
    pub fn draw_list(&self) -> Result<String, ViewError> {
        let mut out = format!("rect 0 0 {} {} background\n", self.size.0, self.size.1);
        out.push_str(&format!("rect 0 0 {} {RULER_HEIGHT} ruler\n", self.size.0));
        for (track, row) in self.timeline.tracks.iter().enumerate() {
            let y = row_y(track)?;
            out.push_str(&format!("text 0 {y} label {:?}\n", row.name));
            for (clip, item) in row.clips.iter().enumerate() {
                let Some(span) = self.span(track, clip)? else {
                    continue;
                };
                // Clip to the viewport only here, at emit time: the visible part, or nothing.
                let left = span.left.max(0);
                let right = span.right.min(i64::from(self.size.0));
                if right <= left {
                    continue; // entirely off screen
                }
                let (x, width) = (to_screen(left)?, to_screen(right - left)?);
                out.push_str(&format!(
                    "rect {x} {} {width} {TRACK_HEIGHT} clip\n",
                    span.y
                ));
                out.push_str(&format!("text {x} {} label {:?}\n", span.y, item.name));
            }
            for channel in &row.channels {
                for key in &channel.keys {
                    let Some(x) = self.visible_x(key.time)? else {
                        continue; // off screen
                    };
                    out.push_str(&format!(
                        "line {x} {y} {x} {} key\n",
                        y.checked_add(TRACK_HEIGHT).ok_or(ViewError::Overflow)?
                    ));
                }
            }
        }
        if let Some(x) = self.visible_x(self.playhead)? {
            out.push_str(&format!("line {x} 0 {x} {} playhead\n", self.size.1));
        }
        Ok(out)
    }

    /// The screen x of a time when it lies in the viewport [0, width], else None.
    fn visible_x(&self, time: Time) -> Result<Option<i32>, ViewError> {
        let x = self.time_to_px64(time)?;
        if (0..=i64::from(self.size.0)).contains(&x) {
            to_screen(x).map(Some)
        } else {
            Ok(None)
        }
    }
}

/// A value already clipped to the viewport always fits an i32.
fn to_screen(v: i64) -> Result<i32, ViewError> {
    i32::try_from(v).map_err(|_| ViewError::Overflow)
}
