use std::collections::{BTreeMap, BTreeSet};

use mesh_kernel::gen::{
    closed_box_with_triangles, icosphere, jittered_closed_mesh, GenerateError, Mesh, SeededRng,
};

const PACKAGE: &str =
    "[package]\nname = \"mesh-kernel\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

// This is deliberately a fail-closed manifest-subset guard, not a TOML parser.
// Supporting new manifest syntax requires extending the negative fixtures too.
fn manifest_has_no_dependencies(manifest: &str) -> Result<(), String> {
    let mut package = false;
    let mut has_package = false;
    let mut tables = BTreeSet::new();
    let mut package_keys = BTreeSet::new();
    for (index, raw) in manifest.lines().enumerate() {
        let fail = |reason: &str| format!("line {}: {reason}", index + 1);
        let line = without_comment(raw)?.trim();
        if line.is_empty() {
            continue;
        }
        if line.contains("\"\"\"") || line.contains("'''") {
            return Err(fail("multiline strings are unsupported"));
        }
        if line.starts_with('[') {
            let inner = line
                .strip_prefix('[')
                .and_then(|text| text.strip_suffix(']'))
                .ok_or_else(|| fail("malformed table header"))?;
            let path = key_path(inner)?;
            if !tables.insert(path.clone()) {
                return Err(fail("duplicate table"));
            }
            package = match path.as_slice() {
                [name] if name == "package" => true,
                [name] if DEPENDENCY_TABLES.contains(&name.as_str()) => false,
                [target, selector] if target == "target" && !selector.is_empty() => false,
                [target, selector, name]
                    if target == "target"
                        && !selector.is_empty()
                        && DEPENDENCY_TABLES.contains(&name.as_str()) =>
                {
                    false
                }
                _ => return Err(fail("unsupported table or dependency subtable")),
            };
            has_package |= package;
        } else {
            if !package {
                return Err(fail(
                    "entry outside package: dependency tables must be empty",
                ));
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| fail("malformed package assignment"))?;
            let path = key_path(key)?;
            if path.len() != 1 || value.trim().is_empty() {
                return Err(fail("unsupported package assignment"));
            }
            if !package_keys.insert(path) {
                return Err(fail("duplicate package key"));
            }
        }
    }
    if has_package {
        Ok(())
    } else {
        Err("missing package table".into())
    }
}

fn without_comment(line: &str) -> Result<&str, String> {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if character.is_control() && character != '\t' {
            return Err("control character in manifest line".into());
        }
        if let Some(delimiter) = quote {
            if escaped {
                escaped = false;
            } else if delimiter == '"' && character == '\\' {
                escaped = true;
            } else if character == delimiter {
                quote = None;
            }
        } else {
            match character {
                '#' => return Ok(&line[..index]),
                '"' | '\'' => quote = Some(character),
                _ => {}
            }
        }
    }
    if quote.is_some() || escaped {
        return Err("unterminated or multiline string".into());
    }
    Ok(line)
}

fn key_path(text: &str) -> Result<Vec<String>, String> {
    let mut chars = text.trim().chars().peekable();
    let mut parts = Vec::new();
    loop {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        let mut part = String::new();
        if let Some(delimiter @ ('"' | '\'')) = chars.peek().copied() {
            chars.next();
            let mut closed = false;
            for character in chars.by_ref() {
                if character == delimiter {
                    closed = true;
                    break;
                }
                if character == '\\' || character.is_control() {
                    return Err("escaped or control-character table keys are unsupported".into());
                }
                part.push(character);
            }
            if !closed {
                return Err("unterminated quoted key".into());
            }
        } else {
            while let Some(&character) = chars.peek() {
                if !character.is_ascii_alphanumeric() && character != '_' && character != '-' {
                    break;
                }
                part.push(character);
                chars.next();
            }
        }
        if part.is_empty() {
            return Err("empty or unsupported key".into());
        }
        parts.push(part);
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        match chars.next() {
            None => return Ok(parts),
            Some('.') => {}
            Some(_) => return Err("unsupported key syntax".into()),
        }
    }
}

#[test]
fn cargo_toml_has_no_dependencies() {
    let manifest = include_str!("../Cargo.toml");
    assert!(!manifest.trim().is_empty(), "manifest must not be empty");
    eprintln!("manifest_bytes={}", manifest.len());
    manifest_has_no_dependencies(manifest)
        .unwrap_or_else(|reason| panic!("dependency-free manifest guard failed: {reason}"));
}

#[test]
fn dependency_guard_accepts_empty_tables_and_ignores_string_contents() {
    assert!(manifest_has_no_dependencies(PACKAGE).is_ok());
    for table in DEPENDENCY_TABLES {
        let manifest = format!("{PACKAGE}\n[ {table} ] # no entries\n");
        assert!(
            manifest_has_no_dependencies(&manifest).is_ok(),
            "{manifest}"
        );
        for selector in [
            "'cfg(unix)'",
            "'cfg(target_os = \"windows\")'",
            "\"x86_64-pc-windows-msvc\"",
        ] {
            let manifest = format!("{PACKAGE}\n[target.{selector}.{table}]\n");
            assert!(
                manifest_has_no_dependencies(&manifest).is_ok(),
                "{manifest}"
            );
        }
    }
    let harmless = format!(
        "{PACKAGE}description = \"# [dependencies] escaped \\\"quote\\\"\"\n\
         [\"target\".'cfg(unix)'.\"dependencies\"]\n# injected = \"1\"\n"
    );
    assert!(manifest_has_no_dependencies(&harmless).is_ok());
    assert!(manifest_has_no_dependencies(&PACKAGE.replace('\n', "\r\n")).is_ok());
}

#[test]
fn dependency_guard_rejects_mutations() {
    // Disabling the guard's non-package-entry rejection makes these assertions
    // fail behaviorally: an injected dependency would incorrectly be accepted.
    for table in DEPENDENCY_TABLES {
        for prefix in ["", "target.'cfg(windows)'."] {
            for entry in [
                "injected = \"1\"",
                "\"injected\" = { version = \"1\" }",
                "injected = { path = 'C:\\hostile\\crate' }",
                "injected.version = \"1\" # comment",
            ] {
                let mutated = format!("{PACKAGE}\n[{prefix}{table}]\n{entry}\n");
                assert!(
                    manifest_has_no_dependencies(&mutated).is_err(),
                    "accepted dependency mutation: {mutated}"
                );
            }
            let subtable = format!("{PACKAGE}\n[{prefix}{table}.injected]\n");
            assert!(manifest_has_no_dependencies(&subtable).is_err());
        }
    }
    for hostile in [
        "",
        "# no manifest content\n",
        "dependencies = { injected = \"1\" }\n",
        "[target]\n'cfg(unix)' = { dependencies = { injected = \"1\" } }\n",
        "[[dependencies]]\n",
        "[\"\\u0064ependencies\"]\n",
        "[dependencies]\n[dependencies]\n",
        "[dependencies]\n[\"dependencies\"]\n",
        "[package]\nname = 'first'\nname = 'duplicate'\n",
        "[package]\ndescription = '''\n[dependencies]\ninjected = '1'\n'''\n",
        "[dependencies\r.injected]\n",
        "[dependencies\n",
        "[workspace]\n",
    ] {
        assert!(
            manifest_has_no_dependencies(hostile).is_err(),
            "accepted unsupported or hostile manifest: {hostile}"
        );
    }
    let huge = format!(
        "{PACKAGE}[dependencies]\ninjected = '{}'\n",
        "x".repeat(100_000)
    );
    assert!(manifest_has_no_dependencies(&huge).is_err());
}

fn root(parents: &mut [usize], mut vertex: usize) -> usize {
    while parents[vertex] != vertex {
        parents[vertex] = parents[parents[vertex]];
        vertex = parents[vertex];
    }
    vertex
}

// Independent indexed-topology check: each edge occurs twice in opposite
// directions, all vertices participate in one component, and Euler's value is 2.
// Positive scalar triple products define outward orientation for these
// origin-containing fixtures; the strict comparison with zero is intentional.
fn assert_closed_outward(mesh: &Mesh) {
    assert!(!mesh.triangles.is_empty());
    assert!(!mesh.vertices.is_empty());
    let mut edges = BTreeMap::<(usize, usize), (usize, isize)>::new();
    let mut used = vec![false; mesh.vertices.len()];
    let mut parents: Vec<_> = (0..mesh.vertices.len()).collect();
    for &[a, b, c] in &mesh.triangles {
        assert!(a < mesh.vertices.len() && b < mesh.vertices.len() && c < mesh.vertices.len());
        assert!(a != b && b != c && c != a);
        for (u, v) in [(a, b), (b, c), (c, a)] {
            used[u] = true;
            used[v] = true;
            let key = (u.min(v), u.max(v));
            let entry = edges.entry(key).or_default();
            entry.0 += 1;
            entry.1 += if u < v { 1 } else { -1 };
            let u_root = root(&mut parents, u);
            let v_root = root(&mut parents, v);
            parents[u_root] = v_root;
        }
        assert!(triple(mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]) > 0.0);
    }
    assert!(used.into_iter().all(|value| value));
    let first = root(&mut parents, 0);
    for vertex in 0..mesh.vertices.len() {
        assert_eq!(root(&mut parents, vertex), first);
    }
    for (edge, incidence) in &edges {
        assert_eq!(*incidence, (2, 0), "edge {edge:?}");
    }
    assert_eq!(mesh.vertices.len() + mesh.triangles.len(), edges.len() + 2);
    assert!(mesh
        .vertices
        .iter()
        .flatten()
        .all(|value| value.is_finite()));
}

fn triple(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> f64 {
    a[0] * (b[1] * c[2] - b[2] * c[1])
        + a[1] * (b[2] * c[0] - b[0] * c[2])
        + a[2] * (b[0] * c[1] - b[1] * c[0])
}

#[test]
fn generated_box_has_exact_triangle_count() {
    for n in [100_000, 12, 14, 16] {
        let mesh = closed_box_with_triangles(n).expect("valid even box count");
        eprintln!(
            "requested_triangles={n} generated_triangles={}",
            mesh.triangles.len()
        );
        assert_eq!(mesh.triangles.len(), n);
        assert_eq!(mesh.vertices.len(), n / 2 + 2);
        assert_closed_outward(&mesh);
        assert!(mesh
            .vertices
            .iter()
            .flatten()
            .all(|coordinate| (-1.0..=1.0).contains(coordinate)));
        let volume: f64 = mesh
            .triangles
            .iter()
            .map(|&[a, b, c]| triple(mesh.vertices[a], mesh.vertices[b], mesh.vertices[c]) / 6.0)
            .sum();
        // Analytic volume of [-1,1]^3 is 8. The absolute tolerance allows
        // 64 machine epsilons per face for arithmetic and accumulation.
        let tolerance = 64.0 * f64::EPSILON * n as f64;
        assert!((volume - 8.0).abs() <= tolerance, "volume={volume}");
    }
    assert_eq!(
        closed_box_with_triangles(13),
        Err(GenerateError::InvalidTriangleCount)
    );
}

#[test]
fn invalid_and_unrepresentable_sizes_are_errors() {
    for n in 0..12 {
        assert_eq!(
            closed_box_with_triangles(n),
            Err(GenerateError::InvalidTriangleCount)
        );
    }
    assert_eq!(
        closed_box_with_triangles(usize::MAX),
        Err(GenerateError::InvalidTriangleCount)
    );
    // Even, but its vertex storage alone exceeds the platform's Vec layout.
    assert_eq!(
        closed_box_with_triangles(usize::MAX - 1),
        Err(GenerateError::AllocationFailed)
    );
    assert_eq!(icosphere(u32::MAX), Err(GenerateError::CountOverflow));
}

#[test]
fn seeded_rng_matches_splitmix64_reference_vector() {
    // SplitMix64 reference sequence starting with state zero, not generated
    // from the implementation under test.
    let mut rng = SeededRng::new(0);
    for expected in [
        0xe220_a839_7b1d_cdaf,
        0x6e78_9e6a_a1b9_65f4,
        0x06c4_5d18_8009_454f,
    ] {
        assert_eq!(rng.next_u64(), expected);
    }
}

#[test]
fn seeded_jitter_has_reference_count_and_closed_bounded_geometry() {
    let mut counts = BTreeSet::new();
    for seed in [0, 1, 2, 42, u64::MAX] {
        let mesh = jittered_closed_mesh(seed).expect("every seed is valid");
        let count = mesh.triangles.len();
        if seed == 0 {
            // Count from the first SplitMix64 reference word modulo 995.
            assert_eq!(count, 1_322);
            // Decimal-arithmetic references from the next two reference words,
            // after discarding their low 11 bits and applying the corner map.
            for (actual, expected) in mesh.vertices[0]
                .iter()
                .zip([-1.008_559_000_368_936_3, -1.059_195_778_550_925_3])
            {
                assert!((actual - expected).abs() <= 4.0 * f64::EPSILON);
            }
        }
        assert!((12..=2_000).contains(&count));
        assert_eq!(count % 2, 0);
        counts.insert(count);
        assert_closed_outward(&mesh);
        assert!(mesh
            .vertices
            .iter()
            .flatten()
            .all(|coordinate| (-17.0 / 16.0..=17.0 / 16.0).contains(coordinate)));
    }
    assert!(counts.len() > 1, "the seed must influence the count");
    // This seed makes the first internal state zero, hence its mixed word is
    // zero and selects the lower count endpoint exactly.
    let zero_state_seed = 0_u64.wrapping_sub(0x9e37_79b9_7f4a_7c15);
    assert_eq!(
        jittered_closed_mesh(zero_state_seed)
            .unwrap()
            .triangles
            .len(),
        12
    );
}

#[test]
fn icospheres_have_shared_edges_and_unit_radius() {
    for (level, count) in [(0, 20), (1, 80), (2, 320), (6, 81_920)] {
        let mesh = icosphere(level).expect("representable fixture level");
        assert_eq!(mesh.triangles.len(), count);
        assert_eq!(mesh.vertices.len(), count / 2 + 2);
        assert_closed_outward(&mesh);
        // Absolute tolerance: 32 machine epsilons for normalization and the
        // three squared components of a unit-radius vertex.
        for point in &mesh.vertices {
            let radius_squared: f64 = point.iter().map(|coordinate| coordinate * coordinate).sum();
            assert!((radius_squared - 1.0).abs() <= 32.0 * f64::EPSILON);
        }
    }
    // Rounded high-precision values for a unit icosahedron's golden rectangle,
    // an external numerical reference rather than another generator call.
    let base = icosphere(0).unwrap();
    for (actual, expected) in
        base.vertices[0]
            .iter()
            .zip([-0.525_731_112_119_133_6, 0.850_650_808_352_039_9, 0.0])
    {
        assert!((actual - expected).abs() <= 4.0 * f64::EPSILON);
    }
}
