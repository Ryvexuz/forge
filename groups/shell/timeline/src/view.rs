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

    /// Conversion truncates toward zero; intermediate products are checked.
    pub fn time_to_pixel(&self, time: Time) -> Result<i32, ViewError> {
        let ticks = i128::from(time.0) - i128::from(self.scroll.0);
        let pixels = ticks
            .checked_mul(i128::from(self.zoom))
            .ok_or(ViewError::Overflow)?
            / i128::from(TICKS_PER_SECOND);
        i32::try_from(pixels).map_err(|_| ViewError::Overflow)
    }

    pub fn pixel_to_time(&self, x: i32) -> Result<Time, ViewError> {
        let ticks = i128::from(x) * i128::from(TICKS_PER_SECOND) / i128::from(self.zoom)
            + i128::from(self.scroll.0);
        i64::try_from(ticks)
            .map(Time)
            .map_err(|_| ViewError::Overflow)
    }

    /// Like `time_to_pixel`, but a position outside the `i32` range saturates (it is off screen).
    fn time_to_pixel_clamped(&self, time: Time) -> i32 {
        match self.time_to_pixel(time) {
            Ok(px) => px,
            Err(_) if time.0 < self.scroll.0 => i32::MIN,
            Err(_) => i32::MAX,
        }
    }

    pub fn layout(&self, track: usize, clip: usize) -> Result<Option<Rect>, ViewError> {
        self.layout_with(track, clip, false)
    }

    /// With `clamp`, pixel positions outside the `i32` range saturate instead of failing.
    fn layout_with(
        &self,
        track: usize,
        clip: usize,
        clamp: bool,
    ) -> Result<Option<Rect>, ViewError> {
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
        let (x, right) = if clamp {
            (
                self.time_to_pixel_clamped(Time(start)),
                self.time_to_pixel_clamped(Time(end)),
            )
        } else {
            (
                self.time_to_pixel(Time(start))?,
                self.time_to_pixel(Time(end))?,
            )
        };
        let y = i32::try_from(track)
            .ok()
            .and_then(|t| t.checked_mul(TRACK_HEIGHT))
            .and_then(|t| t.checked_add(RULER_HEIGHT))
            .ok_or(ViewError::Overflow)?;
        Ok(Some(Rect {
            x,
            y,
            width: if clamp {
                right.saturating_sub(x)
            } else {
                right.checked_sub(x).ok_or(ViewError::Overflow)?
            },
            height: TRACK_HEIGHT,
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
        for (channel, data) in row.channels.iter().enumerate() {
            for (key, data) in data.keys.iter().enumerate() {
                // A key whose pixel does not fit an i32 is off screen and cannot be hit.
                let Ok(px) = self.time_to_pixel(data.time) else {
                    continue;
                };
                if (i64::from(x) - i64::from(px)).abs() <= i64::from(HANDLE) {
                    return Ok(Target::Key {
                        track,
                        channel,
                        key,
                    });
                }
            }
        }
        for clip in 0..row.clips.len() {
            if let Some(rect) = self.layout_with(track, clip, true)? {
                let right = i64::from(rect.x) + i64::from(rect.width);
                if (i64::from(x) - i64::from(rect.x)).abs() <= i64::from(HANDLE) {
                    return Ok(Target::LeftHandle { track, clip });
                }
                if (i64::from(x) - right).abs() <= i64::from(HANDLE) {
                    return Ok(Target::RightHandle { track, clip });
                }
                if i64::from(x) > i64::from(rect.x) && i64::from(x) < right {
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
                    return Ok(self.drag.take().map(|d| Edit::MoveClip {
                        track: d.track,
                        clip: d.clip,
                        delta: d.delta,
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
            let y = i32::try_from(track)
                .ok()
                .and_then(|t| t.checked_mul(TRACK_HEIGHT))
                .and_then(|t| t.checked_add(RULER_HEIGHT))
                .ok_or(ViewError::Overflow)?;
            out.push_str(&format!("text 0 {y} label {:?}\n", row.name));
            for (clip, item) in row.clips.iter().enumerate() {
                if let Some(r) = self.layout_with(track, clip, true)? {
                    out.push_str(&format!(
                        "rect {} {} {} {} clip\n",
                        r.x, r.y, r.width, r.height
                    ));
                    out.push_str(&format!("text {} {} label {:?}\n", r.x, r.y, item.name));
                }
            }
            for channel in &row.channels {
                for key in &channel.keys {
                    let Ok(x) = self.time_to_pixel(key.time) else {
                        continue; // off screen
                    };
                    out.push_str(&format!(
                        "line {x} {y} {x} {} key\n",
                        y.checked_add(TRACK_HEIGHT).ok_or(ViewError::Overflow)?
                    ));
                }
            }
        }
        let x = self.time_to_pixel_clamped(self.playhead);
        out.push_str(&format!("line {x} 0 {x} {} playhead\n", self.size.1));
        Ok(out)
    }
}
