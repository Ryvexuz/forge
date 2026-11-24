use nodegraph::capi::*;
use std::collections::BTreeMap;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

static ABI_TESTS: Mutex<()> = Mutex::new(());

fn code(actual: i32, expected: i32) {
    assert!(
        [
            NG_OK,
            NG_ERR_NULL_HANDLE,
            NG_ERR_STALE_HANDLE,
            NG_ERR_NULL_POINTER,
            NG_ERR_BAD_UTF8,
            NG_ERR_BUFFER_TOO_SMALL,
            NG_ERR_GRAPH,
            NG_ERR_EVAL,
            NG_ERR_FORMAT,
            NG_ERR_BAD_VALUE,
            NG_ERR_NO_VALUE,
            NG_ERR_PANIC,
            NG_ERR_POISONED,
            NG_ERR_LIMIT,
        ]
        .contains(&actual),
        "undeclared status {actual}"
    );
    assert_eq!(actual, expected);
}

fn create() -> u64 {
    let mut h = 0;
    // SAFETY: writable aligned output.
    code(unsafe { ng_graph_create(&mut h) }, NG_OK);
    assert_ne!(h, 0);
    h
}

fn add(h: u64, name: &[u8]) -> u64 {
    let mut node = u64::MAX;
    // SAFETY: readable name and disjoint writable output.
    code(
        unsafe { ng_add_node(h, name.as_ptr(), name.len(), 0.0, 0.0, &mut node) },
        NG_OK,
    );
    node
}

fn f64_param(h: u64, node: u64, value: f64) {
    let name: &[u8] = b"value";
    // SAFETY: readable name.
    code(
        unsafe { ng_set_param_f64(h, node, name.as_ptr(), name.len(), value) },
        NG_OK,
    );
}

fn connect(h: u64, from: u64, to: u64, input: &[u8]) {
    let out: &[u8] = b"out";
    // SAFETY: both port slices are readable.
    code(
        unsafe {
            ng_connect(
                h,
                from,
                out.as_ptr(),
                out.len(),
                to,
                input.as_ptr(),
                input.len(),
            )
        },
        NG_OK,
    );
}

fn evaluate(h: u64, node: u64, expected: i32) {
    let out: &[u8] = b"out";
    let mut tag = -99;
    // SAFETY: readable port and disjoint aligned writable output.
    code(
        unsafe { ng_evaluate(h, node, out.as_ptr(), out.len(), &mut tag) },
        NG_OK,
    );
    assert_eq!(tag, expected);
}

fn read_f64(h: u64) -> f64 {
    let mut value = -99.0;
    // SAFETY: aligned writable output.
    code(unsafe { ng_read_f64(h, &mut value) }, NG_OK);
    value
}

fn save(h: u64) -> Vec<u8> {
    let mut n = 0;
    // SAFETY: size query with writable length.
    code(
        unsafe { ng_save(h, null_mut(), 0, &mut n) },
        NG_ERR_BUFFER_TOO_SMALL,
    );
    assert!(n > 0);
    let mut bytes = vec![0; n];
    // SAFETY: writable capacity n and disjoint length output.
    code(
        unsafe { ng_save(h, bytes.as_mut_ptr(), bytes.len(), &mut n) },
        NG_OK,
    );
    assert_eq!(n, bytes.len());
    bytes
}

fn last_error() -> String {
    let mut n = 0;
    // SAFETY: size query with writable length.
    code(
        unsafe { ng_last_error(null_mut(), 0, &mut n) },
        NG_ERR_BUFFER_TOO_SMALL,
    );
    assert!(n > 0, "a refused call must leave a diagnostic");
    let mut bytes = vec![0; n];
    // SAFETY: writable capacity n and disjoint length output.
    code(
        unsafe { ng_last_error(bytes.as_mut_ptr(), bytes.len(), &mut n) },
        NG_OK,
    );
    assert_eq!(n, bytes.len());
    String::from_utf8(bytes).expect("UTF-8 diagnostic")
}

// Snapshot stored values as bits, not approximate numeric references.
fn stored(h: u64) -> Vec<u8> {
    let mut value: f64 = -99.0;
    // SAFETY: aligned writable output.
    let status = unsafe { ng_read_f64(h, &mut value) };
    if status == NG_OK {
        return value.to_bits().to_ne_bytes().to_vec();
    }
    assert_eq!(value, -99.0, "failed read wrote output");
    if status == NG_ERR_NO_VALUE {
        code(status, NG_ERR_NO_VALUE);
        return Vec::new();
    }
    code(status, NG_ERR_BAD_VALUE);
    let (mut n, mut w, mut height) = (0, 99, 99);
    // SAFETY: size query with aligned, disjoint scalar outputs.
    code(
        unsafe { ng_read_image(h, null_mut(), 0, &mut n, &mut w, &mut height) },
        NG_ERR_BUFFER_TOO_SMALL,
    );
    assert_eq!((w, height), (99, 99));
    let mut pixels = vec![0.0; n];
    // SAFETY: buffer has n writable floats, outputs are disjoint.
    code(
        unsafe { ng_read_image(h, pixels.as_mut_ptr(), n, &mut n, &mut w, &mut height) },
        NG_OK,
    );
    let mut bits = w.to_ne_bytes().to_vec();
    bits.extend(height.to_ne_bytes());
    for pixel in pixels {
        bits.extend(pixel.to_bits().to_ne_bytes());
    }
    bits
}

fn refused(h: u64, expected: i32, call: impl FnOnce() -> i32) -> String {
    let before = save(h);
    let value = stored(h);
    code(call(), expected);
    let message = last_error();
    assert_eq!(save(h), before, "refused call changed canonical graph");
    assert_eq!(stored(h), value, "refused call changed stored value");
    message
}

fn solid(h: u64) -> u64 {
    let node = add(h, b"image.solid");
    for name in [b"width".as_slice(), b"height".as_slice()] {
        // SAFETY: readable name.
        code(
            unsafe { ng_set_param_i64(h, node, name.as_ptr(), name.len(), 8) },
            NG_OK,
        );
    }
    f64_param(h, node, 0.25);
    node
}

#[test]
fn capi_round_trip_through_extern_c() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    let h = create();
    let a = add(h, b"math.constant");
    let b = add(h, b"math.constant");
    let c = add(h, b"math.add");
    f64_param(h, a, 3.0);
    f64_param(h, b, 4.0);
    connect(h, a, c, b"a");
    connect(h, b, c, b"b");
    evaluate(h, c, NG_TYPE_F64);
    assert_eq!(read_f64(h), 7.0);
    let image = solid(h);
    let invert = add(h, b"image.invert");
    connect(h, image, invert, b"a");
    evaluate(h, invert, NG_TYPE_IMAGE);
    let (mut n, mut w, mut height) = (0, 99, 99);
    // SAFETY: all allocations and lengths below are valid and disjoint.
    unsafe {
        code(
            ng_read_image(h, null_mut(), 0, &mut n, &mut w, &mut height),
            NG_ERR_BUFFER_TOO_SMALL,
        );
        assert_eq!(n, 64);
        assert_eq!((w, height), (99, 99));
        let mut pixels = vec![-99.0; n];
        code(
            ng_read_image(
                h,
                pixels.as_mut_ptr(),
                pixels.len(),
                &mut n,
                &mut w,
                &mut height,
            ),
            NG_OK,
        );
        assert_eq!((w, height), (8, 8));
        assert_eq!(pixels, vec![0.75; 64]);
        let bytes = save(h);
        assert!(bytes.starts_with(b"schema=1\n"));
        let h2 = create();
        code(ng_load(h2, bytes.as_ptr(), bytes.len()), NG_OK);
        assert_eq!(save(h2), bytes);
        evaluate(h2, c, NG_TYPE_F64);
        assert_eq!(read_f64(h2), 7.0);
        code(ng_graph_free(h2), NG_OK);
    }
    code(ng_graph_free(h), NG_OK);
}

#[test]
fn capi_rejects_null_and_bad_utf8() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    code(ng_graph_free(0), NG_ERR_NULL_HANDLE);
    // SAFETY: null cases must be rejected; all non-null allocations below are
    // valid, aligned, disjoint and live for their calls, except an oversized
    // length deliberately rejected before dereferencing its input.
    unsafe {
        code(ng_graph_create(null_mut()), NG_ERR_NULL_POINTER);
        let h = create();
        let a = add(h, b"math.constant");
        let b = add(h, b"math.constant");
        let c = add(h, b"math.add");
        let image = solid(h);
        let invert = add(h, b"image.invert");
        let divide = add(h, b"math.divide");
        let mut node = u64::MAX;
        let mut tag = -99;
        let mut value = -99.0;
        let mut n = usize::MAX;
        let (mut w, mut height) = (99, 99);
        let bad: &[u8] = &[0xff];
        let out: &[u8] = b"out";
        let p: &[u8] = b"value";
        let i: &[u8] = b"a";
        let ct: &[u8] = b"math.constant";
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_add_node(h, null(), 0, 0.0, 0.0, &mut node)
        });
        refused(h, NG_ERR_BAD_UTF8, || {
            ng_add_node(h, bad.as_ptr(), bad.len(), 0.0, 0.0, &mut node)
        });
        assert_eq!(node, u64::MAX);
        refused(h, NG_ERR_BAD_UTF8, || {
            ng_set_param_f64(h, a, bad.as_ptr(), bad.len(), 1.0)
        });
        refused(h, NG_ERR_BAD_UTF8, || {
            ng_connect(h, a, bad.as_ptr(), bad.len(), c, i.as_ptr(), i.len())
        });
        refused(h, NG_ERR_BAD_UTF8, || {
            ng_connect(h, a, out.as_ptr(), out.len(), c, bad.as_ptr(), bad.len())
        });
        refused(h, NG_ERR_BAD_UTF8, || {
            ng_evaluate(h, a, bad.as_ptr(), bad.len(), &mut tag)
        });
        assert_eq!(tag, -99);
        // Pointer validation precedes UTF-8 validation.
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_add_node(h, bad.as_ptr(), bad.len(), 0.0, 0.0, null_mut())
        });
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_connect(h, a, bad.as_ptr(), bad.len(), c, null(), 0)
        });
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_set_param_i64(h, a, null(), 0, 1)
        });
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_evaluate(h, a, out.as_ptr(), out.len(), null_mut())
        });
        refused(h, NG_ERR_NULL_POINTER, || ng_read_f64(h, null_mut()));
        refused(h, NG_ERR_NO_VALUE, || ng_read_f64(h, &mut value));
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_read_image(h, null_mut(), 0, &mut n, &mut w, &mut height)
        });
        assert_eq!(value, -99.0);
        let unknown: &[u8] = b"math.nope";
        refused(h, NG_ERR_GRAPH, || {
            ng_add_node(h, unknown.as_ptr(), unknown.len(), 0.0, 0.0, &mut node)
        });
        for name in [
            b"".as_slice(),
            b"C:\\bad",
            b"math.constant\r",
            b"math.constant\n",
        ] {
            refused(h, NG_ERR_GRAPH, || {
                ng_add_node(h, name.as_ptr(), name.len(), 0.0, 0.0, &mut node)
            });
        }
        for x in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            refused(h, NG_ERR_GRAPH, || {
                ng_add_node(h, ct.as_ptr(), ct.len(), x, 0.0, &mut node)
            });
        }
        refused(h, NG_ERR_GRAPH, || {
            ng_connect(h, a, out.as_ptr(), out.len(), invert, i.as_ptr(), i.len())
        });
        connect(h, a, c, b"a");
        refused(h, NG_ERR_GRAPH, || {
            ng_connect(h, b, out.as_ptr(), out.len(), c, i.as_ptr(), i.len())
        });
        connect(h, b, c, b"b");
        refused(h, NG_ERR_GRAPH, || {
            ng_connect(
                h,
                invert,
                out.as_ptr(),
                out.len(),
                invert,
                i.as_ptr(),
                i.len(),
            )
        });
        refused(h, NG_ERR_GRAPH, || {
            ng_set_param_i64(h, a, p.as_ptr(), p.len(), 1)
        });
        for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            refused(h, NG_ERR_GRAPH, || {
                ng_set_param_f64(h, a, p.as_ptr(), p.len(), number)
            });
        }
        // Exact pass-through, not an arithmetic reference: include subnormals
        // and both finite endpoints. No new floating-point arithmetic in ABI.
        for number in [
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            f64::MIN,
            f64::MAX,
            -0.0,
        ] {
            f64_param(h, a, number);
            evaluate(h, a, NG_TYPE_F64);
            assert_eq!(read_f64(h).to_bits(), number.to_bits());
        }
        f64_param(h, a, 3.0);
        evaluate(h, a, NG_TYPE_F64);
        connect(h, a, divide, b"a");
        connect(h, b, divide, b"b");
        let message = refused(h, NG_ERR_EVAL, || {
            ng_evaluate(h, divide, out.as_ptr(), out.len(), &mut tag)
        });
        assert!(message.contains("divide by zero"), "{message}");
        assert_eq!(tag, -99);
        assert_eq!(read_f64(h), 3.0);
        for bytes in [
            b"nope".as_slice(),
            b"schema=1\r\nkind=graph\nnodes\nedges\n",
            b"schema=1\nkind=graph\nnodes\nnodes\nedges\n",
        ] {
            refused(h, NG_ERR_FORMAT, || ng_load(h, bytes.as_ptr(), bytes.len()));
        }
        refused(h, NG_ERR_BAD_UTF8, || ng_load(h, bad.as_ptr(), bad.len()));
        refused(h, NG_ERR_NULL_POINTER, || ng_load(h, null(), 0));
        refused(h, NG_ERR_LIMIT, || ng_load(h, bad.as_ptr(), usize::MAX));
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_save(h, null_mut(), 0, null_mut())
        });
        let bytes = save(h);
        let mut short = vec![0xAA; bytes.len()];
        refused(h, NG_ERR_BUFFER_TOO_SMALL, || {
            ng_save(h, short.as_mut_ptr(), bytes.len() - 1, &mut n)
        });
        assert_eq!(n, bytes.len());
        assert!(short.iter().all(|&byte| byte == 0xAA));
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_save(h, null_mut(), bytes.len(), &mut n)
        });
        evaluate(h, image, NG_TYPE_IMAGE);
        refused(h, NG_ERR_BAD_VALUE, || ng_read_f64(h, &mut value));
        assert_eq!(value, -99.0);
        let mut pixels = vec![-99.0; 64];
        refused(h, NG_ERR_BUFFER_TOO_SMALL, || {
            ng_read_image(h, pixels.as_mut_ptr(), 63, &mut n, &mut w, &mut height)
        });
        assert_eq!((n, w, height), (64, 99, 99));
        assert_eq!(pixels, vec![-99.0; 64]);
        refused(h, NG_ERR_NULL_POINTER, || {
            ng_read_image(h, null_mut(), 64, &mut n, &mut w, &mut height)
        });
        let h2 = create();
        code(ng_graph_free(h2), NG_OK);
        refused(h, NG_ERR_STALE_HANDLE, || {
            ng_add_node(h2, null(), 0, 0.0, 0.0, &mut node)
        });
        n = usize::MAX;
        refused(h, NG_ERR_STALE_HANDLE, || {
            ng_save(h2, null_mut(), 0, &mut n)
        });
        assert_eq!(n, usize::MAX);
        assert_eq!(node, u64::MAX);
        code(ng_graph_free(h2), NG_ERR_STALE_HANDLE);
        code(ng_graph_free(u64::MAX), NG_ERR_STALE_HANDLE);
        let h3 = create();
        assert_ne!(h3, h2);
        refused(h3, NG_ERR_STALE_HANDLE, || {
            ng_add_node(h2, ct.as_ptr(), ct.len(), 0.0, 0.0, &mut node)
        });
        // Successful load clears a previously stored value.
        code(ng_load(h, bytes.as_ptr(), bytes.len()), NG_OK);
        refused(h, NG_ERR_NO_VALUE, || ng_read_f64(h, &mut value));
        code(ng_graph_free(h3), NG_OK);
        const ABI_HANDLE_LIMIT: usize = 4096; // Independent N10 contract fixture.
        let mut handles = Vec::new();
        for _ in 1..ABI_HANDLE_LIMIT {
            handles.push(create());
        }
        let mut blocked = u64::MAX;
        refused(h, NG_ERR_LIMIT, || ng_graph_create(&mut blocked));
        assert_eq!(blocked, u64::MAX);
        for handle in handles {
            code(ng_graph_free(handle), NG_OK);
        }
        code(ng_graph_free(h), NG_OK);
    }
}

#[test]
fn capi_error_does_not_unwind() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    code(ng_selftest_panic(), NG_ERR_PANIC);
    assert!(last_error().contains("nodegraph C ABI selftest panic"));
    let h = create();
    let one = add(h, b"math.constant");
    f64_param(h, one, 1.0);
    let mut previous = one;
    for _ in 0..20 {
        let sum = add(h, b"math.add");
        connect(h, previous, sum, b"a");
        connect(h, one, sum, b"b");
        previous = sum;
    }
    evaluate(h, previous, NG_TYPE_F64);
    assert_eq!(read_f64(h), 21.0);
    code(ng_graph_free(h), NG_OK);
}

fn spaces(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn c_type(rust: &str) -> &'static str {
    match rust {
        "u64" => "uint64_t",
        "i64" => "int64_t",
        "u32" => "uint32_t",
        "i32" => "int32_t",
        "f64" => "double",
        "f32" => "float",
        "usize" => "size_t",
        "*mut u64" => "uint64_t *",
        "*mut u32" => "uint32_t *",
        "*mut i32" => "int32_t *",
        "*mut f64" => "double *",
        "*mut f32" => "float *",
        "*mut usize" => "size_t *",
        "*mut u8" => "uint8_t *",
        "*const u8" => "const uint8_t *",
        other => panic!("unmapped Rust ABI type: {other}"),
    }
}

fn rust_exports(source: &str) -> Vec<(String, Vec<String>)> {
    let lines: Vec<_> = source.lines().collect();
    let mut exports = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some((_, signature)) = line.split_once("extern \"C\" fn ") else {
            continue;
        };
        assert!(index > 0);
        assert_eq!(lines[index - 1].trim(), "#[no_mangle]");
        assert!(line.starts_with("pub "), "non-public export");
        let (name, first) = signature.split_once('(').expect("parameter list");
        let rest = std::iter::once(first)
            .chain(lines[index + 1..].iter().copied())
            .collect::<Vec<_>>()
            .join(" ");
        let mut depth = 1;
        let close = rest
            .char_indices()
            .find_map(|(i, c)| {
                if c == '(' {
                    depth += 1;
                }
                if c == ')' {
                    depth -= 1;
                }
                (depth == 0).then_some(i)
            })
            .expect("unterminated signature");
        let result = rest[close + 1..]
            .trim()
            .strip_prefix("->")
            .expect("return type");
        assert_eq!(result.split('{').next().unwrap().trim(), "i32");
        let params = rest[..close]
            .split(',')
            .filter(|p| !p.trim().is_empty())
            .map(|p| {
                let (_, ty) = p.split_once(':').expect("named Rust parameter");
                c_type(&spaces(ty)).to_string()
            })
            .collect();
        exports.push((name.trim().to_string(), params));
    }
    exports
}

fn header_exports(header: &str) -> Vec<(String, Vec<String>)> {
    header
        .lines()
        .filter_map(|line| {
            let declaration = line.strip_prefix("int32_t ")?;
            let declaration = declaration.strip_suffix(");").expect("one-line prototype");
            let (name, params) = declaration.split_once('(').expect("C parameters");
            let params = if params == "void" {
                Vec::new()
            } else {
                params
                    .split(',')
                    .map(|p| {
                        let p = p.trim();
                        let start = p
                            .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                            .expect("named C parameter")
                            + 1;
                        assert!(!p[start..].is_empty());
                        spaces(p[..start].trim())
                    })
                    .collect()
            };
            Some((name.to_string(), params))
        })
        .collect()
}

fn rust_codes(source: &str) -> BTreeMap<String, i32> {
    let mut codes = BTreeMap::new();
    for line in source.lines() {
        if let Some(rest) = line.strip_prefix("pub const NG_") {
            let (name, rest) = rest.split_once(": i32 = ").expect("i32 constant");
            let value = rest
                .strip_suffix(';')
                .expect("constant terminator")
                .parse()
                .unwrap();
            assert_eq!(codes.insert(format!("NG_{name}"), value), None);
        }
    }
    codes
}

fn header_codes(header: &str) -> BTreeMap<String, i32> {
    let mut codes = BTreeMap::new();
    for line in header.lines() {
        if let Some(rest) = line.strip_prefix("#define NG_") {
            let (name, value) = rest.split_once(' ').expect("integer macro");
            let value = value.parse().unwrap();
            assert_eq!(codes.insert(format!("NG_{name}"), value), None);
        }
    }
    codes
}

#[test]
fn header_matches_capi_exports() {
    let header = include_str!("../include/nodegraph.h");
    let rust = include_str!("../src/capi.rs");
    let exports = rust_exports(rust);
    assert_eq!(exports.len(), 14);
    assert_eq!(exports, header_exports(header));
    assert_eq!(rust_codes(rust).len(), 19);
    assert_eq!(rust_codes(rust), header_codes(header));
    assert!(!rust.contains("#[repr(C)]"));
    assert!(!header.contains("struct"));
    // Behavioural negative controls: parsers must expose drift, not ignore it.
    let extra = "\n#[no_mangle]\npub extern \"C\" fn ng_debug_count() -> i32 { 0 }\n";
    assert_ne!(
        rust_exports(&format!("{rust}{extra}")),
        header_exports(header)
    );
    assert_ne!(
        exports,
        header_exports(&header.replace("double *out_value", "float *out_value"))
    );
}

fn header_version(header: &str) -> u32 {
    let versions: Vec<u32> = header
        .lines()
        .filter_map(|line| {
            line.strip_prefix("#define NODEGRAPH_ABI_VERSION ")
                .map(|v| v.parse().unwrap())
        })
        .collect();
    assert_eq!(versions.len(), 1);
    versions[0]
}

#[test]
fn abi_version_matches_header() {
    let header = include_str!("../include/nodegraph.h");
    let version = header_version(header);
    assert_eq!(version, NODEGRAPH_ABI_VERSION);
    let mut actual = 0;
    // SAFETY: aligned writable output.
    code(unsafe { ng_abi_version(&mut actual) }, NG_OK);
    assert_eq!(actual, version);
    assert_ne!(
        header_version(&header.replace("NODEGRAPH_ABI_VERSION 1", "NODEGRAPH_ABI_VERSION 2")),
        actual
    );
    // SAFETY: null is rejected, not dereferenced.
    code(unsafe { ng_abi_version(null_mut()) }, NG_ERR_NULL_POINTER);
}
