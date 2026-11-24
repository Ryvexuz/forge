use std::collections::BTreeMap;
use std::ffi::CString;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use timeline::capi::*;

const S: i64 = 254_016_000_000;
static SERIAL: Mutex<()> = Mutex::new(());

fn c(text: &str) -> CString {
    CString::new(text).unwrap()
}

fn code(actual: i32, expected: i32) {
    // The declared result codes occupy this entire inclusive integer band.
    assert!(
        (TL_ERR_LIMIT..=TL_OK).contains(&actual),
        "undeclared code: {actual}"
    );
    assert_eq!(actual, expected);
}

fn create() -> u64 {
    let mut h = 0;
    // SAFETY: h is live writable u64 storage.
    unsafe { code(tl_create(&mut h), TL_OK) };
    assert_ne!(h, 0);
    h
}

fn saved(h: u64) -> Vec<u8> {
    let mut n = 0;
    // SAFETY: n is writable; the null buffer is used only for a size query.
    unsafe { code(tl_save(h, null_mut(), 0, &mut n), TL_ERR_BUFFER_TOO_SMALL) };
    assert!(n > 0);
    let mut bytes = vec![0; n];
    // SAFETY: the buffer has n writable bytes, disjoint from n.
    unsafe { code(tl_save(h, bytes.as_mut_ptr(), bytes.len(), &mut n), TL_OK) };
    assert_eq!(n, bytes.len());
    bytes
}

fn populated() -> u64 {
    let h = create();
    code(
        track(h, 1, "animation", "Motion", Some("object/position")),
        TL_OK,
    );
    code(track(h, 2, "audio", "Tone", None), TL_OK);
    code(track(h, 3, "video", "Picture", None), TL_OK);
    code(clip(h, 1, 0, 10 * S, S, "motion", "Move"), TL_OK);
    code(clip(h, 2, S, 6 * S, 0, "tone", "Beep"), TL_OK);
    code(clip(h, 3, 2 * S, 6 * S, S / 2, "frames", "Shot"), TL_OK);
    for (time, value, interp) in [
        (0, 1.0, TL_INTERP_STEP),
        (2 * S, 3.0, TL_INTERP_LINEAR),
        (8 * S, -1.0, TL_INTERP_LINEAR),
    ] {
        code(tl_add_key(h, 1, 0, time, value, interp as u32), TL_OK);
    }
    h
}

fn track(h: u64, id: u64, kind: &str, name: &str, target: Option<&str>) -> i32 {
    let (kind, name, target) = (c(kind), c(name), target.map(c));
    let target_ptr = target.as_ref().map_or(null(), |s| s.as_ptr());
    // SAFETY: CStrings stay live through the call; optional target may be null.
    unsafe { tl_add_track(h, id, kind.as_ptr(), name.as_ptr(), target_ptr) }
}

fn clip(h: u64, id: u64, start: i64, len: i64, offset: i64, source: &str, name: &str) -> i32 {
    let (source, name) = (c(source), c(name));
    // SAFETY: both CStrings stay live through the call.
    unsafe { tl_add_clip(h, id, start, len, offset, source.as_ptr(), name.as_ptr()) }
}

fn probe(h: u64) -> (i32, u64, i32, u64) {
    let (mut value, mut frame) = (123.0_f64, 456);
    // SAFETY: both outputs are live, aligned scalar storage.
    unsafe {
        let a = tl_read_value(h, 1, 0, &mut value);
        let b = tl_read_video_frame(h, 3, &mut frame);
        assert!([TL_OK, TL_ERR_NOT_IN_CLIP].contains(&a));
        assert!([TL_OK, TL_ERR_NOT_IN_CLIP].contains(&b));
        (a, value.to_bits(), b, frame)
    }
}

// Record all undo states, then restore them through redo. This observes history
// through the C API rather than treating unchanged serialization as proof of it.
fn history(h: u64) -> Vec<Vec<u8>> {
    let original = saved(h);
    let mut states = Vec::new();
    let mut done = -1;
    // SAFETY: done is live writable storage throughout the calls.
    unsafe {
        loop {
            code(tl_undo(h, &mut done), TL_OK);
            if done == 0 {
                break;
            }
            assert_eq!(done, 1);
            states.push(saved(h));
        }
        for _ in &states {
            code(tl_redo(h, &mut done), TL_OK);
            assert_eq!(done, 1);
        }
    }
    assert_eq!(saved(h), original);
    states
}

fn numeric_edges() {
    // Exact binary midpoints are independent references, including subnormals,
    // the largest finite endpoints and near-cancelling opposite signs.
    for (a, b, expected) in [
        (f64::from_bits(1), f64::from_bits(3), f64::from_bits(2)),
        (f64::MIN_POSITIVE, f64::MIN_POSITIVE, f64::MIN_POSITIVE),
        (f64::MAX, f64::MAX, f64::MAX),
        (-f64::MAX, f64::MAX, 0.0),
        (
            -f64::MAX,
            f64::from_bits(f64::MAX.to_bits() - 1),
            -2.0_f64.powi(f64::MAX_EXP - f64::MANTISSA_DIGITS as i32 - 1),
        ),
    ] {
        let h = create();
        let mut value = 123.0;
        code(track(h, 1, "animation", "A", Some("x")), TL_OK);
        code(clip(h, 1, i64::MAX - 2, 2, 0, "s", "c"), TL_OK);
        // SAFETY: value is live writable scalar storage.
        unsafe {
            code(tl_add_key(h, 1, 0, 0, a, TL_INTERP_LINEAR as u32), TL_OK);
            code(tl_add_key(h, 1, 0, 2, b, TL_INTERP_LINEAR as u32), TL_OK);
            code(tl_scrub(h, i64::MAX - 2), TL_OK);
            code(tl_read_value(h, 1, 0, &mut value), TL_OK);
            assert_eq!(value, a);
            code(tl_scrub(h, i64::MAX - 1), TL_OK);
            code(tl_read_value(h, 1, 0, &mut value), TL_OK);
        }
        assert_eq!(value, expected);
        code(tl_free(h), TL_OK);
    }
}

#[test]
fn capi_round_trip_through_extern_c() {
    let _serial = SERIAL.lock().unwrap();
    let h = populated();
    let (mut value, mut frame, mut done) = (123.0_f64, 456, -1);
    let mut samples = vec![123.0; 480];
    code(tl_scrub(h, 61 * S / 10), TL_OK);
    // SAFETY: outputs have their declared extents; CStrings live through each call.
    unsafe {
        code(tl_read_value(h, 1, 0, &mut value), TL_OK);
        assert!((value - (-0.4)).abs() <= 1e-12);
        code(
            tl_read_audio(h, 2, samples.as_mut_ptr(), samples.len()),
            TL_OK,
        );
        for (k, sample) in samples.iter().enumerate() {
            let cycles = ((292_800 + k - 48_000) as f64 * (440.0 / 48_000.0)).rem_euclid(1.0);
            let expected = (std::f64::consts::TAU * cycles).sin() * 0.5;
            assert!(
                (sample - expected).abs() <= 1e-12,
                "sample {k}: {sample} != {expected}"
            );
        }
        code(tl_read_video_frame(h, 3, &mut frame), TL_OK);
        assert_eq!(frame, 110);
        code(clip(h, 3, 9 * S, S, 0, "frames", "Later"), TL_OK);
        code(tl_scrub(h, 95 * S / 10), TL_OK);
        code(tl_read_video_frame(h, 3, &mut frame), TL_OK);
        assert_eq!(frame, 12);
        code(tl_undo(h, &mut done), TL_OK);
        assert_eq!(done, 1);
        frame = 456;
        code(tl_read_video_frame(h, 3, &mut frame), TL_ERR_NOT_IN_CLIP);
        assert_eq!(frame, 456);
        let before = saved(h);
        code(tl_add_key(h, 1, 0, 0, 1.0, 0), TL_ERR_EDIT_REFUSED);
        assert_eq!(saved(h), before); // refusal must preserve the pending redo.
        code(tl_redo(h, &mut done), TL_OK);
        assert_eq!(done, 1);
        code(tl_read_video_frame(h, 3, &mut frame), TL_OK);
        assert_eq!(frame, 12);
    }
    let bytes = saved(h);
    assert!(bytes.starts_with(b"timeline\t1\n"));
    // A nonempty destination history makes retaining the old stack observable.
    let h2 = populated();
    code(tl_scrub(h2, 61 * S / 10), TL_OK);
    // SAFETY: bytes is readable for its length; scalar outputs are writable.
    unsafe {
        code(tl_load(h2, bytes.as_ptr(), bytes.len()), TL_OK);
        assert_eq!(saved(h2), bytes);
        code(tl_read_value(h2, 1, 0, &mut value), TL_OK);
        assert_eq!(value, 1.0); // load reset the playhead to zero.
        code(tl_scrub(h2, 61 * S / 10), TL_OK);
        code(tl_read_value(h2, 1, 0, &mut value), TL_OK);
        assert!((value - (-0.4)).abs() <= 1e-12);
        code(tl_undo(h2, &mut done), TL_OK);
        assert_eq!(done, 0);
        code(tl_redo(h2, &mut done), TL_OK);
        assert_eq!(done, 0);
        // An exact half sample rounds down; one tick later rounds up.
        let half = S + S / 48_000 / 2;
        for (time, cycles) in [(half, 0.0), (half + 1, 440.0 / 48_000.0)] {
            code(tl_scrub(h2, time), TL_OK);
            code(tl_read_audio(h2, 2, &mut value, 1), TL_OK);
            let expected = (std::f64::consts::TAU * cycles).sin() * 0.5;
            assert!((value - expected).abs() <= 1e-12);
        }
    }
    code(tl_free(h), TL_OK);
    code(tl_free(h2), TL_OK);
    numeric_edges();
}

#[test]
fn capi_error_codes_not_panics() {
    let _serial = SERIAL.lock().unwrap();
    code(tl_free(0), TL_ERR_NULL_HANDLE);
    code(tl_scrub(0, 0), TL_ERR_NULL_HANDLE);
    // SAFETY: null is intentionally rejected before dereference.
    unsafe { code(tl_create(null_mut()), TL_ERR_NULL_POINTER) };
    let stale = create();
    code(tl_free(stale), TL_OK);
    code(tl_scrub(stale, 0), TL_ERR_STALE_HANDLE);
    code(tl_free(stale), TL_ERR_STALE_HANDLE);
    code(tl_scrub(u64::MAX, 0), TL_ERR_STALE_HANDLE);
    let h = populated();
    code(tl_scrub(h, 61 * S / 10), TL_OK);
    macro_rules! refused {
        ($call:expr, $expected:expr) => {{
            let before = saved(h);
            let undo = history(h);
            let position = probe(h);
            code($call, $expected);
            assert_eq!(saved(h), before, "refusal changed serialization");
            assert_eq!(probe(h), position, "refusal changed playhead or output");
            assert_eq!(history(h), undo, "refusal changed undo history");
        }};
    }
    let (mut value, mut frame, mut done, mut n) = (123.0_f64, 456, -1, 0);
    let (name, source, target) = (c("Name"), c("source"), c("target"));
    let (np, sp, tp) = (name.as_ptr(), source.as_ptr(), target.as_ptr());
    let invalid = CString::from_vec_with_nul(vec![0xff, 0]).unwrap();
    let mut block = vec![123.0; 1_048_577];
    // SAFETY: all non-null buffers have their stated extents and are disjoint;
    // strings live through each call; null arguments exercise rejection only.
    unsafe {
        let size = saved(h).len();
        let mut short = vec![0xAA; size - 1];
        refused!(
            tl_save(h, short.as_mut_ptr(), short.len(), &mut n),
            TL_ERR_BUFFER_TOO_SMALL
        );
        assert_eq!(n, size);
        assert!(short.iter().all(|&byte| byte == 0xAA));
        n = 777;
        refused!(tl_save(h, null_mut(), size, &mut n), TL_ERR_NULL_POINTER);
        assert_eq!(n, size);
        refused!(tl_save(h, null_mut(), 0, null_mut()), TL_ERR_NULL_POINTER);
        refused!(clip(h, 99, 0, S, 0, "source", "Name"), TL_ERR_BAD_TRACK);
        refused!(tl_read_video_frame(h, 2, &mut frame), TL_ERR_BAD_TRACK);
        refused!(tl_read_value(h, 3, 0, &mut value), TL_ERR_BAD_TRACK);
        refused!(tl_read_audio(h, 3, block.as_mut_ptr(), 1), TL_ERR_BAD_TRACK);
        assert_eq!(frame, 456);
        for (start, len, offset) in [
            (0, 0, 0),
            (0, -1, 0),
            (-1, S, 0),
            (i64::MAX, 1, 0),
            (0, 1, -1),
            (0, 1, i64::MAX),
        ] {
            refused!(
                clip(h, 1, start, len, offset, "source", "Name"),
                TL_ERR_BAD_TIME
            );
        }
        refused!(tl_scrub(h, -1), TL_ERR_BAD_TIME);
        refused!(tl_add_key(h, 1, 0, -1, 1.0, 0), TL_ERR_BAD_TIME);
        refused!(track(h, 4, "midi", "Name", None), TL_ERR_BAD_VALUE);
        refused!(track(h, 4, "animation", "Name", None), TL_ERR_BAD_VALUE);
        refused!(
            track(h, 4, "audio", "Name", Some("target")),
            TL_ERR_BAD_VALUE
        );
        refused!(tl_add_key(h, 1, 0, S, 1.0, 7), TL_ERR_BAD_VALUE);
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            refused!(tl_add_key(h, 1, 0, S, bad, 0), TL_ERR_BAD_VALUE);
        }
        refused!(tl_add_key(h, 1, 1, S, 1.0, 0), TL_ERR_BAD_VALUE);
        refused!(tl_read_value(h, 1, 1, &mut value), TL_ERR_BAD_VALUE);
        refused!(
            tl_read_audio(h, 2, block.as_mut_ptr(), block.len()),
            TL_ERR_BAD_VALUE
        );
        assert!(block.iter().all(|&sample| sample == 123.0));
        for bad in [c("a\nb"), c(""), invalid, c("a\rb"), c("a\u{0085}b")] {
            refused!(
                tl_add_track(h, 4, c("audio").as_ptr(), bad.as_ptr(), null()),
                TL_ERR_BAD_TEXT
            );
        }
        refused!(clip(h, 1, 11 * S, S, 0, "a:b", "Name"), TL_ERR_BAD_TEXT);
        refused!(
            track(h, 4, "animation", "Name", Some("a\\b")),
            TL_ERR_BAD_TEXT
        );
        refused!(track(h, 1, "audio", "Name", None), TL_ERR_EDIT_REFUSED);
        refused!(clip(h, 1, S, S, 0, "source", "Name"), TL_ERR_EDIT_REFUSED);
        refused!(tl_add_key(h, 1, 0, 0, 1.0, 0), TL_ERR_EDIT_REFUSED);
        // Every pointer-bearing handle entry also checks the handle before pointers.
        for bad in [0, stale, u64::MAX, 1_u64 << 32] {
            let expected = if bad == 0 {
                TL_ERR_NULL_HANDLE
            } else {
                TL_ERR_STALE_HANDLE
            };
            code(tl_add_track(bad, 1, null(), null(), null()), expected);
            code(tl_add_clip(bad, 1, 0, 1, 0, null(), null()), expected);
            code(tl_read_value(bad, 1, 0, &mut value), expected);
            code(tl_read_audio(bad, 2, block.as_mut_ptr(), 1), expected);
            code(tl_read_video_frame(bad, 3, &mut frame), expected);
            code(tl_undo(bad, &mut done), expected);
            code(tl_redo(bad, &mut done), expected);
            n = 777;
            code(tl_save(bad, null_mut(), 0, &mut n), expected);
            assert_eq!(n, 777);
            code(tl_load(bad, null(), 0), expected);
        }
        assert_eq!((value, frame, done), (123.0, 456, -1));
        refused!(tl_add_track(h, 4, null(), np, tp), TL_ERR_NULL_POINTER);
        refused!(
            tl_add_track(h, 4, c("audio").as_ptr(), null(), null()),
            TL_ERR_NULL_POINTER
        );
        refused!(tl_add_clip(h, 1, 0, 1, 0, null(), np), TL_ERR_NULL_POINTER);
        refused!(tl_add_clip(h, 1, 0, 1, 0, sp, null()), TL_ERR_NULL_POINTER);
        refused!(tl_read_value(h, 1, 0, null_mut()), TL_ERR_NULL_POINTER);
        refused!(tl_read_audio(h, 2, null_mut(), 0), TL_ERR_NULL_POINTER);
        refused!(tl_read_video_frame(h, 3, null_mut()), TL_ERR_NULL_POINTER);
        refused!(tl_undo(h, null_mut()), TL_ERR_NULL_POINTER);
        refused!(tl_redo(h, null_mut()), TL_ERR_NULL_POINTER);
        refused!(tl_load(h, null(), 0), TL_ERR_NULL_POINTER);
        let overlap =
            b"timeline\t1\ntrack\t1\tvideo\tV\nclip\t1\t0\t10\ts\t0\tA\nclip\t1\t5\t10\ts\t0\tB\n";
        let unordered = b"timeline\t1\ntrack\t1\tanimation\tA\nchannel\t1\tx\nkey\t1\t0\t2\t1\tstep\t-\t-\t-\t-\nkey\t1\t0\t1\t2\tstep\t-\t-\t-\t-\n";
        let prefix: String = (0..4096)
            .map(|id| format!("track\t{id}\tvideo\tV\n"))
            .collect();
        let huge_bad = format!("timeline\t1\n{prefix}nope\n");
        for bytes in [
            b"nope".as_slice(),
            overlap.as_slice(),
            unordered.as_slice(),
            &[0xff],
            b"",
            huge_bad.as_bytes(),
        ] {
            refused!(tl_load(h, bytes.as_ptr(), bytes.len()), TL_ERR_FORMAT);
        }
        code(tl_read_audio(h, 2, block.as_mut_ptr(), 0), TL_OK);
        assert!(block.iter().all(|&sample| sample == 123.0));
        code(tl_scrub(h, 11 * S), TL_OK);
        refused!(tl_read_value(h, 1, 0, &mut value), TL_ERR_NOT_IN_CLIP);
        refused!(tl_read_video_frame(h, 3, &mut frame), TL_ERR_NOT_IN_CLIP);
        assert_eq!((value, frame), (123.0, 456));
        code(tl_read_audio(h, 2, block.as_mut_ptr(), 480), TL_OK);
        assert!(block[..480].iter().all(|&sample| sample == 0.0));
        code(tl_read_audio(h, 2, block.as_mut_ptr(), 1_048_576), TL_OK);
        assert!(block[..1_048_576].iter().all(|&sample| sample == 0.0));
        assert_eq!(block[1_048_576], 123.0);
        code(tl_scrub(h, i64::MAX), TL_OK);
        refused!(tl_read_audio(h, 2, block.as_mut_ptr(), 480), TL_ERR_EVAL);
        assert!(block[..480].iter().all(|&sample| sample == 0.0));
        code(track(h, 4, "animation", "Empty curve", Some("x")), TL_OK);
        code(clip(h, 4, 0, S, 0, "motion", "Empty"), TL_OK);
        code(tl_scrub(h, 0), TL_OK);
        refused!(tl_read_value(h, 4, 0, &mut value), TL_ERR_EVAL);
        assert_eq!(value, 123.0);
    }
    code(tl_free(h), TL_OK);
    code(tl_selftest_panic(), TL_ERR_PANIC);
    let after_panic = create();
    code(tl_free(after_panic), TL_OK);
}

#[test]
fn capi_freed_handle_never_aliases_a_new_one() {
    let _serial = SERIAL.lock().unwrap();
    let a = create();
    code(tl_free(a), TL_OK);
    let b = create();
    assert_ne!(a, b);
    code(tl_scrub(a, 0), TL_ERR_STALE_HANDLE);
    code(tl_scrub(b, 0), TL_OK);
    code(tl_free(b), TL_OK);
    let mut seen = vec![a, b];
    for _ in 0..50 {
        let h = create();
        assert!(!seen.contains(&h), "reissued handle: {h}");
        seen.push(h);
        code(tl_free(h), TL_OK);
        code(tl_scrub(h, 0), TL_ERR_STALE_HANDLE);
        code(tl_free(h), TL_ERR_STALE_HANDLE);
    }
    let handles: Vec<_> = (0..4096).map(|_| create()).collect();
    let mut untouched = 777;
    // SAFETY: untouched is writable scalar storage, which must not change on error.
    unsafe { code(tl_create(&mut untouched), TL_ERR_LIMIT) };
    assert_eq!(untouched, 777);
    for h in handles {
        code(tl_free(h), TL_OK);
    }
}

type Signature = (String, Vec<String>);

fn c_type(rust: &str) -> &str {
    match rust {
        "u64" => "uint64_t",
        "i64" => "int64_t",
        "u32" => "uint32_t",
        "i32" => "int32_t",
        "f64" => "double",
        "usize" => "size_t",
        "*mut u64" => "uint64_t *",
        "*mut i32" => "int32_t *",
        "*mut f64" => "double *",
        "*mut usize" => "size_t *",
        "*mut u8" => "uint8_t *",
        "*const u8" => "const uint8_t *",
        "*const c_char" => "const char *",
        other => panic!("unmapped Rust type: {other}"),
    }
}

fn rust_signatures(source: &str) -> Vec<Signature> {
    let lines: Vec<_> = source.lines().collect();
    let mut found = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some((_, tail)) = line.split_once("extern \"C\" fn ") else {
            continue;
        };
        if i == 0 || lines[i - 1].trim() != "#[no_mangle]" {
            continue;
        }
        let signature = std::iter::once(tail)
            .chain(lines[i + 1..].iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let (name, rest) = signature.split_once('(').unwrap();
        let mut depth = 1;
        let close = rest
            .char_indices()
            .find_map(|(i, ch)| {
                match ch {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(i)
            })
            .expect("matching parameter delimiter");
        assert!(rest[close + 1..].trim_start().starts_with("-> i32 {"));
        let params = rest[..close]
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                let (_, ty) = p.split_once(':').expect("named Rust parameter");
                c_type(&ty.split_whitespace().collect::<Vec<_>>().join(" ")).to_owned()
            })
            .collect();
        found.push((name.trim().to_owned(), params));
    }
    found
}

fn header_signatures(header: &str) -> Vec<Signature> {
    header
        .lines()
        .filter_map(|line| {
            let tail = line.strip_prefix("int32_t ")?;
            let (name, rest) = tail.split_once('(').unwrap();
            let params = rest.strip_suffix(");").expect("single-line prototype");
            let params = if params == "void" {
                Vec::new()
            } else {
                params
                    .split(',')
                    .map(|p| {
                        let p = p.trim();
                        let end = p.rfind(|ch: char| ch == '*' || ch.is_whitespace()).unwrap();
                        p[..=end].split_whitespace().collect::<Vec<_>>().join(" ")
                    })
                    .collect()
            };
            Some((name.to_owned(), params))
        })
        .collect()
}

fn constants(text: &str, rust: bool) -> BTreeMap<String, i32> {
    let mut found = BTreeMap::new();
    for line in text.lines() {
        let prefix = if rust { "pub const TL_" } else { "#define TL_" };
        if let Some(tail) = line.strip_prefix(prefix) {
            let (name, value) = if rust {
                let (name, value) = tail.split_once(": i32 = ").unwrap();
                (name, value.strip_suffix(';').unwrap())
            } else {
                tail.split_once(' ').unwrap()
            };
            assert!(
                found
                    .insert(name.to_owned(), value.parse().unwrap())
                    .is_none(),
                "duplicate {name}"
            );
        }
    }
    found
}

#[test]
fn capi_symbols_match_header() {
    let _serial = SERIAL.lock().unwrap();
    let rust = include_str!("../src/capi.rs");
    let header = include_str!("../include/timeline.h");
    let functions = rust_signatures(rust);
    assert_eq!(functions.len(), 14);
    assert_eq!(functions, header_signatures(header));
    let codes = constants(rust, true);
    assert_eq!(codes.len(), 18);
    assert_eq!(codes, constants(header, false));
    println!(
        "C/header functions: {}; constants: {}",
        functions.len(),
        codes.len()
    );
    assert!(!rust.contains("#[repr(C)]"));
    assert!(!header.contains("struct"));
    // Behavioural negative controls: a drifted type, extra symbol, missing code,
    // and reordered prototypes must all change the comparison.
    let wrong_type = header.replace("double *out_value", "uint64_t *out_value");
    assert_ne!(functions, header_signatures(&wrong_type));
    let extra = "\n#[no_mangle]\npub extern \"C\" fn tl_debug_count() -> i32 { 0 }\n";
    assert_ne!(functions, rust_signatures(&format!("{rust}{extra}")));
    let missing_code = header.replace("#define TL_ERR_EVAL -12\n", "");
    assert_ne!(codes, constants(&missing_code, false));
    let mut reversed = header_signatures(header);
    reversed.reverse();
    assert_ne!(functions, reversed);
}
