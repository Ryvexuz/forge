//! Reserved for the C interface boundary.
//!
//! This module implements that boundary.
//! Handles index a process-wide, generation-checked table, never Rust pointers.
//! Callers keep handles thread-bound; the built-in registry and last error are
//! thread-local. Every export uses `guard`: zero means success, negative codes
//! name failures. Unsafe pointer access is confined to this file.
//! Text lengths above isize::MAX are rejected before dereferencing the input.
//! No Rust panic unwinds across this boundary (requires panic=unwind).
//! A foreign unwind into Rust is out of scope.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard};

use crate::eval::Evaluator;
use crate::format;
use crate::graph::{Graph, NodeId, Position};
use crate::kind::KindRegistry;
use crate::kinds::register_all;
use crate::value::{Value, ValueType};

pub const NODEGRAPH_ABI_VERSION: u32 = 1;
pub const NG_OK: i32 = 0;
pub const NG_ERR_NULL_HANDLE: i32 = -1;
pub const NG_ERR_STALE_HANDLE: i32 = -2;
pub const NG_ERR_NULL_POINTER: i32 = -3;
pub const NG_ERR_BAD_UTF8: i32 = -4;
pub const NG_ERR_BUFFER_TOO_SMALL: i32 = -5;
pub const NG_ERR_GRAPH: i32 = -6;
pub const NG_ERR_EVAL: i32 = -7;
pub const NG_ERR_FORMAT: i32 = -8;
pub const NG_ERR_BAD_VALUE: i32 = -9;
pub const NG_ERR_NO_VALUE: i32 = -10;
pub const NG_ERR_PANIC: i32 = -11;
pub const NG_ERR_POISONED: i32 = -12;
pub const NG_ERR_LIMIT: i32 = -13;
pub const NG_TYPE_BOOL: i32 = 1;
pub const NG_TYPE_INT: i32 = 2;
pub const NG_TYPE_F64: i32 = 3;
pub const NG_TYPE_TEXT: i32 = 4;
pub const NG_TYPE_IMAGE: i32 = 5;

// ABI resource bound specified by N10; not a graph or image dimension limit.
const MAX_HANDLES: usize = 4096;

struct State {
    graph: Graph,
    evaluator: Evaluator,
    value: Option<Value>,
}

struct Slot {
    // Zero denotes a retired slot after generation exhaustion.
    generation: u32,
    state: Option<State>,
}

struct Table {
    slots: Vec<Slot>,
}

static TABLE: Mutex<Table> = Mutex::new(Table { slots: Vec::new() });

thread_local! {
    static REGISTRY: Result<KindRegistry, Fail> = {
        let mut registry = KindRegistry::new();
        register_all(&mut registry)
            .map(|()| registry)
            .map_err(|e| fail(NG_ERR_GRAPH, e))
    };
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

#[derive(Clone)]
struct Fail {
    code: i32,
    message: String,
}

fn fail(code: i32, message: impl std::fmt::Display) -> Fail {
    Fail {
        code,
        message: message.to_string(),
    }
}

fn guard(f: impl FnOnce() -> Result<i32, Fail>) -> i32 {
    let result = catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let message = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("non-text panic");
        Err(fail(NG_ERR_PANIC, message))
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            LAST_ERROR.with(|last| *last.borrow_mut() = error.message);
            error.code
        }
    }
}

fn table() -> Result<MutexGuard<'static, Table>, Fail> {
    TABLE
        .lock()
        .map_err(|_| fail(NG_ERR_POISONED, "handle table is poisoned"))
}

impl Table {
    fn slot(&mut self, handle: u64) -> Result<&mut Slot, Fail> {
        if handle == 0 {
            return Err(fail(NG_ERR_NULL_HANDLE, "null handle"));
        }
        let low = handle as u32;
        let generation = (handle >> 32) as u32;
        let slot = low
            .checked_sub(1)
            .and_then(|index| self.slots.get_mut(index as usize))
            .filter(|s| s.generation == generation && s.state.is_some());
        slot.ok_or_else(|| fail(NG_ERR_STALE_HANDLE, "stale or forged handle"))
    }
}

fn with_state(handle: u64, f: impl FnOnce(&mut State) -> Result<i32, Fail>) -> Result<i32, Fail> {
    let mut table = table()?;
    let slot = table.slot(handle)?;
    f(slot.state.as_mut().expect("validated live slot"))
}

fn registry<T>(f: impl FnOnce(&KindRegistry) -> Result<T, Fail>) -> Result<T, Fail> {
    REGISTRY.with(|r| match r {
        Ok(r) => f(r),
        Err(e) => Err(e.clone()),
    })
}

fn pointer<T>(p: *const T) -> Result<(), Fail> {
    if p.is_null() {
        Err(fail(NG_ERR_NULL_POINTER, "null pointer"))
    } else {
        Ok(())
    }
}

// Caller guarantees a readable, non-null allocation of len bytes.
unsafe fn text<'a>(p: *const u8, len: usize) -> Result<&'a str, Fail> {
    if len > isize::MAX as usize {
        return Err(fail(NG_ERR_LIMIT, "text length exceeds slice range"));
    }
    // SAFETY: callers check null first and promise len readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(p, len) };
    std::str::from_utf8(bytes).map_err(|e| fail(NG_ERR_BAD_UTF8, e))
}

// All buffers share the size-query rule. No destination reference is formed.
unsafe fn buffer<T: Copy>(
    data: &[T],
    buf: *mut T,
    cap: usize,
    out_len: *mut usize,
) -> Result<(), Fail> {
    pointer(out_len)?;
    // SAFETY: caller promises an aligned, writable out_len, checked non-null.
    unsafe { out_len.write(data.len()) };
    if cap < data.len() {
        return Err(fail(NG_ERR_BUFFER_TOO_SMALL, "buffer too small"));
    }
    pointer(buf)?;
    // SAFETY: caller promises writable capacity and no overlap with source.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len()) };
    Ok(())
}

/// Report the ABI version.
/// # Safety
/// Non-null output pointers must be aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn ng_abi_version(out_version: *mut u32) -> i32 {
    guard(|| {
        pointer(out_version)?;
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_version.write(NODEGRAPH_ABI_VERSION) };
        Ok(NG_OK)
    })
}

/// Create a graph with no nodes.
/// # Safety
/// Non-null out_handle must be aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn ng_graph_create(out_handle: *mut u64) -> i32 {
    guard(|| {
        pointer(out_handle)?;
        let mut table = table()?;
        let index = match table
            .slots
            .iter()
            .position(|s| s.state.is_none() && s.generation != 0)
        {
            Some(index) => index,
            None => {
                if table.slots.len() >= MAX_HANDLES {
                    return Err(fail(NG_ERR_LIMIT, "handle table full"));
                }
                table.slots.push(Slot {
                    generation: 1,
                    state: None,
                });
                table.slots.len() - 1
            }
        };
        let slot = &mut table.slots[index];
        slot.state = Some(State {
            graph: Graph::new(),
            evaluator: Evaluator::new(),
            value: None,
        });
        let handle = (u64::from(slot.generation) << 32) | (index as u64 + 1);
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_handle.write(handle) };
        Ok(NG_OK)
    })
}

/// Free a live table entry, invalidating all copies of its handle.
#[no_mangle]
pub extern "C" fn ng_graph_free(handle: u64) -> i32 {
    guard(|| {
        let mut table = table()?;
        let slot = table.slot(handle)?;
        slot.state = None;
        slot.generation = slot.generation.checked_add(1).unwrap_or(0);
        Ok(NG_OK)
    })
}

/// Add a built-in type with default parameters.
/// # Safety
/// Non-null text pointers must cover their lengths. Outputs must be aligned,
/// writable and disjoint from inputs.
#[no_mangle]
pub unsafe extern "C" fn ng_add_node(
    handle: u64,
    type_name: *const u8,
    type_len: usize,
    x: f32,
    y: f32,
    out_node: *mut u64,
) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(type_name)?;
            pointer(out_node)?;
            // SAFETY: null checked; caller guarantees readable input.
            let name = unsafe { text(type_name, type_len)? };
            let node = registry(|r| {
                let kind = r.lookup(name).map_err(|e| fail(NG_ERR_GRAPH, e))?;
                let params = kind
                    .params()
                    .iter()
                    .map(|p| match p.value_type {
                        ValueType::Bool => Ok(Value::Bool(false)),
                        ValueType::Int => Ok(Value::Int(0)),
                        ValueType::F64 => Ok(Value::F64(0.0)),
                        ValueType::Text => Ok(Value::Text(String::new())),
                        _ => Err(fail(NG_ERR_BAD_VALUE, "parameter has no default")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                s.graph
                    .add_node(r, name, params, Position { x, y })
                    .map_err(|e| fail(NG_ERR_GRAPH, e))
            })?;
            // SAFETY: checked non-null; caller guarantees writable output.
            unsafe { out_node.write(node.0) };
            Ok(NG_OK)
        })
    })
}

// Both parameter exports use the same validation and graph-edit path.
unsafe fn set_param(
    handle: u64,
    node: u64,
    name: *const u8,
    name_len: usize,
    value: Value,
) -> Result<i32, Fail> {
    with_state(handle, |s| {
        pointer(name)?;
        // SAFETY: null checked; caller guarantees readable name_len bytes.
        let name = unsafe { text(name, name_len)? };
        s.graph
            .set_param(NodeId(node), name, value)
            .map_err(|e| fail(NG_ERR_GRAPH, e))?;
        Ok(NG_OK)
    })
}

/// Set an F64 parameter.
/// # Safety
/// A non-null name must cover name_len readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ng_set_param_f64(
    handle: u64,
    node: u64,
    name: *const u8,
    name_len: usize,
    value: f64,
) -> i32 {
    // SAFETY: the caller supplies the readable text allocation.
    guard(|| unsafe { set_param(handle, node, name, name_len, Value::F64(value)) })
}

/// Set an Int parameter.
/// # Safety
/// A non-null name must cover name_len readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ng_set_param_i64(
    handle: u64,
    node: u64,
    name: *const u8,
    name_len: usize,
    value: i64,
) -> i32 {
    // SAFETY: the caller supplies the readable text allocation.
    guard(|| unsafe { set_param(handle, node, name, name_len, Value::Int(value)) })
}

/// Connect two named ports.
/// # Safety
/// Non-null port pointers must cover their respective readable byte lengths.
#[no_mangle]
pub unsafe extern "C" fn ng_connect(
    handle: u64,
    from: u64,
    from_port: *const u8,
    from_len: usize,
    to: u64,
    to_port: *const u8,
    to_len: usize,
) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(from_port)?;
            pointer(to_port)?;
            // SAFETY: both inputs checked non-null; caller supplies lengths.
            let (from_port, to_port) =
                unsafe { (text(from_port, from_len)?, text(to_port, to_len)?) };
            s.graph
                .connect(NodeId(from), from_port, NodeId(to), to_port)
                .map_err(|e| fail(NG_ERR_GRAPH, e))?;
            Ok(NG_OK)
        })
    })
}

/// Evaluate an output, retaining the last successful value.
/// # Safety
/// Non-null port covers port_len readable bytes; out_type is aligned, writable
/// and disjoint from that input.
#[no_mangle]
pub unsafe extern "C" fn ng_evaluate(
    handle: u64,
    node: u64,
    port: *const u8,
    port_len: usize,
    out_type: *mut i32,
) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(port)?;
            pointer(out_type)?;
            // SAFETY: null checked; caller guarantees readable input.
            let port = unsafe { text(port, port_len)? };
            let value = registry(|r| {
                s.evaluator
                    .evaluate(&s.graph, r, (NodeId(node), port))
                    .map_err(|e| fail(NG_ERR_EVAL, e))
            })?;
            let code = match &value {
                Value::Bool(_) => NG_TYPE_BOOL,
                Value::Int(_) => NG_TYPE_INT,
                Value::F64(_) => NG_TYPE_F64,
                Value::Text(_) => NG_TYPE_TEXT,
                Value::Image(_) => NG_TYPE_IMAGE,
            };
            s.value = Some(value);
            // SAFETY: checked non-null; caller guarantees writable output.
            unsafe { out_type.write(code) };
            Ok(NG_OK)
        })
    })
}

/// Read the last successful F64 value.
/// # Safety
/// Non-null out_value must be aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn ng_read_f64(handle: u64, out_value: *mut f64) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(out_value)?;
            match s.value.as_ref() {
                Some(Value::F64(value)) => {
                    // SAFETY: checked non-null; caller guarantees writable output.
                    unsafe { out_value.write(*value) };
                    Ok(NG_OK)
                }
                None => Err(fail(NG_ERR_NO_VALUE, "no evaluated value")),
                _ => Err(fail(NG_ERR_BAD_VALUE, "stored value is not F64")),
            }
        })
    })
}

/// Copy the last image's pixels, with no partial copy.
/// # Safety
/// Scalar outputs must be aligned, writable and mutually disjoint. If cap is
/// sufficient, buf must cover cap writable f32s disjoint from those outputs.
#[no_mangle]
pub unsafe extern "C" fn ng_read_image(
    handle: u64,
    buf: *mut f32,
    cap: usize,
    out_len: *mut usize,
    out_width: *mut u32,
    out_height: *mut u32,
) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(out_len)?;
            pointer(out_width)?;
            pointer(out_height)?;
            if !matches!(s.value.as_ref(), Some(Value::Image(_))) {
                pointer(buf)?;
            }
            let image = match s.value.as_ref() {
                Some(Value::Image(image)) => image,
                None => return Err(fail(NG_ERR_NO_VALUE, "no evaluated value")),
                _ => return Err(fail(NG_ERR_BAD_VALUE, "stored value is not Image")),
            };
            // SAFETY: checked scalar pointers; caller guarantees buffer capacity.
            unsafe { buffer(&image.pixels, buf, cap, out_len)? };
            // SAFETY: checked non-null, aligned and disjoint writable outputs.
            unsafe {
                out_width.write(image.width);
                out_height.write(image.height);
            }
            Ok(NG_OK)
        })
    })
}

/// Save canonical UTF-8 bytes without a NUL terminator.
/// # Safety
/// out_len is aligned and writable. If cap is sufficient, buf covers cap
/// writable bytes disjoint from out_len.
#[no_mangle]
pub unsafe extern "C" fn ng_save(
    handle: u64,
    buf: *mut u8,
    cap: usize,
    out_len: *mut usize,
) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(out_len)?;
            let text = format::write(&s.graph).map_err(|e| fail(NG_ERR_FORMAT, e))?;
            // SAFETY: checked out_len; caller guarantees buffer capacity.
            unsafe { buffer(text.as_bytes(), buf, cap, out_len)? };
            Ok(NG_OK)
        })
    })
}

/// Atomically replace a graph from canonical UTF-8, clearing evaluator/value.
/// # Safety
/// For len <= isize::MAX, non-null buf must cover len readable bytes.
#[no_mangle]
pub unsafe extern "C" fn ng_load(handle: u64, buf: *const u8, len: usize) -> i32 {
    guard(|| {
        with_state(handle, |s| {
            pointer(buf)?;
            // SAFETY: null checked; caller guarantees readable input.
            let text = unsafe { text(buf, len)? };
            let graph = registry(|r| format::parse(text, r).map_err(|e| fail(NG_ERR_FORMAT, e)))?;
            s.graph = graph;
            s.evaluator = Evaluator::new();
            s.value = None;
            Ok(NG_OK)
        })
    })
}

/// Read this thread's last diagnostic without a NUL. Size queries preserve it.
/// # Safety
/// out_len is aligned and writable. If cap is sufficient, buf covers cap
/// writable bytes disjoint from out_len.
#[no_mangle]
pub unsafe extern "C" fn ng_last_error(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    guard(|| {
        pointer(out_len)?;
        let message = LAST_ERROR.with(|last| last.borrow().clone());
        // SAFETY: checked out_len; caller guarantees buffer capacity.
        unsafe { buffer(message.as_bytes(), buf, cap, out_len) }.map_err(|e| {
            if e.code == NG_ERR_BUFFER_TOO_SMALL {
                fail(e.code, &message)
            } else {
                e
            }
        })?;
        Ok(NG_OK)
    })
}

/// Exercise panic containment without taking or poisoning the table lock.
#[no_mangle]
pub extern "C" fn ng_selftest_panic() -> i32 {
    guard(|| panic!("nodegraph C ABI selftest panic"))
}
