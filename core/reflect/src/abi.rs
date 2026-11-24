//! Reflection ABI module.
//!
//! Targeted formatting skips keep export signatures on one line for the
//! hand-written header verifier.

use crate::change::{Change, ChangeSink};
use crate::desc::{FieldDesc, Kind, Reflect, TypeDesc};
use crate::limits::{Limits, LimitsError};
use crate::path::{self, Path, Segment};
use crate::registry::{CreateError, Registry};
use crate::schema::{hash_hex, schema_hash};
use crate::text::{load_text, save_text};
use crate::value::Value;
use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub const REFLECT_ABI_VERSION: u32 = 1;
pub const REFLECT_OK: i32 = 0;
pub const REFLECT_ERR_NULL_HANDLE: i32 = -1;
pub const REFLECT_ERR_STALE_HANDLE: i32 = -2;
pub const REFLECT_ERR_NULL_POINTER: i32 = -3;
pub const REFLECT_ERR_BAD_UTF8: i32 = -4;
pub const REFLECT_ERR_BUFFER_TOO_SMALL: i32 = -5;
pub const REFLECT_ERR_NO_TYPE: i32 = -6;
pub const REFLECT_ERR_INDEX: i32 = -7;
pub const REFLECT_ERR_PATH: i32 = -8;
pub const REFLECT_ERR_REFUSED: i32 = -9;
pub const REFLECT_ERR_VALUE: i32 = -10;
pub const REFLECT_ERR_PANIC: i32 = -11;
pub const REFLECT_ERR_POISONED: i32 = -12;
pub const REFLECT_ERR_LIMIT: i32 = -13;
pub const REFLECT_ERR_INTERNAL: i32 = -14;

// Fixed ABI live-object bound, not a configurable reflection data limit.
const MAX_HANDLES: usize = 4096;

#[derive(Clone)]
struct Fail {
    code: i32,
    message: Option<String>,
}

fn fail(code: i32, message: impl std::fmt::Display) -> Fail {
    Fail {
        code,
        message: Some(message.to_string()),
    }
}

thread_local! {
    static LAST_ERROR: RefCell<String> = const { RefCell::new(String::new()) };
}

fn guard(f: impl FnOnce() -> Result<(), Fail>) -> i32 {
    let result = match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let message = match payload.downcast_ref::<&str>() {
                Some(message) => *message,
                None => match payload.downcast_ref::<String>() {
                    Some(message) => message.as_str(),
                    None => "non-text panic",
                },
            };
            Err(fail(REFLECT_ERR_PANIC, message))
        }
    };
    match result {
        Ok(()) => REFLECT_OK,
        Err(error) => {
            if let Some(message) = error.message {
                LAST_ERROR.with(|last| *last.borrow_mut() = message);
            }
            error.code
        }
    }
}

fn registry() -> Result<&'static Registry, Fail> {
    static REGISTRY: OnceLock<Result<Registry, Fail>> = OnceLock::new();
    REGISTRY
        .get_or_init(|| {
            let mut registry = Registry::new();
            crate::sample::register_samples(&mut registry)
                .map_err(|error| fail(REFLECT_ERR_INTERNAL, error))?;
            Ok(registry)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn limits() -> Result<&'static Limits, Fail> {
    static LIMITS: OnceLock<Result<Limits, LimitsError>> = OnceLock::new();
    LIMITS
        .get_or_init(|| Limits::parse(include_str!("../reflect.toml")))
        .as_ref()
        .map_err(|error| fail(REFLECT_ERR_INTERNAL, error))
}

struct Slot {
    // Zero generation retires a slot permanently.
    generation: u32,
    object: Option<Box<dyn Reflect + Send>>,
}

struct Table {
    slots: Vec<Slot>,
}

static TABLE: Mutex<Table> = Mutex::new(Table { slots: Vec::new() });

fn table() -> Result<MutexGuard<'static, Table>, Fail> {
    TABLE
        .lock()
        .map_err(|_| fail(REFLECT_ERR_POISONED, "handle table is poisoned"))
}

impl Table {
    fn slot(&mut self, handle: u64) -> Result<&mut Slot, Fail> {
        if handle == 0 {
            return Err(fail(REFLECT_ERR_NULL_HANDLE, "null handle"));
        }
        let generation = (handle >> 32) as u32;
        (handle as u32)
            .checked_sub(1)
            .and_then(|index| self.slots.get_mut(index as usize))
            .filter(|slot| slot.generation == generation && slot.object.is_some())
            .ok_or_else(|| fail(REFLECT_ERR_STALE_HANDLE, "stale or forged handle"))
    }

    fn insert(&mut self, object: Box<dyn Reflect + Send>) -> Result<u64, Fail> {
        if self.slots.iter().filter(|slot| slot.object.is_some()).count() >= MAX_HANDLES {
            return Err(fail(REFLECT_ERR_LIMIT, "handle table full"));
        }
        let index = match self
            .slots
            .iter()
            .position(|slot| slot.object.is_none() && slot.generation != 0)
        {
            Some(index) => index,
            None => {
                if self.slots.len() >= u32::MAX as usize {
                    return Err(fail(REFLECT_ERR_LIMIT, "handle index exhausted"));
                }
                self.slots.push(Slot {
                    generation: 1,
                    object: None,
                });
                self.slots.len() - 1
            }
        };
        let slot = self
            .slots
            .get_mut(index)
            .ok_or_else(|| fail(REFLECT_ERR_INTERNAL, "missing allocated slot"))?;
        slot.object = Some(object);
        Ok((u64::from(slot.generation) << 32) | (index as u64 + 1))
    }
}

fn with_object(handle: u64, f: impl FnOnce(&mut dyn Reflect) -> Result<(), Fail>) -> Result<(), Fail> {
    let mut table = table()?;
    let object = table
        .slot(handle)?
        .object
        .as_mut()
        .ok_or_else(|| fail(REFLECT_ERR_INTERNAL, "missing live object"))?;
    f(object.as_mut())
}

fn pointer<T>(pointer: *const T) -> Result<(), Fail> {
    if pointer.is_null() {
        Err(fail(REFLECT_ERR_NULL_POINTER, "null pointer"))
    } else {
        Ok(())
    }
}

unsafe fn text<'a>(input: *const u8, len: usize) -> Result<&'a str, Fail> {
    pointer(input)?;
    if len > isize::MAX as usize {
        return Err(fail(REFLECT_ERR_LIMIT, "text length exceeds slice range"));
    }
    // SAFETY: null and slice range checked; caller guarantees readable bytes.
    let bytes = unsafe { std::slice::from_raw_parts(input, len) };
    std::str::from_utf8(bytes).map_err(|error| fail(REFLECT_ERR_BAD_UTF8, error))
}

unsafe fn buffer(data: &[u8], buf: *mut u8, cap: usize, out_len: *mut usize) -> Result<(), Fail> {
    pointer(out_len)?;
    // SAFETY: checked non-null; caller provides aligned, writable length.
    unsafe { out_len.write(data.len()) };
    if cap < data.len() {
        return Err(fail(REFLECT_ERR_BUFFER_TOO_SMALL, "buffer too small"));
    }
    pointer(buf)?;
    // SAFETY: caller provides sufficient writable capacity, disjoint from data.
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), buf, data.len()) };
    Ok(())
}

fn type_desc(name: &str) -> Result<&'static TypeDesc, Fail> {
    let registry = registry()?;
    registry
        .find(name)
        .and_then(|index| registry.desc(index))
        .ok_or_else(|| fail(REFLECT_ERR_NO_TYPE, format!("no type {name}")))
}

fn target_kind(object: &dyn Reflect, path: &Path) -> Result<Kind, Fail> {
    let mut kind = Kind::Struct(Box::new(object.dyn_desc()));
    for segment in path.segments() {
        kind = match (segment, &kind) {
            (Segment::Field(name), Kind::Struct(desc)) => desc
                .field_index(name)
                .and_then(|index| desc.field(index))
                .map(|field| field.kind.clone()),
            (Segment::Index(_), Kind::List(item) | Kind::Array(item, _)) => Some(*item.clone()),
            _ => None,
        }
        .ok_or_else(|| fail(REFLECT_ERR_INTERNAL, "successful path has no target kind"))?;
    }
    Ok(kind)
}

fn wrapper(kind: Kind) -> Result<TypeDesc, Fail> {
    TypeDesc::new("Value", 1, vec![FieldDesc::new("v", kind)])
        .map_err(|error| fail(REFLECT_ERR_INTERNAL, error))
}

fn save_token(kind: Kind, value: Value, limits: &Limits) -> Result<String, Fail> {
    let desc = wrapper(kind)?;
    let text = save_text(&desc, &Value::Struct(vec![value]), limits)
        .map_err(|error| fail(REFLECT_ERR_VALUE, error))?;
    text.lines()
        .nth(2)
        .and_then(|line| line.strip_prefix("v = "))
        .map(str::to_owned)
        .ok_or_else(|| fail(REFLECT_ERR_INTERNAL, "missing value token"))
}

fn load_token(kind: Kind, token: &str, limits: &Limits) -> Result<Value, Fail> {
    if token.is_empty() || token.contains(['\n', '\r']) {
        return Err(fail(REFLECT_ERR_VALUE, "empty or multiline value token"));
    }
    // Reject oversized tokens before allocating the wrapper document.
    if token.len() as u64 > limits.max_text_file_bytes {
        return Err(fail(REFLECT_ERR_VALUE, "value token exceeds text file limit"));
    }
    let desc = wrapper(kind)?;
    let hash = hash_hex(schema_hash(&desc).map_err(|error| fail(REFLECT_ERR_INTERNAL, error))?);
    let text = format!("reflect value 1\ntype Value version=1 hash={hash}\nv = {token}\nend\n");
    let loaded = load_text(&desc, &text, limits).map_err(|error| fail(REFLECT_ERR_VALUE, error))?;
    match loaded.value {
        Value::Struct(mut fields) if fields.len() == 1 => fields
            .pop()
            .ok_or_else(|| fail(REFLECT_ERR_INTERNAL, "missing loaded value")),
        _ => Err(fail(REFLECT_ERR_INTERNAL, "invalid loaded wrapper")),
    }
}

struct DropChanges;

impl ChangeSink for DropChanges {
    fn changed(&mut self, _change: &Change) {}
}

/// # Safety
/// Non-null out_version must be aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn reflect_abi_version(out_version: *mut u32) -> i32 {
    guard(|| {
        pointer(out_version)?;
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_version.write(REFLECT_ABI_VERSION) };
        Ok(())
    })
}

/// # Safety
/// Non-null out_count must be aligned and writable.
#[no_mangle]
pub unsafe extern "C" fn reflect_type_count(out_count: *mut usize) -> i32 {
    guard(|| {
        pointer(out_count)?;
        let count = registry()?.len();
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_count.write(count) };
        Ok(())
    })
}

/// # Safety
/// out_len must be aligned and writable. Sufficient buf must be writable
/// for cap bytes and disjoint from out_len.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_type_name(index: usize, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    guard(|| {
        let desc = registry()?.desc(index).ok_or_else(|| fail(REFLECT_ERR_INDEX, "type index out of range"))?;
        // SAFETY: caller guarantees writable, disjoint outputs.
        unsafe { buffer(desc.name().as_bytes(), buf, cap, out_len) }
    })
}

/// # Safety
/// Non-null type_name covers type_len readable bytes. Non-null out_count
/// must be aligned, writable and disjoint from the input.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_field_count(type_name: *const u8, type_len: usize, out_count: *mut usize) -> i32 {
    guard(|| {
        pointer(out_count)?;
        // SAFETY: caller guarantees readable input; text validates null and length.
        let name = unsafe { text(type_name, type_len)? };
        let count = type_desc(name)?.fields().len();
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_count.write(count) };
        Ok(())
    })
}

/// # Safety
/// Non-null type_name covers type_len readable bytes. out_len is aligned
/// and writable; sufficient buf covers cap writable bytes. All are disjoint.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_field_name(type_name: *const u8, type_len: usize, index: usize, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    guard(|| {
        // SAFETY: caller guarantees readable input; text validates null and length.
        let name = unsafe { text(type_name, type_len)? };
        let field = type_desc(name)?.field(index).ok_or_else(|| fail(REFLECT_ERR_INDEX, "field index out of range"))?;
        // SAFETY: caller guarantees writable, disjoint outputs.
        unsafe { buffer(field.name.as_bytes(), buf, cap, out_len) }
    })
}

/// # Safety
/// Non-null type_name covers type_len readable bytes. Non-null out_handle
/// must be aligned, writable and disjoint from the input.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_create(type_name: *const u8, type_len: usize, out_handle: *mut u64) -> i32 {
    guard(|| {
        pointer(out_handle)?;
        // SAFETY: caller guarantees readable input; text validates null and length.
        let name = unsafe { text(type_name, type_len)? };
        let object = registry()?.create(name).map_err(|error| {
            let code = match &error {
                CreateError::NoType { .. } => REFLECT_ERR_NO_TYPE,
                CreateError::Build { .. } => REFLECT_ERR_INTERNAL,
            };
            fail(code, error)
        })?;
        let handle = table()?.insert(object)?;
        // SAFETY: checked non-null; caller guarantees writable output.
        unsafe { out_handle.write(handle) };
        Ok(())
    })
}

/// # Safety
/// Callers must obey the single-threaded, thread-bound handle contract.
#[no_mangle]
pub unsafe extern "C" fn reflect_destroy(handle: u64) -> i32 {
    guard(|| {
        let mut table = table()?;
        let slot = table.slot(handle)?;
        slot.object = None;
        slot.generation = if slot.generation == u32::MAX {
            0
        } else {
            slot.generation + 1
        };
        Ok(())
    })
}

/// # Safety
/// Non-null path covers path_len readable bytes. out_len is aligned and
/// writable; sufficient buf covers cap writable bytes. All are disjoint.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_get(handle: u64, path: *const u8, path_len: usize, buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    guard(|| {
        with_object(handle, |object| {
            // SAFETY: caller guarantees readable input; text validates null and length.
            let text = unsafe { text(path, path_len)? };
            let limits = limits()?;
            let path = path::parse_path(text, limits).map_err(|error| fail(REFLECT_ERR_PATH, error))?;
            let value = path::get_path(object, &path).map_err(|error| fail(REFLECT_ERR_REFUSED, error))?;
            let token = save_token(target_kind(object, &path)?, value, limits)?;
            // SAFETY: caller guarantees writable, disjoint outputs.
            unsafe { buffer(token.as_bytes(), buf, cap, out_len) }
        })
    })
}

/// # Safety
/// Non-null path and value cover their respective readable byte lengths.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_set(handle: u64, path: *const u8, path_len: usize, value: *const u8, value_len: usize) -> i32 {
    guard(|| {
        with_object(handle, |object| {
            // SAFETY: caller guarantees readable input; text validates null and length.
            let path_text = unsafe { text(path, path_len)? };
            let limits = limits()?;
            let path = path::parse_path(path_text, limits).map_err(|error| fail(REFLECT_ERR_PATH, error))?;
            path::get_path(object, &path).map_err(|error| fail(REFLECT_ERR_REFUSED, error))?;
            let kind = target_kind(object, &path)?;
            // SAFETY: caller guarantees readable input; text validates null and length.
            let token = unsafe { text(value, value_len)? };
            let value = load_token(kind, token, limits)?;
            path::set_path(object, &path, value, limits, &mut DropChanges)
                .map_err(|error| fail(REFLECT_ERR_REFUSED, error))
        })
    })
}

/// Reads the diagnostic without changing it, including on buffer errors.
/// # Safety
/// out_len is aligned and writable; sufficient buf covers cap writable
/// bytes disjoint from out_len.
#[rustfmt::skip]
#[no_mangle]
pub unsafe extern "C" fn reflect_last_error(buf: *mut u8, cap: usize, out_len: *mut usize) -> i32 {
    guard(|| {
        let message = LAST_ERROR.with(|last| last.borrow().clone());
        // SAFETY: caller guarantees writable, disjoint outputs.
        unsafe { buffer(message.as_bytes(), buf, cap, out_len) }
            .map_err(|error| Fail { code: error.code, message: None })
    })
}

/// Exercises containment without taking the handle table lock.
/// # Safety
/// Callers must obey the single-threaded ABI contract.
#[no_mangle]
pub unsafe extern "C" fn reflect_selftest_panic() -> i32 {
    guard(|| panic!("reflect selftest panic"))
}
