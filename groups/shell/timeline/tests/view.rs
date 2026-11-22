use timeline::edit::Edit;
use timeline::format::parse;
use timeline::time::{Time, TICKS_PER_SECOND};
use timeline::track::{Clip, Timeline, Track};
use timeline::view::{Modifiers, Target, TimelineView, ViewError, ViewInput};

fn pointer(x: i32, up: bool) -> ViewInput {
    if up {
        ViewInput::PointerUp {
            x,
            y: 40,
            modifiers: Modifiers::default(),
        }
    } else {
        ViewInput::PointerDown {
            x,
            y: 40,
            modifiers: Modifiers::default(),
        }
    }
}

#[test]
fn view_hit_test_round_trip() {
    for zoom in [1, 2, 24, 48_000, 96_000] {
        for seconds in [0, 36_000] {
            let scroll = seconds * TICKS_PER_SECOND;
            let timeline = Timeline {
                tracks: vec![Track {
                    id: 1,
                    kind: "audio".into(),
                    name: "test".into(),
                    clips: vec![Clip {
                        start: Time(scroll + 10 * TICKS_PER_SECOND),
                        len: Time(20 * TICKS_PER_SECOND),
                        source: "test.wav".into(),
                        source_offset: Time(0),
                        name: "clip".into(),
                    }],
                    channels: vec![],
                }],
            };
            let view = TimelineView::new(timeline, zoom, Time(scroll), (4_000_000, 200)).unwrap();
            for offset in [-1, 0, 1, 10 * TICKS_PER_SECOND, 30 * TICKS_PER_SECOND] {
                let time = Time(scroll + offset);
                let px = view.time_to_pixel(time).unwrap();
                // Independent exact rational reference for the forward conversion.
                assert_eq!(
                    i128::from(px),
                    i128::from(offset) * i128::from(zoom) / i128::from(TICKS_PER_SECOND)
                );
                let back = view.pixel_to_time(px).unwrap();
                let tolerance = (TICKS_PER_SECOND as u64).div_ceil(zoom) as i64;
                assert!((back.0 - time.0).abs() <= tolerance);
            }
            let left = (10 * zoom) as i32;
            let right = (30 * zoom) as i32;
            assert_eq!(
                view.hit_test(left, 40).unwrap(),
                Target::LeftHandle { track: 0, clip: 0 }
            );
            assert_eq!(
                view.hit_test(right, 40).unwrap(),
                Target::RightHandle { track: 0, clip: 0 }
            );
            assert_eq!(
                view.hit_test((left + right) / 2, 40).unwrap(),
                Target::Clip { track: 0, clip: 0 }
            );
            assert_eq!(view.hit_test(5, 5).unwrap(), Target::Ruler);
            assert_eq!(view.hit_test(0, 100).unwrap(), Target::Empty);
            // Empty space inside track 0's row: past the right handle, on no clip or handle.
            let beyond = right + 20;
            assert_eq!(view.hit_test(beyond, 40).unwrap(), Target::Empty);
        }
    }
}

#[test]
fn view_drawlist_matches_golden() {
    let timeline = parse(include_str!("fixtures/timeline_v1.tl")).unwrap();
    // One tick per pixel makes the small integer-tick v1 fixture visible.
    let mut view =
        TimelineView::new(timeline, TICKS_PER_SECOND as u64, Time(0), (320, 180)).unwrap();
    let mut actual = String::from("state start\n");
    actual.push_str(&view.draw_list().unwrap());
    view.input(ViewInput::ScrubTo(Time(40))).unwrap();
    actual.push_str("state middle\n");
    actual.push_str(&view.draw_list().unwrap());
    assert_eq!(
        view.hit_test(20, 140).unwrap(),
        Target::Key {
            track: 2,
            channel: 0,
            key: 2,
        }
    );
    assert_eq!(view.input(pointer(30, false)).unwrap(), None);
    view.input(ViewInput::PointerMove {
        x: 50,
        y: 40,
        modifiers: Modifiers::default(),
    })
    .unwrap();
    actual.push_str("state drag\n");
    actual.push_str(&view.draw_list().unwrap());
    assert_eq!(actual, include_str!("fixtures/widget_scrub.drawlist"));
    assert_eq!(
        view.input(pointer(50, true)).unwrap(),
        Some(Edit::MoveClip {
            track: 0,
            clip: 0,
            delta: Time(20),
        })
    );
    assert_eq!(view.input(pointer(50, true)).unwrap(), None);
    assert_eq!(view.timeline.tracks[0].clips[0].start, Time(-10));
}

#[test]
fn view_errors_and_scrubbing() {
    let empty = Timeline { tracks: vec![] };
    assert!(matches!(
        TimelineView::new(empty.clone(), 0, Time(0), (1, 1)),
        Err(ViewError::InvalidZoom)
    ));
    assert!(matches!(
        TimelineView::new(empty.clone(), 1, Time(0), (0, 1)),
        Err(ViewError::InvalidSize)
    ));
    let mut view = TimelineView::new(empty, 1, Time(i64::MAX), (320, 180)).unwrap();
    assert_eq!(view.pixel_to_time(1), Err(ViewError::Overflow));
    let extreme =
        TimelineView::new(empty_timeline(), u64::MAX, Time(i64::MAX), (320, 180)).unwrap();
    assert_eq!(
        extreme.time_to_pixel(Time(i64::MIN)),
        Err(ViewError::Overflow)
    );
    view.input(ViewInput::ScrubTo(Time(i64::MIN))).unwrap();
    assert_eq!(view.playhead, Time(i64::MIN));
    let mut view = TimelineView::new(Timeline { tracks: vec![] }, 1, Time(0), (320, 180)).unwrap();
    view.input(ViewInput::PointerDown {
        x: 1,
        y: 2,
        modifiers: Modifiers::default(),
    })
    .unwrap();
    view.input(ViewInput::PointerUp {
        x: 2,
        y: 2,
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert_eq!(view.playhead, Time(2 * TICKS_PER_SECOND));
    view.input(ViewInput::Wheel { pixels: 3 }).unwrap();
    assert_eq!(view.pixel_to_time(0).unwrap(), Time(3 * TICKS_PER_SECOND));
}

fn empty_timeline() -> Timeline {
    Timeline { tracks: vec![] }
}

#[test]
fn view_offscreen_items_do_not_fail_hit_test_or_draw_list() {
    let far = 36_000 * TICKS_PER_SECOND;
    let clip = |start: i64, name: &str| Clip {
        start: Time(start),
        len: Time(20 * TICKS_PER_SECOND),
        source: "test.wav".into(),
        source_offset: Time(0),
        name: name.into(),
    };
    let timeline = Timeline {
        tracks: vec![Track {
            id: 1,
            kind: "audio".into(),
            name: "test".into(),
            clips: vec![clip(0, "near"), clip(far, "far")],
            channels: vec![],
        }],
    };
    // At 96,000 px/s the far clip is about 3.4e9 px away: it does not fit an i32.
    let view = TimelineView::new(timeline.clone(), 96_000, Time(0), (4_000_000, 200)).unwrap();
    assert_eq!(
        view.hit_test(960_000, 40).unwrap(),
        Target::Clip { track: 0, clip: 0 }
    );
    let list = view.draw_list().unwrap();
    assert!(list.contains("\"near\""));
    // Scrolled to the far end: the near clip is off screen on the left.
    let view = TimelineView::new(timeline, 96_000, Time(far), (4_000_000, 200)).unwrap();
    assert_eq!(
        view.hit_test(960_000, 40).unwrap(),
        Target::Clip { track: 0, clip: 1 }
    );
    assert!(view.draw_list().unwrap().contains("\"far\""));
}

#[test]
fn view_long_clip_starting_offscreen_left_hits_and_draws_visible_part() {
    // A 10-hour timeline at 96,000 px/s; clip 0 lasts 7 hours (> 6.2 h), so its left
    // edge is about -2.4e9 px when scrolled near its end: outside the i32 range.
    let hour = 3_600 * TICKS_PER_SECOND;
    let clip = |start: i64, len: i64, name: &str| Clip {
        start: Time(start),
        len: Time(len),
        source: "test.wav".into(),
        source_offset: Time(0),
        name: name.into(),
    };
    let timeline = Timeline {
        tracks: vec![Track {
            id: 1,
            kind: "audio".into(),
            name: "test".into(),
            clips: vec![
                clip(0, 7 * hour, "long"),
                clip(
                    10 * hour - 20 * TICKS_PER_SECOND,
                    20 * TICKS_PER_SECOND,
                    "end",
                ),
            ],
            channels: vec![],
        }],
    };
    // Scroll to 10 s before the long clip's end: its right edge is at 960,000 px.
    let scroll = Time(7 * hour - 10 * TICKS_PER_SECOND);
    let view = TimelineView::new(timeline, 96_000, scroll, (4_000_000, 200)).unwrap();
    assert_eq!(
        view.hit_test(480_000, 40).unwrap(),
        Target::Clip { track: 0, clip: 0 }
    );
    assert_eq!(
        view.hit_test(960_000, 40).unwrap(),
        Target::RightHandle { track: 0, clip: 0 }
    );
    assert_eq!(view.hit_test(960_010, 40).unwrap(), Target::Empty);
    let list = view.draw_list().unwrap();
    // The visible part: from the viewport's left edge to the clip's right edge.
    assert!(
        list.contains("rect 0 24 960000 48 clip\ntext 0 24 label \"long\"\n"),
        "{list}"
    );
    // The end clip is about 1.0e9 px to the right, past the viewport: not drawn.
    assert!(!list.contains("\"end\""), "{list}");
    // The public i32 contract is unchanged: the long clip's left edge is outside i32.
    assert_eq!(view.layout(0, 0), Err(ViewError::Overflow));
}

#[test]
fn view_click_without_move_yields_no_edit() {
    let timeline = parse(include_str!("fixtures/timeline_v1.tl")).unwrap();
    let mut view =
        TimelineView::new(timeline, TICKS_PER_SECOND as u64, Time(0), (320, 180)).unwrap();
    assert_eq!(view.input(pointer(30, false)).unwrap(), None);
    assert_eq!(view.selection, Target::Clip { track: 0, clip: 0 });
    // Released where it was pressed: a selection click, not a drag.
    assert_eq!(view.input(pointer(30, true)).unwrap(), None);
}

#[test]
fn view_reads_zoom_scroll_and_size() {
    let view = TimelineView::new(empty_timeline(), 96_000, Time(7), (640, 200)).unwrap();
    assert_eq!(view.zoom(), 96_000);
    assert_eq!(view.scroll(), Time(7));
    assert_eq!(view.size(), (640, 200));
}
