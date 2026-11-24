use nodegraph::capi::*;
use std::ptr::null_mut;
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
fn read_image_writes_out_len_before_a_null_dimension() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    let h = create();
    let img = solid(h);
    evaluate(h, img, NG_TYPE_IMAGE);

    let (mut n, mut height) = (usize::MAX, 99);
    // SAFETY: null outputs must be rejected; non-null outputs are aligned and writable.
    code(
        unsafe { ng_read_image(h, null_mut(), 0, &mut n, null_mut(), &mut height) },
        NG_ERR_NULL_POINTER,
    );
    assert_eq!(n, 64);
    assert_eq!(height, 99);

    let (mut n, mut w) = (usize::MAX, 99);
    // SAFETY: null outputs must be rejected; non-null outputs are aligned and writable.
    code(
        unsafe { ng_read_image(h, null_mut(), 0, &mut n, &mut w, null_mut()) },
        NG_ERR_NULL_POINTER,
    );
    assert_eq!(n, 64);
    assert_eq!(w, 99);

    let (mut n, mut w, mut height) = (usize::MAX, 99, 99);
    // SAFETY: size query with aligned, disjoint writable scalar outputs.
    code(
        unsafe { ng_read_image(h, null_mut(), 0, &mut n, &mut w, &mut height) },
        NG_ERR_BUFFER_TOO_SMALL,
    );
    assert_eq!(n, 64);
    assert_eq!((w, height), (99, 99));

    let h2 = create();
    let (mut n, mut height) = (usize::MAX, 99);
    // SAFETY: null outputs must be rejected; non-null outputs are aligned and writable.
    code(
        unsafe { ng_read_image(h2, null_mut(), 0, &mut n, null_mut(), &mut height) },
        NG_ERR_NULL_POINTER,
    );
    assert_eq!(n, usize::MAX);
    assert_eq!(height, 99);

    code(ng_graph_free(h), NG_OK);
    code(ng_graph_free(h2), NG_OK);
}
