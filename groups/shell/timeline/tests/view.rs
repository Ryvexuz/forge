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
