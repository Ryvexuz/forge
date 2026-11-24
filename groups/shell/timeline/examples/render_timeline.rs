//! Draws a three-track timeline with a waveform added by this example, not the
//! draw list. The picture is for t24's human check, not a golden image.
//! `--out` is relative to the process working directory, not the source file.

use timeline::audio::{tone, AudioDriver};
use timeline::driver::TrackDriver;
use timeline::time::{Time, TICKS_PER_SECOND as S};
use timeline::track::{Channel, Clip, Interp, Keyframe, Timeline, Track};
use timeline::view::TimelineView;

// Fixed t24 canvas dimensions keep the human check repeatable.
const SIZE: (i32, i32) = (1280, 360);
// Two screen pixels per font pixel make the small bitmap legible.
const SCALE: i64 = 2;
// A twelve-pixel cell leaves two pixels between five-column glyphs.
const ADVANCE: i64 = 12;
// Half-second fades at 48 kHz expose the audio driver's envelope.
const FADE: u64 = 24_000;
// The plan's six draw-list colours, in symbolic-colour order.
const PALETTE: [[u8; 3]; 6] = [
    [24, 24, 28],
    [60, 60, 70],
    [70, 110, 170],
    [235, 235, 235],
    [250, 200, 60],
    [230, 50, 50],
];
// Green distinguishes the example's waveform from draw-list primitives.
const WAVE: [u8; 3] = [120, 220, 140];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Colour {
    Background,
    Ruler,
    Clip,
    Label,
    Key,
    Playhead,
}

impl Colour {
    fn rgb(self) -> [u8; 3] {
        PALETTE[self as usize]
    }
}

#[derive(Debug, PartialEq)]
enum Record {
    Rect(i32, i32, i32, i32, Colour),
    Line(i32, i32, i32, i32, Colour),
    Text(i32, i32, Colour, String),
}

#[derive(Debug, PartialEq)]
enum DrawError {
    Kind(usize),
    Fields(usize),
    Number(usize),
    Colour(usize),
    Diagonal(usize),
    Escape(usize),
}

fn parse_args(args: &[String]) -> Result<String, String> {
    if args.len() == 2
        && args[0] == "--out"
        && !args[1].is_empty()
        && !args[1].starts_with("--")
        && !args[1].contains(['\0', '\r', '\n'])
    {
        return Ok(args[1].clone());
    }
    Err(if args.first().is_some_and(|s| s == "--out") {
        args.get(2)
            .or_else(|| args.get(1))
            .cloned()
            .unwrap_or_else(|| "--out".into())
    } else {
        args.first().cloned().unwrap_or_else(|| "--out".into())
    })
}

fn parse_draw_list(text: &str) -> Result<Vec<Record>, DrawError> {
    let mut records = Vec::new();
    for (index, raw) in text.split_terminator('\n').enumerate() {
        let n = index + 1;
        if raw.contains('\r') {
            return Err(DrawError::Fields(n));
        }
        let mut rest = raw;
        let mut field = || {
            rest = rest.trim_start_matches([' ', '\t']);
            let end = rest.find([' ', '\t']).unwrap_or(rest.len());
            let token = &rest[..end];
            rest = &rest[end..];
            token
        };
        let kind = field();
        let count = match kind {
            "rect" | "line" => 5,
            "text" => 3,
            _ => return Err(DrawError::Kind(n)),
        };
        let fields: Vec<_> = (0..count).map(|_| field()).collect();
        if fields.iter().any(|s| s.is_empty()) {
            return Err(DrawError::Fields(n));
        }
        let tail = rest.trim_start_matches([' ', '\t']);
        if kind != "text" && !tail.is_empty() {
            return Err(DrawError::Fields(n));
        }
        let number = |i: usize| fields[i].parse::<i32>().map_err(|_| DrawError::Number(n));
        let x = number(0)?;
        let y = number(1)?;
        let colour = match fields[count - 1] {
            "background" => Colour::Background,
            "ruler" => Colour::Ruler,
            "clip" => Colour::Clip,
            "label" => Colour::Label,
            "key" => Colour::Key,
            "playhead" => Colour::Playhead,
            _ => return Err(DrawError::Colour(n)),
        };
        let record = if kind == "text" {
            let quoted = tail
                .strip_prefix('"')
                .and_then(|s| s.strip_suffix('"'))
                .ok_or(DrawError::Fields(n))?;
            let mut chars = quoted.chars();
            let mut value = String::new();
            while let Some(c) = chars.next() {
                value.push(match c {
                    '\\' => match chars.next() {
                        Some(c @ ('\\' | '"')) => c,
                        _ => return Err(DrawError::Escape(n)),
                    },
                    '"' => return Err(DrawError::Fields(n)),
                    c if c.is_control() => return Err(DrawError::Escape(n)),
                    c => c,
                });
            }
            Record::Text(x, y, colour, value)
        } else {
            let a = number(2)?;
            let b = number(3)?;
            if kind == "rect" {
                if a < 0 || b < 0 {
                    return Err(DrawError::Number(n));
                }
                Record::Rect(x, y, a, b, colour)
            } else {
                if x != a && y != b {
                    return Err(DrawError::Diagonal(n));
                }
                Record::Line(x, y, a, b, colour)
            }
        };
        records.push(record);
    }
    Ok(records)
}

struct Image {
    width: i32,
    height: i32,
    rgb: Vec<u8>,
}

impl Image {
    fn new(width: i32, height: i32) -> Self {
        let width = width.max(0);
        let height = height.max(0);
        let mut rgb = Vec::new();
        for _ in 0..i64::from(width) * i64::from(height) {
            rgb.extend_from_slice(&Colour::Background.rgb());
        }
        Self { width, height, rgb }
    }

    fn pixel(&self, x: i32, y: i32) -> Option<[u8; 3]> {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 3;
        Some([self.rgb[i], self.rgb[i + 1], self.rgb[i + 2]])
    }

    fn fill_rect(&mut self, x: i64, y: i64, w: i64, h: i64, colour: [u8; 3]) {
        for py in y.max(0)..y.saturating_add(h.max(0)).min(i64::from(self.height)) {
            for px in x.max(0)..x.saturating_add(w.max(0)).min(i64::from(self.width)) {
                let i = (py as usize * self.width as usize + px as usize) * 3;
                self.rgb[i..i + 3].copy_from_slice(&colour);
            }
        }
    }
}

fn paint(image: &mut Image, record: &Record) -> u32 {
    match record {
        Record::Rect(x, y, w, h, c) => image.fill_rect(
            i64::from(*x),
            i64::from(*y),
            i64::from(*w),
            i64::from(*h),
            c.rgb(),
        ),
        Record::Line(x, y, a, b, c) => image.fill_rect(
            i64::from(*x.min(a)),
            i64::from(*y.min(b)),
            (i64::from(*x) - i64::from(*a)).abs() + 1,
            (i64::from(*y) - i64::from(*b)).abs() + 1,
            c.rgb(),
        ),
        Record::Text(x, y, c, text) => {
            let mut tofu = 0_u32;
            let mut origin = i64::from(*x) + 4;
            for ch in text.chars() {
                let rows = glyph(ch).unwrap_or_else(|| {
                    tofu = tofu.saturating_add(1);
                    [31; 7]
                });
                for (row, bits) in rows.iter().enumerate() {
                    for col in 0..5 {
                        if bits & (1 << (4 - col)) != 0 {
                            image.fill_rect(
                                origin + col * SCALE,
                                i64::from(*y) + 4 + row as i64 * SCALE,
                                SCALE,
                                SCALE,
                                c.rgb(),
                            );
                        }
                    }
                }
                origin = origin.saturating_add(ADVANCE);
            }
            return tofu;
        }
    }
    0
}

// Each row uses bit 4 at the left. Lowercase forms are distinct from capitals.
const FONT: &[(char, [u8; 7])] = &[
    (' ', [0, 0, 0, 0, 0, 0, 0]),
    ('A', [14, 17, 17, 31, 17, 17, 17]),
    ('B', [30, 17, 17, 30, 17, 17, 30]),
    ('C', [14, 17, 16, 16, 16, 17, 14]),
    ('D', [30, 17, 17, 17, 17, 17, 30]),
    ('E', [31, 16, 16, 30, 16, 16, 31]),
    ('F', [31, 16, 16, 30, 16, 16, 16]),
    ('G', [14, 17, 16, 23, 17, 17, 15]),
    ('H', [17, 17, 17, 31, 17, 17, 17]),
    ('I', [14, 4, 4, 4, 4, 4, 14]),
    ('J', [7, 2, 2, 2, 18, 18, 12]),
    ('K', [17, 18, 20, 24, 20, 18, 17]),
    ('L', [16, 16, 16, 16, 16, 16, 31]),
    ('M', [17, 27, 21, 21, 17, 17, 17]),
    ('N', [17, 25, 21, 19, 17, 17, 17]),
    ('O', [14, 17, 17, 17, 17, 17, 14]),
    ('P', [30, 17, 17, 30, 16, 16, 16]),
    ('Q', [14, 17, 17, 17, 21, 18, 13]),
    ('R', [30, 17, 17, 30, 20, 18, 17]),
    ('S', [15, 16, 16, 14, 1, 1, 30]),
    ('T', [31, 4, 4, 4, 4, 4, 4]),
    ('U', [17, 17, 17, 17, 17, 17, 14]),
    ('V', [17, 17, 17, 17, 17, 10, 4]),
    ('W', [17, 17, 17, 21, 21, 21, 10]),
    ('X', [17, 17, 10, 4, 10, 17, 17]),
    ('Y', [17, 17, 10, 4, 4, 4, 4]),
    ('Z', [31, 1, 2, 4, 8, 16, 31]),
    ('a', [0, 0, 14, 1, 15, 17, 15]),
    ('b', [16, 16, 30, 17, 17, 17, 30]),
    ('c', [0, 0, 14, 16, 16, 17, 14]),
    ('d', [1, 1, 15, 17, 17, 17, 15]),
    ('e', [0, 0, 14, 17, 31, 16, 14]),
    ('f', [6, 9, 8, 28, 8, 8, 8]),
    ('g', [0, 14, 17, 17, 15, 1, 14]),
    ('h', [16, 16, 30, 17, 17, 17, 17]),
    ('i', [4, 0, 12, 4, 4, 4, 14]),
    ('j', [2, 0, 6, 2, 2, 18, 12]),
    ('k', [16, 16, 18, 20, 24, 20, 18]),
    ('l', [12, 4, 4, 4, 4, 4, 14]),
    ('m', [0, 0, 26, 21, 21, 21, 21]),
    ('n', [0, 0, 30, 17, 17, 17, 17]),
    ('o', [0, 0, 14, 17, 17, 17, 14]),
    ('p', [0, 30, 17, 17, 30, 16, 16]),
    ('q', [0, 15, 17, 17, 15, 1, 1]),
    ('r', [0, 0, 22, 25, 16, 16, 16]),
    ('s', [0, 0, 15, 16, 14, 1, 30]),
    ('t', [8, 8, 28, 8, 8, 9, 6]),
    ('u', [0, 0, 17, 17, 17, 19, 13]),
    ('v', [0, 0, 17, 17, 17, 10, 4]),
    ('w', [0, 0, 17, 17, 21, 21, 10]),
    ('x', [0, 0, 17, 10, 4, 10, 17]),
    ('y', [0, 17, 17, 17, 15, 1, 14]),
    ('z', [0, 0, 31, 2, 4, 8, 31]),
    ('0', [14, 17, 19, 21, 25, 17, 14]),
    ('1', [4, 12, 4, 4, 4, 4, 14]),
    ('2', [14, 17, 1, 2, 4, 8, 31]),
    ('3', [30, 1, 1, 14, 1, 1, 30]),
    ('4', [2, 6, 10, 18, 31, 2, 2]),
    ('5', [31, 16, 16, 30, 1, 1, 30]),
    ('6', [14, 16, 16, 30, 17, 17, 14]),
    ('7', [31, 1, 2, 4, 8, 8, 8]),
    ('8', [14, 17, 17, 14, 17, 17, 14]),
    ('9', [14, 17, 17, 15, 1, 1, 14]),
    ('-', [0, 0, 0, 31, 0, 0, 0]),
    ('_', [0, 0, 0, 0, 0, 0, 31]),
    ('.', [0, 0, 0, 0, 0, 12, 12]),
    ('/', [1, 2, 2, 4, 8, 8, 16]),
];

fn glyph(c: char) -> Option<[u8; 7]> {
    FONT.iter().find(|(ch, _)| *ch == c).map(|(_, rows)| *rows)
}

fn build_view() -> Result<TimelineView, String> {
    let mut tracks = Vec::new();
    for (i, (kind, name, start, len, source, label)) in [
        ("animation", "Motion", 0, 10, "motion", "Move"),
        ("audio", "Tone", 1, 6, "tone", "Beep"),
        ("video", "Picture", 2, 6, "frames", "Shot"),
    ]
    .into_iter()
    .enumerate()
    {
        tracks.push(Track {
            id: i as u64 + 1,
            kind: kind.into(),
            name: name.into(),
            clips: vec![Clip {
                start: Time(start * S),
                len: Time(len * S),
                source: source.into(),
                source_offset: Time(0),
                name: label.into(),
            }],
            channels: Vec::new(),
        });
    }
    tracks[0].channels.push(Channel {
        target: "object/position".into(),
        keys: [(0, 1.0), (2, 3.0), (5, 0.5), (8, -1.0)]
            .into_iter()
            .map(|(t, value)| Keyframe {
                time: Time(t * S),
                value,
                interp: if t == 0 { Interp::Step } else { Interp::Linear },
                handles: None,
            })
            .collect(),
    });
    let mut view = TimelineView::new(Timeline { tracks }, 100, Time(0), SIZE)
        .map_err(|e| format!("ViewError {e:?}"))?;
    view.scrub_to(Time(45 * S / 10));
    Ok(view)
}

struct Stats {
    records: usize,
    tofu: u32,
    wave_columns: usize,
    playhead_x: i32,
}

fn waveform(image: &mut Image, view: &TimelineView) -> Result<usize, String> {
    let driver_error = |e| format!("DriverError {e:?}");
    let mut audio = AudioDriver::new(48_000).map_err(driver_error)?;
    audio
        .add_clip(
            Time(S)..Time(7 * S),
            tone(440.0, 0.5).map_err(driver_error)?,
            FADE,
            FADE,
        )
        .map_err(driver_error)?;
    let rect = view
        .layout(1, 0)
        .map_err(|e| format!("ViewError {e:?}"))?
        .ok_or("ViewError missing audio clip")?;
    let end = (i64::from(rect.x) + i64::from(rect.width)).min(i64::from(image.width));
    let mut columns = 0;
    for x in i64::from(rect.x).max(0)..end {
        let x = x as i32;
        let start = view
            .pixel_to_time(x)
            .map_err(|e| format!("ViewError {e:?}"))?;
        let end = view
            .pixel_to_time(x + 1)
            .map_err(|e| format!("ViewError {e:?}"))?;
        let mut samples = [0.0; 48_000 / 100 + 2];
        audio
            .evaluate(start..end, &mut samples)
            .map_err(driver_error)?;
        // One validated tone bounds each sample to [-0.5, 0.5].
        if samples.iter().any(|s| !s.is_finite() || s.abs() > 0.5) {
            return Err("DriverError invalid waveform sample".into());
        }
        let peak = samples.iter().fold(0.0_f64, |p, s| p.max(s.abs()));
        let h = (peak * 20.0).round() as i64;
        let mid = i64::from(rect.y) + 24;
        let top = (mid - h).max(i64::from(rect.y));
        let bottom = (mid + h + 1).min(i64::from(rect.y) + i64::from(rect.height));
        image.fill_rect(i64::from(x), top, 1, bottom - top, WAVE);
        columns += 1;
    }
    Ok(columns)
}

fn render(view: &TimelineView) -> Result<(Image, Stats), String> {
    let records = parse_draw_list(&view.draw_list().map_err(|e| format!("ViewError {e:?}"))?)
        .map_err(|e| format!("DrawError {e:?}"))?;
    let mut image = Image::new(SIZE.0, SIZE.1);
    let mut stats = Stats {
        records: records.len(),
        tofu: 0,
        wave_columns: 0,
        playhead_x: view
            .time_to_pixel(view.playhead)
            .map_err(|e| format!("ViewError {e:?}"))?,
    };
    let mut waved = false;
    for record in &records {
        if !waved && matches!(record, Record::Line(_, _, _, _, Colour::Playhead)) {
            stats.wave_columns = waveform(&mut image, view)?;
            waved = true;
        }
        stats.tofu = stats.tofu.saturating_add(paint(&mut image, record));
    }
    if !waved {
        stats.wave_columns = waveform(&mut image, view)?;
    }
    Ok((image, stats))
}

fn to_ppm(image: &Image) -> Vec<u8> {
    debug_assert_eq!(
        image.pixel(image.width, 0),
        None,
        "the right image edge is exclusive"
    );
    let mut bytes = format!("P6\n{} {}\n255\n", image.width, image.height).into_bytes();
    bytes.extend_from_slice(&image.rgb);
    bytes
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let out = match parse_args(&args) {
        Ok(out) => out,
        Err(arg) => {
            eprintln!("render_timeline: refused {arg}");
            std::process::exit(2);
        }
    };
    let (image, stats) = match build_view().and_then(|view| render(&view)) {
        Ok(result) => result,
        Err(error) => {
            eprintln!("render_timeline: FAIL {error}");
            std::process::exit(1);
        }
    };
    let length = match std::fs::write(&out, to_ppm(&image)).and_then(|()| std::fs::metadata(&out)) {
        Ok(metadata) => metadata.len(),
        Err(error) => {
            eprintln!("render_timeline: FAIL write {error}");
            std::process::exit(1);
        }
    };
    println!(
        "render_timeline: records={} tofu={} wave_columns={} playhead_x={}",
        stats.records, stats.tofu, stats.wave_columns, stats.playhead_x
    );
    println!("render_timeline: wrote bytes={length}");
    println!(
        "render_timeline: ok width={} height={}",
        image.width, image.height
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_draw_list_reads_the_golden_fixture() {
        let fixture = include_str!("../tests/fixtures/widget_scrub.drawlist");
        let sections: Vec<_> = fixture.split("state ").filter(|s| !s.is_empty()).collect();
        assert_eq!(sections.len(), 3);
        for (section, name) in sections.iter().zip(["start", "middle", "drag"]) {
            let (header, body) = section.split_once('\n').unwrap();
            assert_eq!(header, name);
            let records = parse_draw_list(body).unwrap();
            assert_eq!(records.len(), 14);
            if name == "start" {
                let kinds: Vec<_> = records
                    .iter()
                    .map(|r| match r {
                        Record::Rect(..) => "rect",
                        Record::Text(..) => "text",
                        Record::Line(_, _, _, _, Colour::Key) => "key",
                        Record::Line(_, _, _, _, Colour::Playhead) => "playhead",
                        _ => "unexpected",
                    })
                    .collect();
                assert_eq!(
                    kinds,
                    [
                        "rect", "rect", "text", "rect", "text", "text", "rect", "text", "text",
                        "key", "key", "key", "key", "playhead"
                    ]
                );
                assert_eq!(
                    records[2],
                    Record::Text(0, 24, Colour::Label, "Music".into())
                );
            }
        }
    }

    #[test]
    fn parse_draw_list_refuses_bad_records() {
        for (input, error) in [
            ("circle 0 0 5 key", DrawError::Kind(1)),
            ("rect 0 0 5 key", DrawError::Fields(1)),
            ("rect 0 0 x 5 key", DrawError::Number(1)),
            ("rect 0 0 5 5 purple", DrawError::Colour(1)),
            ("line 0 0 5 5 key", DrawError::Diagonal(1)),
            (r#"text 0 0 label "a\nb""#, DrawError::Escape(1)),
            ("rect 0 0 -1 5 clip", DrawError::Number(1)),
            ("state start", DrawError::Kind(1)),
            ("rect 0 0 1 1 clip\nbogus", DrawError::Kind(2)),
            ("rect 0 0 1 1 clip extra", DrawError::Fields(1)),
            ("rect 2147483648 0 1 1 clip", DrawError::Number(1)),
            ("rect 0 0 1 1 clip\r", DrawError::Fields(1)),
            (r#"text 0 0 label "a" "b""#, DrawError::Fields(1)),
        ] {
            assert_eq!(parse_draw_list(input), Err(error), "{input}");
        }
        assert_eq!(
            parse_draw_list(r#"text 0 0 label "say \"hi\" \\ ok""#).unwrap(),
            vec![Record::Text(0, 0, Colour::Label, "say \"hi\" \\ ok".into())]
        );
    }

    #[test]
    fn raster_clips_to_the_image() {
        let mut image = Image::new(10, 10);
        for record in parse_draw_list(
            "rect -5 -5 8 8 clip\nrect 8 8 100 100 key\n\
            rect 20 20 5 5 key\nrect 0 0 0 5 key\nline 4 -3 4 20 playhead",
        )
        .unwrap()
        {
            paint(&mut image, &record);
        }
        for y in 0..10 {
            for x in 0..10 {
                let expected = if x == 4 {
                    Colour::Playhead
                } else if x < 3 && y < 3 {
                    Colour::Clip
                } else if x >= 8 && y >= 8 {
                    Colour::Key
                } else {
                    Colour::Background
                };
                assert_eq!(image.pixel(x, y), Some(expected.rgb()));
            }
        }
        assert_eq!(image.pixel(10, 0), None);
        assert_eq!(image.pixel(-1, 0), None);
        paint(
            &mut image,
            &Record::Line(i32::MIN, 9, i32::MAX, 9, Colour::Key),
        );
        paint(
            &mut image,
            &Record::Rect(i32::MAX, i32::MAX, i32::MAX, i32::MAX, Colour::Clip),
        );
        assert_eq!(image.pixel(9, 9), Some(Colour::Key.rgb()));
    }

    #[test]
    fn font_is_five_by_seven() {
        for (i, (c, rows)) in FONT.iter().enumerate() {
            assert!(rows.iter().all(|r| *r < 32), "{c}");
            assert!(*c == ' ' || rows.iter().any(|r| *r != 0), "{c}");
            for (other, bitmap) in &FONT[..i] {
                assert_ne!(rows, bitmap, "{c} and {other}");
            }
        }
        for c in " ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_./".chars() {
            assert!(glyph(c).is_some(), "{c}");
        }
        for c in ['?', 'é', '\t'] {
            assert_eq!(glyph(c), None);
        }
        let mut image = Image::new(60, 30);
        assert_eq!(
            paint(&mut image, &Record::Text(0, 0, Colour::Label, "a?b".into())),
            1
        );
        assert_eq!(image.pixel(16, 4), Some(Colour::Label.rgb()));
    }

    #[test]
    fn render_shows_tracks_clips_keys_wave_and_playhead() {
        let view = build_view().unwrap();
        let (image, stats) = render(&view).unwrap();
        assert_eq!((image.width, image.height), SIZE);
        assert_eq!(stats.playhead_x, 450);
        assert_eq!(stats.tofu, 0);
        assert_eq!(stats.records, 16);
        assert_eq!(
            stats.records,
            parse_draw_list(&view.draw_list().unwrap()).unwrap().len()
        );
        for (x, y, colour) in [
            (450, 5, Colour::Playhead),
            (700, 160, Colour::Clip),
            (500, 60, Colour::Key),
            (1100, 96, Colour::Background),
            (1, 300, Colour::Background),
            (450, 96, Colour::Playhead),
        ] {
            assert_eq!(image.pixel(x, y), Some(colour.rgb()), "({x},{y})");
        }
        assert_eq!(image.pixel(400, 96), Some(WAVE));
        let count = (72..120)
            .filter(|y| image.pixel(400, *y) == Some(WAVE))
            .count();
        assert!((17..=21).contains(&count), "wave pixels={count}");
        assert_eq!(stats.wave_columns, 600);
        let ppm = to_ppm(&image);
        assert!(ppm.starts_with(b"P6\n1280 360\n255\n"));
        assert_eq!(ppm.len(), 1_382_416);
    }

    #[test]
    fn render_is_deterministic() {
        let first = render(&build_view().unwrap()).unwrap().0;
        let second = render(&build_view().unwrap()).unwrap().0;
        assert_eq!(to_ppm(&first), to_ppm(&second));
    }

    #[test]
    fn parse_args_takes_only_out() {
        let strings = |args: &[&str]| args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            parse_args(&strings(&["--out", "x.ppm"])),
            Ok("x.ppm".into())
        );
        for args in [
            vec![],
            vec!["--out"],
            vec!["--out", "a", "--out", "b"],
            vec!["--size", "5"],
            vec!["x.ppm"],
            vec!["--out", ""],
            vec!["--out", "--size"],
        ] {
            assert!(parse_args(&strings(&args)).is_err(), "{args:?}");
        }
    }
}
