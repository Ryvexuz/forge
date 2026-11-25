//! Offline glTF round trip; output paths are relative to the Broadsword root.

use scene::gltf::{import_gltf, GltfError};
use scene::gltf_write::export_gltf;
use scene::json::{self, Value};
use scene::scene::Scene;
use scene::usd::{compare_scenes, UsdError};
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

pub struct Options {
    input: String,
    shared: bool,
    output: String,
    corrupt: bool,
}

fn relative(text: &str) -> Result<(), String> {
    if text.is_empty()
        || text.contains(['\\', ':'])
        || text
            .chars()
            .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
        || text.split('/').any(|s| {
            let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
            s.is_empty()
                || s.ends_with(['.', ' '])
                || matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
                )
                || ["COM", "LPT"].into_iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|n| {
                        matches!(n.as_bytes(), [b'1'..=b'9']) || matches!(n, "¹" | "²" | "³")
                    })
                })
        })
        || Path::new(text)
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(format!("usage invalid relative path {text:?}"));
    }
    Ok(())
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut args = args.into_iter();
    let (mut input, mut output, mut corrupt) = (None, None, false);
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--in" | "--in-shared" | "--out" => {
                let slot = if flag == "--out" {
                    &mut output
                } else {
                    &mut input
                };
                if slot.is_some() {
                    return Err(format!("usage repeated or conflicting {flag}"));
                }
                let value = args.next().ok_or_else(|| format!("usage missing {flag}"))?;
                if value.starts_with("--") {
                    return Err(format!("usage missing value for {flag}"));
                }
                relative(&value)?;
                *slot = Some((value, flag == "--in-shared"));
            }
            "--corrupt-translation" if !corrupt => corrupt = true,
            _ => return Err(format!("usage unknown or repeated flag {flag:?}")),
        }
    }
    let (input, shared) = input.ok_or("usage require --in or --in-shared")?;
    let (output, _) = output.ok_or("usage require --out")?;
    if Path::new(&output).extension().is_none_or(|e| e != "gltf") {
        return Err("usage --out must end .gltf".into());
    }
    Ok(Options {
        input,
        shared,
        output,
        corrupt,
    })
}

pub fn find_root(start: &Path) -> Result<PathBuf, String> {
    for directory in start.ancestors() {
        if directory.join("broadsword.toml").is_file() {
            return directory.canonicalize().map_err(|e| format!("root {e}"));
        }
    }
    Err(format!("no broadsword.toml above {}", start.display()))
}

// Reject existing links and check each resolved component before file IO.
fn resolve(base: &Path, relative_path: &str, create: bool) -> Result<PathBuf, String> {
    relative(relative_path)?;
    let base = base.canonicalize().map_err(|e| format!("path {e}"))?;
    let mut path = base.clone();
    let parts: Vec<_> = Path::new(relative_path).components().collect();
    for (i, part) in parts.iter().enumerate() {
        path.push(part.as_os_str());
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !path
                        .canonicalize()
                        .map_err(|e| format!("path {e}"))?
                        .starts_with(&base)
                {
                    return Err("path link or escape refused".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if create && i + 1 < parts.len() {
                    fs::create_dir(&path).map_err(|e| format!("mkdir {e}"))?;
                } else if i + 1 < parts.len() {
                    return Err(format!("path {e}"));
                }
            }
            Err(e) => return Err(format!("path {e}")),
        }
    }
    Ok(path)
}

fn field<'a>(value: &'a Value, key: &str) -> Result<Option<&'a Value>, String> {
    let Value::Object(entries) = value else {
        return Err("import expected object".into());
    };
    let mut matches = entries.iter().filter(|(k, _)| k == key);
    let found = matches.next().map(|(_, v)| v);
    if matches.next().is_some() {
        return Err(format!("import duplicate {key}"));
    }
    Ok(found)
}

fn read_scene(path: &Path) -> Result<Scene, String> {
    let text = fs::read(path).map_err(|e| format!("read {e}"))?;
    let value = json::read(std::str::from_utf8(&text).map_err(|e| format!("import {e}"))?)
        .map_err(|e| format!("import {e}"))?;
    let uri = match field(&value, "buffers")? {
        None => None,
        Some(Value::Array(a)) if a.is_empty() => None,
        Some(Value::Array(a)) if a.len() == 1 => match field(&a[0], "uri")? {
            Some(Value::String(s)) => Some(s.as_str()),
            _ => return Err("import missing buffer uri".into()),
        },
        _ => return Err("import expected at most one buffer".into()),
    };
    let name = uri.unwrap_or("none.bin");
    // Ask the importer to validate the URI before using it as a filesystem path.
    match import_gltf(&text, name, &[]) {
        Ok(_) | Err(GltfError::BinLength { .. }) => {}
        Err(e) => return Err(format!("import {e}")),
    }
    let bin = if let Some(uri) = uri {
        let parent = path.parent().ok_or("read missing parent")?;
        fs::read(resolve(parent, uri, false)?).map_err(|e| format!("read {e}"))?
    } else {
        Vec::new()
    };
    import_gltf(&text, name, &bin).map_err(|e| format!("import {e}"))
}

pub fn corrupt_x(x: f32) -> Result<f32, String> {
    let shifted = x + 1.0;
    if !x.is_finite() || !shifted.is_finite() {
        return Err("usage corrupt translation non-finite".into());
    }
    // The negative control must not report success when f32 rounding hides +1.
    if shifted.to_bits() == x.to_bits() {
        return Err("usage corrupt translation unchanged".into());
    }
    Ok(shifted)
}

fn replace_contents(file: &mut std::fs::File, bytes: &[u8]) -> std::io::Result<()> {
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.flush()
}

pub fn run(options: Options, root: &Path) -> Result<(), String> {
    let base = if options.shared {
        root
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    };
    let a = read_scene(&resolve(base, &options.input, false)?)?;
    let mut written = Scene {
        hierarchy: a.hierarchy.clone(),
        meshes: a.meshes.clone(),
        ..Scene::default()
    };
    if options.corrupt {
        let node = written
            .hierarchy
            .nodes
            .first_mut()
            .ok_or("usage no node to corrupt")?;
        node.transform.translation.x = corrupt_x(node.transform.translation.x)?;
    }
    let bin_relative = Path::new(&options.output).with_extension("bin");
    let bin_relative = bin_relative.to_str().ok_or("usage non-UTF8 output")?;
    let bin_name = Path::new(bin_relative)
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("usage missing output stem")?;
    let (text, bin) = export_gltf(&written, bin_name).map_err(|e| format!("export {e}"))?;
    let output = resolve(root, &options.output, true)?;
    let bin_path = resolve(root, bin_relative, false)?;
    let gltf_existed = fs::symlink_metadata(&output).is_ok();
    let bin_existed = fs::symlink_metadata(&bin_path).is_ok();
    {
        let mut gltf_file = OpenOptions::new()
            .write(true)
            .create_new(!gltf_existed)
            .open(&output)
            .map_err(|e| format!("write gltf {e}"))?;
        let mut bin_file = match OpenOptions::new()
            .write(true)
            .create_new(!bin_existed)
            .open(&bin_path)
        {
            Ok(file) => file,
            Err(e) => {
                drop(gltf_file);
                if !gltf_existed {
                    let _ = fs::remove_file(&output);
                }
                return Err(format!("write bin {e}"));
            }
        };
        replace_contents(&mut bin_file, &bin).map_err(|e| format!("write bin {e}"))?;
        replace_contents(&mut gltf_file, text.as_bytes()).map_err(|e| format!("write gltf {e}"))?;
    }
    let disk_text = fs::read(&output).map_err(|e| format!("read gltf {e}"))?;
    let disk_bin = fs::read(&bin_path).map_err(|e| format!("read bin {e}"))?;
    let b = import_gltf(&disk_text, bin_name, &disk_bin).map_err(|e| format!("import {e}"))?;
    compare_scenes(&a, &b).map_err(|e| match e {
        UsdError::Mismatch { what } => format!("mismatch {what}"),
        other => format!("mismatch {other:?}"),
    })?;
    let again = export_gltf(&b, bin_name).map_err(|e| format!("export {e}"))?;
    if again != (text, bin) {
        return Err("nondeterministic".into());
    }
    println!(
        "gltf_roundtrip: ok nodes={} written={} meshes={} gltf_bytes={} bin_bytes={}",
        a.hierarchy.nodes.len(),
        b.hierarchy.nodes.len(),
        b.meshes.len(),
        disk_text.len(),
        disk_bin.len(),
    );
    Ok(())
}

#[cfg(not(test))]
fn main() -> std::process::ExitCode {
    let args = std::env::args_os()
        .skip(1)
        .map(|s| s.into_string().map_err(|_| "usage non-UTF8 argument".into()))
        .collect::<Result<Vec<_>, String>>();
    let result = args.and_then(parse).and_then(|options| {
        let cwd = std::env::current_dir().map_err(|e| format!("cwd {e}"))?;
        run(options, &find_root(&cwd)?)
    });
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("gltf_roundtrip: FAIL {error}");
            std::process::ExitCode::from(if error.starts_with("usage ") { 2 } else { 1 })
        }
    }
}
