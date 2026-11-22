use timeline::edit::{Edit, EditError, UndoStack};
use timeline::time::Time;
use timeline::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};

const SEEDS: [u64; 5] = [1, 7, 42, 1024, 0xdead_beef];

struct Random(u64);
impl Random {
    fn next(&mut self, bound: u64) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound) as usize
    }
}

fn track(id: u64) -> Track {
    Track {
        id,
        kind: "video".into(),
        name: "track".into(),
        clips: vec![],
        channels: vec![Channel {
            target: "x".into(),
            keys: vec![],
        }],
    }
}

fn initial() -> Timeline {
    Timeline {
        tracks: vec![track(0)],
    }
}

fn clip(start: i64, len: i64) -> Clip {
    Clip {
        start: Time(start),
        len: Time(len),
        source: "source".into(),
        source_offset: Time(0),
        name: "clip".into(),
    }
}

fn key_edit(rng: &mut Random, state: &Timeline) -> Edit {
    let count = state.tracks[0].channels[0].keys.len();
    let time = Time(rng.next(100) as i64);
    match rng.next(3) {
        0 if count > 0 => Edit::DeleteKey {
            track: 0,
            channel: 0,
            key: rng.next(count as u64),
        },
        1 if count > 0 => Edit::MoveKey {
            track: 0,
            channel: 0,
            key: rng.next(count as u64),
            time,
        },
        _ => Edit::InsertKey {
            track: 0,
            channel: 0,
            key: Keyframe {
                time,
                value: 1.0,
                interp: Interp::Linear,
                handles: None,
            },
        },
    }
}

#[test]
fn keyframe_edits_keep_order_property() {
    for seed in SEEDS {
        let mut rng = Random(seed);
        let mut state = initial();
        for _ in 0..2_000 {
            let edit = key_edit(&mut rng, &state);
            let before = state.clone();
            if edit.apply(&mut state).is_err() {
                assert_eq!(state, before);
            }
            assert!(state.tracks[0].channels[0]
                .keys
                .windows(2)
                .all(|w| w[0].time.0 < w[1].time.0));
        }
    }
}

#[test]
fn clip_split_join_round_trip() {
    for seed in SEEDS {
        let mut rng = Random(seed);
        for _ in 0..2_000 {
            let start = rng.next(1000) as i64;
            let len = rng.next(1000) as i64 + 2;
            let mut state = initial();
            let mut original = clip(start, len);
            original.source_offset = Time(rng.next(100) as i64);
            state.tracks[0].clips.push(original.clone());
            let before = state.clone();
            for at in [start, start + len] {
                assert_eq!(
                    Edit::SplitClip {
                        track: 0,
                        clip: 0,
                        at: Time(at)
                    }
                    .apply(&mut state),
                    Err(EditError::InvalidSplit)
                );
                assert_eq!(state, before);
            }
            let left_len = rng.next((len - 1) as u64) as i64 + 1;
            Edit::SplitClip {
                track: 0,
                clip: 0,
                at: Time(start + left_len),
            }
            .apply(&mut state)
            .unwrap();
            assert_eq!(
                state.tracks[0].clips[1].source_offset,
                Time(original.source_offset.0 + left_len)
            );
            Edit::JoinClips {
                track: 0,
                left: 0,
                right: 1,
            }
            .apply(&mut state)
            .unwrap();
            assert_eq!(state, before);
        }
    }
}

#[test]
fn clip_edits_reject_overlap_and_bad_ranges() {
    let mut state = initial();
    Edit::AddClip {
        track: 0,
        clip: clip(10, 10),
        source_len: Time(100),
    }
    .apply(&mut state)
    .unwrap();
    for (item, source_len, error) in [
        (clip(15, 5), 100, EditError::Overlap),
        (clip(30, 0), 100, EditError::InvalidRange),
        (clip(30, -1), 100, EditError::InvalidRange),
        (clip(-1, 1), 100, EditError::InvalidRange),
        (clip(30, 10), 9, EditError::SourceRange),
        (clip(i64::MAX, 1), 100, EditError::Overflow),
    ] {
        let before = state.clone();
        assert_eq!(
            Edit::AddClip {
                track: 0,
                clip: item,
                source_len: Time(source_len)
            }
            .apply(&mut state),
            Err(error)
        );
        assert_eq!(state, before);
    }
    let before = state.clone();
    assert_eq!(
        Edit::TrimClip {
            track: 0,
            clip: 0,
            start: Time(10),
            len: Time(10),
            source_offset: Time(i64::MAX),
            source_len: Time(i64::MAX)
        }
        .apply(&mut state),
        Err(EditError::Overflow)
    );
    assert_eq!(state, before);
}

fn random_run(seed: u64) -> (Timeline, Timeline, UndoStack) {
    let mut rng = Random(seed);
    let mut state = initial();
    let mut stack = UndoStack::default();
    let start = state.clone();
    for step in 0..10_000 {
        let count = state.tracks[0].clips.len();
        let edit = match rng.next(8) {
            0 => Edit::AddTrack {
                index: state.tracks.len(),
                track: track(step + 1),
            },
            1 if state.tracks.len() > 1 => Edit::RemoveTrack {
                track: state.tracks.len() - 1,
            },
            2 => Edit::AddClip {
                track: 0,
                clip: clip(rng.next(1000) as i64, 10),
                source_len: Time(100),
            },
            3 if count > 0 => Edit::MoveClip {
                track: 0,
                clip: rng.next(count as u64),
                delta: Time(rng.next(21) as i64 - 10),
            },
            4 if count > 0 => Edit::TrimClip {
                track: 0,
                clip: rng.next(count as u64),
                start: Time(rng.next(1000) as i64),
                len: Time(rng.next(20) as i64 + 1),
                source_offset: Time(rng.next(20) as i64),
                source_len: Time(100),
            },
            5 if count > 0 => Edit::RemoveClip {
                track: 0,
                clip: rng.next(count as u64),
            },
            _ => key_edit(&mut rng, &state),
        };
        let before = state.clone();
        let mut probe = before.clone();
        if let Ok(inverse) = edit.apply(&mut probe) {
            inverse.apply(&mut probe).unwrap();
            assert_eq!(
                probe, before,
                "inverse round trip, seed {seed}, step {step}"
            );
        }
        if stack.apply(&mut state, edit).is_err() {
            assert_eq!(state, before);
        }
    }
    (start, state, stack)
}

#[test]
fn random_edits_then_full_undo_10000() {
    for seed in SEEDS {
        let (start, mut state, mut stack) = random_run(seed);
        while stack.undo(&mut state).unwrap() {}
        assert_eq!(state, start, "seed {seed}");
    }
}

#[test]
fn redo_after_full_undo_returns_end_state() {
    for seed in SEEDS {
        let (start, mut state, mut stack) = random_run(seed);
        let end = state.clone();
        while stack.undo(&mut state).unwrap() {}
        assert_eq!(state, start);
        while stack.redo(&mut state).unwrap() {}
        assert_eq!(state, end, "seed {seed}");
        assert!(stack.undo(&mut state).unwrap());
        let index = state.tracks.len();
        stack
            .apply(
                &mut state,
                Edit::AddTrack {
                    index,
                    track: track(u64::MAX),
                },
            )
            .unwrap();
        assert!(!stack.redo(&mut state).unwrap());
    }
}
