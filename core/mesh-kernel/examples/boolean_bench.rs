use mesh_kernel::boolean::{union, watertight_gate};
use mesh_kernel::gen;
use mesh_kernel::halfedge::{Mesh, MeshError};
use mesh_kernel::vec3::Vec3;
use std::process::ExitCode;
use std::time::Instant;

struct Options {
    tris: usize,
    corrupt_watertight: bool,
}

fn parse_options() -> Result<Options, &'static str> {
    let mut args = std::env::args_os().skip(1);
    let mut tris = None;
    let mut corrupt_watertight = false;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--tris") => {
                if tris.is_some() {
                    return Err("repeated --tris");
                }
                let value = args.next().ok_or("missing --tris value")?;
                let value = value.to_str().ok_or("invalid decimal usize")?;
                if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return Err("invalid decimal usize");
                }
                tris = Some(value.parse::<usize>().map_err(|_| "invalid decimal usize")?);
            }
            Some("--corrupt-watertight") => {
                if corrupt_watertight {
                    return Err("repeated --corrupt-watertight");
                }
                corrupt_watertight = true;
            }
            _ => return Err("unknown argument"),
        }
    }
    Ok(Options {
        tris: *tris.get_or_insert(100_000),
        corrupt_watertight,
    })
}

fn convert(fixture: gen::Mesh, offset: [f64; 3]) -> Result<Mesh, MeshError> {
    Mesh::from_triangles(
        fixture
            .vertices
            .into_iter()
            .map(|[x, y, z]| Vec3::new(x + offset[0], y + offset[1], z + offset[2]))
            .collect(),
        &fixture.triangles,
    )
}

fn run(options: Options) -> Result<(), String> {
    let a = gen::closed_box_with_triangles(options.tris)
        .map_err(|error| format!("generate {error}"))?;
    let a = convert(a, [0.0; 3]).map_err(|error| format!("mesh {error}"))?;
    let b = gen::closed_box_with_triangles(options.tris)
        .map_err(|error| format!("generate {error}"))?;
    let b = convert(b, [0.3, 0.2, 0.1]).map_err(|error| format!("mesh {error}"))?;

    let start = Instant::now();
    let result = union(&a, &b);
    let ms = start.elapsed().as_millis();
    let mut mesh = result.map_err(|error| format!("union {error}"))?;
    if options.corrupt_watertight {
        let (vertices, mut triangles) = mesh.to_triangles();
        triangles.pop();
        mesh = Mesh::from_triangles(vertices, &triangles)
            .map_err(|error| format!("mesh {error}"))?;
    }
    if mesh.faces.is_empty() {
        return Err("empty".to_owned());
    }
    let _checked = watertight_gate(mesh).map_err(|_| "watertight".to_owned())?;
    println!(
        "boolean_bench: ok tris_a={} tris_b={} watertight=true ms={ms}",
        a.faces.len(),
        b.faces.len()
    );
    Ok(())
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(options) => options,
        Err(reason) => {
            eprintln!(
                "boolean_bench: usage: boolean_bench [--tris <n>] [--corrupt-watertight] ({reason})"
            );
            return ExitCode::from(2);
        }
    };
    match run(options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(reason) => {
            println!("boolean_bench: FAIL {reason}");
            ExitCode::FAILURE
        }
    }
}
