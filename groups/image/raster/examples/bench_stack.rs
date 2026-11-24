//! R8 benchmark; timing excludes setup, the memory read and the reference check.

use raster::adjust::Adjustment;
use raster::layer::{composite_document, composite_pixel, BlendMode, Document, Layer};
use raster::mask::compose_alpha;
use raster::paint::stamp_line;
use raster::tile::{Tile, TileStore, TILE_PIXEL_COUNT, TILE_SIZE};
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const VIEW_W: u32 = 1920;
const VIEW_H: u32 = 1080;

#[derive(Debug, PartialEq)]
struct Options {
    size: u32,
    layers: u32,
    coverage: f64,
    frames: u32,
    budget_ms: f64,
    max_resident_gib: f64,
}

fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    const FLAGS: [&str; 6] = [
        "--size",
        "--layers",
        "--frames",
        "--layer-coverage",
        "--frame-budget-ms",
        "--max-resident-gib",
    ];
    let mut values: [Option<String>; 6] = std::array::from_fn(|_| None);
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        let i = FLAGS
            .iter()
            .position(|&known| known == flag)
            .ok_or_else(|| format!("unknown flag {flag:?}"))?;
        if values[i].is_some() {
            return Err(format!("duplicate {flag}"));
        }
        values[i] = Some(
            args.next()
                .ok_or_else(|| format!("missing value for {flag}"))?,
        );
    }
    for (i, value) in values.iter().enumerate() {
        if value.is_none() {
            return Err(format!("missing {}", FLAGS[i]));
        }
    }
    let integer = |i: usize| -> Result<u32, String> {
        let text = values[i].as_deref().unwrap_or("");
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(format!("invalid {}", FLAGS[i]));
        }
        text.parse::<u32>()
            .ok()
            .filter(|&v| v > 0)
            .ok_or_else(|| format!("invalid {}", FLAGS[i]))
    };
    let real = |i: usize| -> Result<f64, String> {
        values[i]
            .as_deref()
            .unwrap_or("")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.0 && (i != 3 || *v <= 1.0))
            .ok_or_else(|| format!("invalid {}", FLAGS[i]))
    };
    Ok(Options {
        size: integer(0)?,
        layers: integer(1)?,
        frames: integer(2)?,
        coverage: real(3)?,
        budget_ms: real(4)?,
        max_resident_gib: real(5)?,
    })
}

#[derive(Debug, PartialEq)]
struct Layout {
    view_origin: (i64, i64),
    view_tiles: Vec<(i32, i32)>,
    patch_tiles: i32,
    patch_origins: Vec<(i32, i32)>,
}

fn layout(options: &Options) -> Result<Layout, String> {
    let size = i64::from(options.size);
    let t = TILE_SIZE as i64;
    if size < 2048 || size % t != 0 {
        return Err("--size must be a multiple of 256 and at least 2048".into());
    }
    // size is bounded by u32; validated coverage is in (0,1], so this product
    // cannot overflow. Reject nonfinite values before converting to an integer.
    let side = (size as f64 * options.coverage.sqrt()).round();
    if !side.is_finite()
        || side <= 0.0
        || side > size as f64
        || side % t as f64 != 0.0
        || (side / size as f64).powi(2) != options.coverage
    {
        return Err("--layer-coverage must be (k*256/size)^2".into());
    }
    let s = side as i64;
    let vx = (size - i64::from(VIEW_W)) / 2;
    let vy = (size - i64::from(VIEW_H)) / 2;
    let bounds = |v: i64, extent: u32| {
        (
            (-(-(v + i64::from(extent) - s)).div_euclid(t)).max(0),
            (v / t).min((size - s) / t),
        )
    };
    let (lx, hx) = bounds(vx, VIEW_W);
    let (ly, hy) = bounds(vy, VIEW_H);
    if lx > hx || ly > hy {
        return Err("no tile-aligned patch covers the viewport".into());
    }
    let mut view_tiles = Vec::new();
    for y in vy / t..=(vy + i64::from(VIEW_H) - 1) / t {
        for x in vx / t..=(vx + i64::from(VIEW_W) - 1) / t {
            view_tiles.push((x as i32, y as i32));
        }
    }
    let mut patch_origins = Vec::new();
    patch_origins
        .try_reserve_exact(options.layers as usize)
        .map_err(|e| format!("layer layout allocation: {e}"))?;
    for k in 0..i64::from(options.layers) {
        patch_origins.push((
            (lx + 3 * k % (hx - lx + 1)) as i32,
            (ly + 5 * k % (hy - ly + 1)) as i32,
        ));
    }
    Ok(Layout {
        view_origin: (vx, vy),
        view_tiles,
        patch_tiles: (s / t) as i32,
        patch_origins,
    })
}

struct Bench {
    sources: Vec<TileStore>,
    opacity: Vec<f32>,
    modes: Vec<BlendMode>,
    adjust_layer: usize,
    adjustment: Option<Adjustment>,
    adjusted: BTreeMap<(i32, i32), Tile>,
    cache: BTreeMap<(i32, i32), Tile>,
    dirty: BTreeSet<(i32, i32)>,
    view_tiles: Vec<(i32, i32)>,
    size: u32,
}

impl Bench {
    fn new(options: &Options, layout: &Layout) -> Result<Bench, String> {
        let mut bench = Self {
            sources: Vec::new(),
            opacity: Vec::new(),
            modes: Vec::new(),
            adjust_layer: (options.layers / 2) as usize,
            adjustment: None,
            adjusted: BTreeMap::new(),
            cache: BTreeMap::new(),
            dirty: layout.view_tiles.iter().copied().collect(),
            view_tiles: layout.view_tiles.clone(),
            size: options.size,
        };
        for (k, &(x, y)) in layout.patch_origins.iter().enumerate() {
            let a = 0.25 + 0.125 * (k % 5) as f32;
            let colour = [37, 53, 71].map(|m| ((k % 100) * m % 100) as f32 / 100.0);
            let tile = Tile::from_pixels(vec![
                [colour[0] * a, colour[1] * a, colour[2] * a, a];
                TILE_PIXEL_COUNT
            ])
            .map_err(|e| e.to_string())?;
            let mut store = TileStore::new();
            for ty in y..y + layout.patch_tiles {
                for tx in x..x + layout.patch_tiles {
                    store.write_tile(tx, ty, tile.clone());
                }
            }
            bench.sources.push(store);
            bench.opacity.push(0.5 + 0.1 * (k % 5) as f32);
            bench.modes.push(BlendMode::ALL[k % 12]);
        }
        bench.composite_dirty()?;
        Ok(bench)
    }

    fn paint(&mut self, frame: u32) -> Result<(), String> {
        let p = frame as usize % self.sources.len();
        let vx = (i64::from(self.size) - i64::from(VIEW_W)) / 2;
        let vy = (i64::from(self.size) - i64::from(VIEW_H)) / 2;
        let f = i64::from(frame);
        let from = (
            vx + 97 * f % i64::from(VIEW_W),
            vy + 61 * f % i64::from(VIEW_H),
        );
        // Tile coordinates fit i32 for all u32 canvas sizes; pixel coordinates
        // required by stamp_line are checked separately, including the endpoint.
        let from = (
            i32::try_from(from.0).map_err(|_| "paint x coordinate overflow")?,
            i32::try_from(from.1).map_err(|_| "paint y coordinate overflow")?,
        );
        let to = (
            from.0.checked_add(40).ok_or("paint x endpoint overflow")?,
            from.1.checked_add(25).ok_or("paint y endpoint overflow")?,
        );
        let changed = stamp_line(&mut self.sources[p], from, to, 12, [0.2, 0.1, 0.05, 0.5])
            .map_err(|e| e.to_string())?;
        for c in changed {
            if self.view_tiles.contains(&c) {
                self.dirty.insert(c);
            }
            if p == self.adjust_layer {
                self.adjusted.remove(&c);
            }
        }
        Ok(())
    }

    fn adjust(&mut self, frame: u32) {
        self.adjustment = Some(Adjustment::BrightnessContrast {
            brightness: ((frame % 21) as f32 - 10.0) * 0.01,
            contrast: 1.0,
        });
        self.adjusted.clear();
        for &c in &self.view_tiles {
            if self.sources[self.adjust_layer].tile_at(c.0, c.1).is_some() {
                self.dirty.insert(c);
            }
        }
    }

    fn composite_dirty(&mut self) -> Result<(), String> {
        // BTreeSet's tuple ordering is deterministic. Whole tiles are composited,
        // including pixels outside the viewport rectangle.
        for &c in &self.dirty {
            let mut pixels = vec![[0.0; 4]; TILE_PIXEL_COUNT];
            for (k, store) in self.sources.iter().enumerate() {
                let Some(raw) = store.tile_at(c.0, c.1) else {
                    continue;
                };
                let source = if k == self.adjust_layer {
                    if let Some(adjustment) = &self.adjustment {
                        if let std::collections::btree_map::Entry::Vacant(entry) =
                            self.adjusted.entry(c)
                        {
                            entry.insert(adjustment.apply(raw).map_err(|e| e.to_string())?);
                        }
                        &self.adjusted[&c]
                    } else {
                        raw
                    }
                } else {
                    raw
                };
                let coverage =
                    compose_alpha(self.opacity[k], 1.0, 1.0).map_err(|e| e.to_string())?;
                for (pixel, &src) in pixels.iter_mut().zip(source.pixels()) {
                    *pixel = composite_pixel(*pixel, src, self.modes[k], coverage);
                }
            }
            self.cache
                .insert(c, Tile::from_pixels(pixels).map_err(|e| e.to_string())?);
        }
        self.dirty.clear();
        Ok(())
    }

    fn verify(&self) -> Result<(), String> {
        for &c in &self.view_tiles {
            let mut doc = Document::new(self.size, self.size).map_err(|e| e.to_string())?;
            for (k, source) in self.sources.iter().enumerate() {
                let Some(raw) = source.tile_at(c.0, c.1) else {
                    continue;
                };
                let tile = match (k == self.adjust_layer, &self.adjustment) {
                    (true, Some(a)) => a.apply(raw).map_err(|e| e.to_string())?,
                    _ => raw.clone(),
                };
                let mut store = TileStore::new();
                store.write_tile(c.0, c.1, tile);
                doc.push_layer(
                    Layer::new(store, self.opacity[k], self.modes[k], None)
                        .map_err(|e| e.to_string())?,
                );
            }
            let expected = composite_document(&doc).map_err(|e| e.to_string())?;
            let zero = Tile::default();
            let expected = expected.tile_at(c.0, c.1).unwrap_or(&zero);
            let matches = self.cache.get(&c).is_some_and(|actual| {
                actual
                    .pixels()
                    .iter()
                    .flatten()
                    .zip(expected.pixels().iter().flatten())
                    .all(|(a, b)| a.to_bits() == b.to_bits())
            });
            if !matches {
                return Err(format!("reference tile=({},{})", c.0, c.1));
            }
        }
        Ok(())
    }
}

fn p99(times: &mut [Duration]) -> Result<Duration, String> {
    if times.is_empty() {
        return Err("timing: empty sample".into());
    }
    times.sort_unstable();
    let rank = (99 * times.len() as u128).div_ceil(100) as usize;
    Ok(times[rank - 1])
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Ok,
    Frame,
    Memory,
}

fn verdict(p99_ms: f64, budget_ms: f64, resident_gib: f64, max_resident_gib: f64) -> Verdict {
    if p99_ms > budget_ms {
        Verdict::Frame
    } else if resident_gib > max_resident_gib {
        Verdict::Memory
    } else {
        Verdict::Ok
    }
}

#[cfg(windows)]
fn peak_resident_bytes() -> Result<u64, String> {
    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }
    let size = std::mem::size_of::<ProcessMemoryCounters>() as u32;
    let mut counters = ProcessMemoryCounters {
        cb: size,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: the pseudo-handle denotes this process, counters is writable with
    // the C layout required by Windows, and both size fields match its storage.
    let result = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
    if result == 0 {
        return Err(format!("memory: {}", std::io::Error::last_os_error()));
    }
    Ok(counters.peak_working_set_size as u64)
}

#[cfg(not(windows))]
fn peak_resident_bytes() -> Result<u64, String> {
    let status =
        std::fs::read_to_string("/proc/self/status").map_err(|e| format!("memory: {e}"))?;
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix("VmHWM:"))
        .ok_or("memory: missing VmHWM")?;
    let mut fields = line.split_whitespace();
    let kb = fields
        .next()
        .and_then(|v| v.parse::<u64>().ok())
        .ok_or("memory: invalid VmHWM")?;
    if fields.next() != Some("kB") || fields.next().is_some() {
        return Err("memory: invalid VmHWM unit".into());
    }
    kb.checked_mul(1024).ok_or("memory: VmHWM overflow".into())
}

fn run(options: Options) -> Result<u8, String> {
    let layout = layout(&options)?;
    let mut bench = Bench::new(&options, &layout)?;
    let mut times = Vec::new();
    times
        .try_reserve_exact(options.frames as usize)
        .map_err(|e| format!("timing allocation: {e}"))?;
    println!(
        "bench_stack: setup size={} layers={} view_tiles={} patch_tiles={}",
        options.size,
        options.layers,
        layout.view_tiles.len(),
        layout.patch_tiles
    );
    for frame in 0..options.frames {
        let start = Instant::now();
        bench.paint(frame)?;
        bench.adjust(frame);
        bench.composite_dirty()?;
        times.push(start.elapsed());
    }
    let resident_gib = peak_resident_bytes()? as f64 / 1_073_741_824.0;
    if let Err(error) = bench.verify() {
        if error.starts_with("reference tile=(") {
            println!("bench_stack: FAIL {error}");
            return Ok(1);
        }
        return Err(error);
    }
    let p99_ms = p99(&mut times)?.as_secs_f64() * 1000.0;
    match verdict(
        p99_ms,
        options.budget_ms,
        resident_gib,
        options.max_resident_gib,
    ) {
        Verdict::Ok => {
            println!(
                "bench_stack: ok size={} layers={} frames={} p99_ms={p99_ms:.3} resident_gib={resident_gib:.3}",
                options.size, options.layers, options.frames
            );
            Ok(0)
        }
        Verdict::Frame => {
            println!(
                "bench_stack: FAIL frame p99_ms={p99_ms:.3} budget_ms={:.3} resident_gib={resident_gib:.3}",
                options.budget_ms
            );
            Ok(1)
        }
        Verdict::Memory => {
            println!(
                "bench_stack: FAIL memory resident_gib={resident_gib:.3} max_resident_gib={:.3} p99_ms={p99_ms:.3}",
                options.max_resident_gib
            );
            Ok(1)
        }
    }
}

fn main() -> std::process::ExitCode {
    match parse(std::env::args().skip(1)).and_then(run) {
        Ok(code) => std::process::ExitCode::from(code),
        Err(error) => {
            eprintln!("bench_stack: error {error}");
            std::process::ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments() -> Vec<String> {
        "--size 16384 --layers 50 --layer-coverage 0.0625 --frames 600 --frame-budget-ms 16.7 --max-resident-gib 16"
            .split_whitespace()
            .map(str::to_owned)
            .collect()
    }

    fn small() -> Options {
        Options {
            size: 2048,
            layers: 3,
            coverage: 1.0,
            ..parse(arguments()).unwrap()
        }
    }

    fn new_small() -> Bench {
        let options = small();
        Bench::new(&options, &layout(&options).unwrap()).unwrap()
    }

    #[test]
    fn arguments_fail_closed() {
        assert_eq!(
            parse(arguments()).unwrap(),
            Options {
                size: 16384,
                layers: 50,
                coverage: 0.0625,
                frames: 600,
                budget_ms: 16.7,
                max_resident_gib: 16.0,
            }
        );
        for (flag, bad) in [
            ("--layers", "0"),
            ("--size", "-1"),
            ("--size", "12a"),
            ("--size", "4294967296"),
            ("--size", "1\r"),
            ("--frame-budget-ms", "nan"),
            ("--frame-budget-ms", "0"),
            ("--max-resident-gib", "inf"),
            ("--layer-coverage", "1.5"),
        ] {
            let mut args = arguments();
            let i = args.iter().position(|s| s == flag).unwrap();
            args[i + 1] = bad.into();
            assert!(parse(args).unwrap_err().contains(flag));
        }
        for extra in [
            vec!["--unknown", "1"],
            vec!["--size", "2048"],
            vec!["--unknown"],
        ] {
            let mut args = arguments();
            args.extend(extra.into_iter().map(str::to_owned));
            assert!(parse(args).is_err());
        }
        assert!(parse(["--size".to_owned()]).is_err());
        let mut args = arguments();
        let i = args.iter().position(|s| s == "--frames").unwrap();
        args.drain(i..i + 2);
        assert!(parse(args).unwrap_err().contains("--frames"));
    }

    #[test]
    fn p99_is_nearest_rank() {
        let mut times: Vec<_> = (1..=600).rev().map(Duration::from_millis).collect();
        assert_eq!(p99(&mut times).unwrap(), Duration::from_millis(594));
        assert_eq!(
            p99(&mut [Duration::from_millis(7)]).unwrap(),
            Duration::from_millis(7)
        );
        let mut times: Vec<_> = (1..=101).map(Duration::from_millis).collect();
        assert_eq!(p99(&mut times).unwrap(), Duration::from_millis(100));
        assert!(p99(&mut []).is_err());
    }

    #[test]
    fn budget_verdict_checks_frame_then_memory() {
        assert_eq!(verdict(16.7, 16.7, 16.0, 16.0), Verdict::Ok);
        assert_eq!(verdict(16.8, 16.7, 1.0, 16.0), Verdict::Frame);
        assert_eq!(verdict(16.8, 16.7, 17.0, 16.0), Verdict::Frame);
        assert_eq!(verdict(1.0, 16.7, 16.1, 16.0), Verdict::Memory);
    }

    #[test]
    fn layout_covers_viewport() {
        let full = parse(arguments()).unwrap();
        let l = layout(&full).unwrap();
        assert_eq!(l.view_origin, (7232, 7652));
        assert_eq!(l.patch_tiles, 16);
        assert_eq!(l.view_tiles.first(), Some(&(28, 29)));
        assert_eq!(l.view_tiles.last(), Some(&(35, 34)));
        assert_eq!(l.patch_origins.iter().map(|c| c.0).min(), Some(20));
        // Allowed x origins are 20..=28, but (3 * k) mod 9 only reaches 0, 3 and 6.
        assert_eq!(l.patch_origins.iter().map(|c| c.0).max(), Some(26));
        assert!(l.patch_origins.iter().all(|c| [20, 23, 26].contains(&c.0)));
        assert_eq!(l.patch_origins.iter().map(|c| c.1).min(), Some(19));
        assert_eq!(l.patch_origins.iter().map(|c| c.1).max(), Some(29));
        let medium = Options {
            size: 4096,
            coverage: 0.25,
            layers: 4,
            ..small()
        };
        for o in [full, medium, small()] {
            let l = layout(&o).unwrap();
            assert_eq!(l.view_tiles.len(), 48);
            assert!(l
                .view_tiles
                .windows(2)
                .all(|w| (w[0].1, w[0].0) < (w[1].1, w[1].0)));
            for (x, y) in l.patch_origins {
                assert!(x >= 0 && y >= 0);
                assert!(i64::from(x + l.patch_tiles) * TILE_SIZE as i64 <= i64::from(o.size));
                assert!(i64::from(y + l.patch_tiles) * TILE_SIZE as i64 <= i64::from(o.size));
                for &(tx, ty) in &l.view_tiles {
                    assert!(tx >= x && tx < x + l.patch_tiles);
                    assert!(ty >= y && ty < y + l.patch_tiles);
                }
            }
        }
        for size in [2000, 1792] {
            assert!(layout(&Options { size, ..small() }).is_err());
        }
        assert!(layout(&Options {
            size: 16384,
            coverage: 0.01,
            ..small()
        })
        .is_err());
    }

    #[test]
    fn paint_only_frames_match_reference() {
        let mut bench = new_small();
        let before = bench.cache.clone();
        for f in 0..5 {
            bench.paint(f).unwrap();
            bench.composite_dirty().unwrap();
        }
        bench.verify().unwrap();
        assert_ne!(bench.cache, before);
    }

    #[test]
    fn full_frames_match_reference() {
        let mut bench = new_small();
        for f in 0..5 {
            bench.paint(f).unwrap();
            bench.adjust(f);
            bench.composite_dirty().unwrap();
            bench.verify().unwrap();
        }
    }

    #[test]
    fn verify_detects_stale_cache() {
        for skip_last in [false, true] {
            let mut bench = new_small();
            for f in 0..5 {
                bench.paint(f).unwrap();
                bench.adjust(f);
                if !skip_last || f != 4 {
                    bench.composite_dirty().unwrap();
                }
            }
            if skip_last {
                assert!(bench.verify().unwrap_err().starts_with("reference tile=("));
            } else {
                let c = bench.view_tiles[0];
                bench.cache.insert(c, Tile::default());
                assert_eq!(
                    bench.verify(),
                    Err(format!("reference tile=({},{})", c.0, c.1))
                );
            }
        }
    }

    #[test]
    fn peak_resident_is_positive() {
        let mut bytes = vec![0_u8; 64 * 1024 * 1024];
        for i in (0..bytes.len()).step_by(4096) {
            bytes[i] = 1;
        }
        std::hint::black_box(&bytes);
        assert!(peak_resident_bytes().unwrap() >= bytes.len() as u64);
        std::hint::black_box(bytes);
    }
}
