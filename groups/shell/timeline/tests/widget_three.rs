use std::f64::consts::TAU;
use timeline::anim::AnimationDriver;
use timeline::audio::{tone, AudioDriver};
use timeline::driver::{DriverError, TrackDriver};
use timeline::edit::{Edit, UndoStack};
use timeline::time::{Rate, Time, TICKS_PER_SECOND as S};
use timeline::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};
use timeline::video::VideoDriver;
use timeline::view::{Modifiers, Target, TimelineView, ViewInput};

#[derive(Debug)]
struct Outputs {
    anim: Option<f64>,
    audio: Vec<f64>,
    video: Option<u64>,
}

fn fixture() -> Timeline {
    let specs = [
        (1, "animation", "Motion", 0, 10 * S, "motion", S, "Move"),
        (2, "audio", "Tone", S, 6 * S, "tone", 0, "Beep"),
        (3, "video", "Picture", 2 * S, 6 * S, "frames", S / 2, "Shot"),
    ];
    Timeline {
        tracks: specs
            .into_iter()
            .map(|(id, kind, name, start, len, source, offset, clip_name)| Track {
                id,
                kind: kind.into(),
                name: name.into(),
                clips: vec![Clip {
                    start: Time(start),
                    len: Time(len),
                    source: source.into(),
                    source_offset: Time(offset),
                    name: clip_name.into(),
                }],
                channels: if id == 1 {
                    vec![Channel {
                        target: "object/position".into(),
                        keys: [
                            (0, 1.0, Interp::Step),
                            (2 * S, 3.0, Interp::Linear),
                            (8 * S, -1.0, Interp::Linear),
                        ]
                        .into_iter()
                        .map(|(time, value, interp)| Keyframe {
                            time: Time(time),
                            value,
                            interp,
                            handles: None,
                        })
                        .collect(),
                    }]
                } else {
                    vec![]
                },
            })
            .collect(),
    }
}

fn unique_track<'a>(view: &'a TimelineView, kind: &str) -> &'a Track {
    let tracks: Vec<_> = view
        .timeline
        .tracks
        .iter()
        .filter(|t| t.kind == kind)
        .collect();
    assert_eq!(tracks.len(), 1, "outputs step i=all: kind {kind}");
    assert_eq!(tracks[0].clips.len(), 1, "outputs step i=all: clips {kind}");
    tracks[0]
}

fn optional_sample(result: Result<(), DriverError>, value: f64) -> Option<f64> {
    match result {
        Ok(()) => Some(value),
        Err(DriverError::InvalidRange) => None,
        Err(error) => panic!("outputs step i=all: unexpected driver error {error:?}"),
    }
}

fn outputs(view: &TimelineView) -> Outputs {
    let p = view.playhead;
    let track = unique_track(view, "animation");
    let anim = AnimationDriver::new(track.clips[0].clone(), track.channels.clone())
        .expect("outputs step i=all: animation construction");
    assert_eq!(track.kind, anim.kind(), "outputs step i=all: animation kind");
    let mut value = [f64::NAN]; // An unwritten animation sample must fail even when zero is expected.
    let result = anim.evaluate(p..Time(p.0 + 1), &mut value);
    let anim = optional_sample(result, value[0]);

    let mut audio = AudioDriver::new(48_000).expect("outputs step i=all: audio construction");
    let track = unique_track(view, audio.kind());
    let clip = &track.clips[0];
    audio
        .add_clip(
            clip.start..Time(clip.start.0 + clip.len.0),
            tone(440.0, 0.5).expect("outputs step i=all: tone"),
            0,
            0,
        )
        .expect("outputs step i=all: audio clip");
    let mut samples = vec![f64::NAN; 480];
    let block = Time::from_samples(480, 48_000).expect("outputs step i=all: block");
    audio
        .evaluate(p..Time(p.0 + block.0), &mut samples)
        .expect("outputs step i=all: audio evaluate");

    let track = unique_track(view, "video");
    let clip = &track.clips[0];
    let video = VideoDriver::new(
        Rate::new(24, 1).expect("outputs step i=all: rate"),
        clip.start,
        clip.len,
        clip.source_offset,
    )
    .expect("outputs step i=all: video construction");
    assert_eq!(track.kind, video.kind(), "outputs step i=all: video kind");
    let mut pixels = [255.0; 8]; // Unwritten high bytes must expose low-byte-only frame writes.
    let result = video.evaluate(p..Time(p.0 + 1), &mut pixels);
    let video = optional_sample(result, 0.0).map(|_| u64::from_le_bytes(pixels.map(|v| v as u8)));
    Outputs {
        anim,
        audio: samples,
        video,
    }
}

fn sample_start(p: Time) -> i128 {
    let numerator = i128::from(p.0) * 48_000;
    let divisor = i128::from(S);
    numerator.div_euclid(divisor) + i128::from(numerator.rem_euclid(divisor) * 2 > divisor)
}

fn expected(p: Time, video_start: Time) -> Outputs {
    let local = (p.0 + S) as f64;
    let anim = if !(0..10 * S).contains(&p.0) {
        None
    } else if local < (2 * S) as f64 {
        Some(1.0)
    } else if local < (8 * S) as f64 {
        let t = (local - (2 * S) as f64) / (6 * S) as f64;
        Some((1.0 - t) * 3.0 - t)
    } else {
        Some(-1.0)
    };
    let s0 = sample_start(p);
    let audio = (0..480)
        .map(|k| {
            let index = s0 + k;
            if (48_000..336_000).contains(&index) {
                (TAU * ((index - 48_000) as f64 * (440.0 / 48_000.0)).rem_euclid(1.0)).sin()
                    * 0.5
            } else {
                0.0
            }
        })
        .collect();
    let video = if (video_start.0..video_start.0 + 6 * S).contains(&p.0) {
        Some((i128::from(S / 2 + p.0 - video_start.0) * 24 / i128::from(S)) as u64)
    } else {
        None
    };
    Outputs {
        anim,
        audio,
        video,
    }
}

#[derive(Default, Debug)]
struct Cases {
    step: usize,
    linear: usize,
    silent: usize,
    sounding: usize,
    absent: usize,
    present: usize,
}

fn compare(view: &TimelineView, p: Time, start: Time, step: &str, i: usize, c: &mut Cases) {
    assert_eq!(view.playhead, p, "{step} i={i}: playhead");
    let got = outputs(view);
    let want = expected(p, start);
    match (got.anim, want.anim) {
        (Some(a), Some(b)) => {
            assert!((a - b).abs() <= 1e-12, "{step} i={i}: anim {a} != {b}")
        }
        (a, b) => assert_eq!(a, b, "{step} i={i}: animation presence"),
    }
    assert_eq!(got.audio.len(), 480, "{step} i={i}: block length");
    for (k, (a, b)) in got.audio.iter().zip(&want.audio).enumerate() {
        assert!(
            (a - b).abs() <= 1e-12,
            "{step} i={i}: audio k={k}: {a} != {b}"
        );
        if !(48_000..336_000).contains(&(sample_start(p) + k as i128)) {
            assert_eq!(*a, 0.0, "{step} i={i}: silence k={k}");
        }
    }
    assert_eq!(got.video, want.video, "{step} i={i}: video");
    c.step += usize::from((0..S).contains(&p.0));
    c.linear += usize::from((S..7 * S).contains(&p.0));
    if want.audio.iter().all(|v| *v == 0.0) {
        c.silent += 1;
    } else {
        c.sounding += 1;
    }
    if want.video.is_some() {
        c.present += 1;
    } else {
        c.absent += 1;
    }
}

fn pointer(phase: u8, x: i32, y: i32) -> ViewInput {
    let modifiers = Modifiers::default();
    match phase {
        0 => ViewInput::PointerDown { x, y, modifiers },
        1 => ViewInput::PointerMove { x, y, modifiers },
        _ => ViewInput::PointerUp { x, y, modifiers },
    }
}

#[test]
fn one_widget_drives_three_track_kinds() {
    let original = fixture();
    let mut view = TimelineView::new(original.clone(), 100, Time(0), (1200, 168))
        .expect("setup i=0: view");
    let mut undo = UndoStack::default();
    let mut cases = Cases::default();
    let script = [
        (0, 350, 10, 35 * S / 10),
        (2, 350, 10, 35 * S / 10),
        (0, 100, 10, S),
        (1, 420, 10, 42 * S / 10),
        (1, 610, 12, 61 * S / 10),
        (2, 610, 12, 61 * S / 10),
    ];
    for (i, (phase, x, y, p)) in script.into_iter().enumerate() {
        assert_eq!(view.input(pointer(phase, x, y)), Ok(None), "ruler i={i}");
        compare(&view, Time(p), Time(2 * S), "ruler", i, &mut cases);
    }
    assert_eq!(
        view.hit_test(500, 140),
        Ok(Target::Clip { track: 2, clip: 0 }),
        "clip i=0: hit"
    );
    for (i, (phase, x)) in [(0, 500), (1, 550)].into_iter().enumerate() {
        assert_eq!(view.input(pointer(phase, x, 140)), Ok(None), "preview i={i}");
        assert_eq!(view.timeline, original, "preview i={i}: unchanged");
        compare(&view, Time(61 * S / 10), Time(2 * S), "preview", i, &mut cases);
    }
    let returned = view.input(pointer(2, 600, 140));
    assert_eq!(
        returned,
        Ok(Some(Edit::MoveClip {
            track: 2,
            clip: 0,
            delta: Time(S)
        })),
        "release i=0: one move"
    );
    assert_eq!(view.timeline, original, "release i=0: unapplied");
    compare(&view, Time(61 * S / 10), Time(2 * S), "release", 0, &mut cases);
    undo.apply(
        &mut view.timeline,
        returned.expect("release i=0").expect("release i=0: edit"),
    )
    .expect("apply i=0");
    compare(&view, Time(61 * S / 10), Time(3 * S), "apply", 0, &mut cases);
    for i in 0..1000 {
        let p = Time(i as i64 * S / 100);
        assert_eq!(view.input(ViewInput::ScrubTo(p)), Ok(None), "scrub i={i}");
        compare(&view, p, Time(3 * S), "scrub", i, &mut cases);
    }
    assert_eq!(undo.undo(&mut view.timeline), Ok(true), "undo i=0");
    assert_eq!(view.timeline, original, "undo i=0: all tracks restored");
    assert_eq!(
        undo.undo(&mut view.timeline),
        Ok(false),
        "undo i=1: exactly one edit"
    );
    compare(&view, Time(999 * S / 100), Time(2 * S), "undo", 1, &mut cases);
    println!("coverage step i=all: {cases:?}");
    for (name, count) in [
        ("step", cases.step),
        ("linear", cases.linear),
        ("silent", cases.silent),
        ("sounding", cases.sounding),
        ("absent", cases.absent),
        ("present", cases.present),
    ] {
        assert!(count >= 1, "coverage i=all: {name}={count}");
    }
}

#[test]
fn widget_three_reference_matches_hand_values() {
    let p = Time(61 * S / 10);
    let before = expected(p, Time(2 * S));
    assert!(
        (before.anim.expect("hand i=0: anim") - (-0.4)).abs() <= 1e-12,
        "hand i=0: animation"
    );
    assert_eq!(sample_start(p), 292_800, "hand i=0: sample start");
    assert_eq!(before.video, Some(110), "hand i=0: video before");
    assert_eq!(
        expected(p, Time(3 * S)).video,
        Some(86),
        "hand i=1: video after"
    );
    let zero = expected(Time(0), Time(2 * S));
    assert_eq!(zero.anim, Some(1.0), "hand i=2: step");
    assert!(zero.audio.iter().all(|v| *v == 0.0), "hand i=2: silence");
    assert_eq!(zero.video, None, "hand i=2: no video");
    assert!(
        expected(Time(99 * S / 100), Time(2 * S))
            .audio
            .iter()
            .all(|v| *v == 0.0),
        "hand i=3: block before clip"
    );
    let onset = expected(Time(S), Time(2 * S));
    assert_eq!(onset.audio[0], 0.0, "hand i=4: onset");
    let hand = (TAU * (440.0 / 48_000.0)).sin() * 0.5;
    assert!(
        (onset.audio[1] - hand).abs() <= 1e-15,
        "hand i=4: next sample"
    );
}
