use reflect::abi::*;
use reflect::desc::{Kind, Reflect};
use reflect::macros::Field;
use reflect::registry::{zero_value, CreateError, Registry, RegistryError};
use reflect::sample::{register_samples, DriveMode, Vehicle};
use reflect::value::Value;
use std::ptr::{null, null_mut};
use std::sync::Mutex;

static ABI_TESTS: Mutex<()> = Mutex::new(());

fn code(actual: i32, expected: i32) {
    assert_eq!(actual, expected, "ABI status");
}

fn bytes(mut call: impl FnMut(*mut u8, usize, *mut usize) -> i32) -> String {
    let mut len = usize::MAX;
    code(call(null_mut(), 0, &mut len), REFLECT_ERR_BUFFER_TOO_SMALL);
    assert!(len > 0 && len < isize::MAX as usize, "answer length {len}");
    let mut data = vec![0xAA; len];
    code(call(data.as_mut_ptr(), data.len(), &mut len), REFLECT_OK);
    assert_eq!(len, data.len());
    String::from_utf8(data).expect("UTF-8 output")
}

fn create() -> u64 {
    let mut handle = 0;
    // SAFETY: readable type name and disjoint, aligned writable output.
    code(
        unsafe { reflect_create(b"Vehicle".as_ptr(), 7, &mut handle) },
        REFLECT_OK,
    );
    assert_ne!(handle, 0);
    handle
}

fn get(handle: u64, path: &str) -> String {
    // SAFETY: readable path; helper supplies valid, disjoint outputs.
    bytes(|buf, cap, len| unsafe {
        reflect_get(handle, path.as_ptr(), path.len(), buf, cap, len)
    })
}

fn set(handle: u64, path: &str, token: &str) -> i32 {
    // SAFETY: both slices are readable for their lengths.
    unsafe { reflect_set(handle, path.as_ptr(), path.len(), token.as_ptr(), token.len()) }
}

fn last_error() -> String {
    // SAFETY: helper supplies valid, disjoint outputs.
    bytes(|buf, cap, len| unsafe { reflect_last_error(buf, cap, len) })
}

fn refused_get(handle: u64, path: &str, expected: i32) {
    let mut len = usize::MAX;
    // SAFETY: readable path; error must precede use of null output buffer.
    code(
        unsafe { reflect_get(handle, path.as_ptr(), path.len(), null_mut(), 0, &mut len) },
        expected,
    );
    assert_eq!(len, usize::MAX, "failed get must not touch length");
}

fn destroy(handle: u64) {
    // SAFETY: test observes the single-threaded handle contract.
    code(unsafe { reflect_destroy(handle) }, REFLECT_OK);
}

#[test]
fn abi_version_is_1() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    // A fresh thread has an empty diagnostic even if a test worker was reused.
    // The process-wide test lock keeps all ABI calls serialized.
    std::thread::spawn(|| {
        let mut len = usize::MAX;
        let mut byte = 0xAA;
        // SAFETY: valid writable outputs, plus rejected null buffer/length.
        unsafe {
            code(
                reflect_last_error(null_mut(), 0, &mut len),
                REFLECT_ERR_NULL_POINTER,
            );
            assert_eq!(len, 0);
            code(reflect_last_error(&mut byte, 0, &mut len), REFLECT_OK);
            assert_eq!(len, 0);
            assert_eq!(byte, 0xAA);
        }
    })
    .join()
    .expect("fresh-thread diagnostic check");
    let mut version = 0;
    // SAFETY: aligned writable output, followed by a null rejection case.
    unsafe {
        code(reflect_abi_version(&mut version), REFLECT_OK);
        assert_eq!(version, 1);
        assert_eq!(REFLECT_ABI_VERSION, 1);
        code(reflect_abi_version(null_mut()), REFLECT_ERR_NULL_POINTER);
    }
}

#[test]
fn abi_set_get_roundtrip() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    // SAFETY: all slices and outputs are valid, aligned and disjoint.
    unsafe {
        let mut count = usize::MAX;
        code(reflect_type_count(&mut count), REFLECT_OK);
        assert_eq!(count, 2);
        assert_eq!(
            bytes(|buf, cap, len| reflect_type_name(0, buf, cap, len)),
            "Vehicle"
        );
        assert_eq!(
            bytes(|buf, cap, len| reflect_type_name(1, buf, cap, len)),
            "Wheel"
        );
        code(
            reflect_field_count(b"Vehicle".as_ptr(), 7, &mut count),
            REFLECT_OK,
        );
        assert_eq!(count, 6);
        for (index, name) in ["name", "mass_kg", "position", "wheels", "note", "drive"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                bytes(|buf, cap, len| {
                    reflect_field_name(b"Vehicle".as_ptr(), 7, index, buf, cap, len)
                }),
                name
            );
        }
    }
    let handle = create();
    assert_eq!(get(handle, "mass_kg"), "0.0");
    assert_eq!(get(handle, "drive"), "Rear");
    code(set(handle, "mass_kg", "1350.0"), REFLECT_OK);
    assert_eq!(get(handle, "mass_kg"), "1350.0");
    let quoted = r#""car \"one\"""#;
    code(set(handle, "name", quoted), REFLECT_OK);
    assert_eq!(get(handle, "name"), quoted);
    code(
        set(handle, "wheels", "[{radius_m=0.3,width_m=0.2,steered=true}]"),
        REFLECT_OK,
    );
    assert_eq!(get(handle, "wheels[0].radius_m"), "0.3");
    code(set(handle, "wheels[0].radius_m", "2.0"), REFLECT_ERR_REFUSED);
    assert!(last_error().contains("value outside the field's range"));
    assert_eq!(get(handle, "wheels[0].radius_m"), "0.3");
    code(set(handle, "mass_kg", "6000.0"), REFLECT_ERR_REFUSED);
    assert_eq!(get(handle, "mass_kg"), "1350.0");
    for token in ["1350", "NaN", "inf", "-inf", "1e9999"] {
        code(set(handle, "mass_kg", token), REFLECT_ERR_VALUE);
        assert_eq!(get(handle, "mass_kg"), "1350.0");
    }
    code(set(handle, "nope", "1.0"), REFLECT_ERR_REFUSED);
    code(set(handle, "a..b", "1.0"), REFLECT_ERR_PATH);
    refused_get(handle, "nope", REFLECT_ERR_REFUSED);
    refused_get(handle, "a..b", REFLECT_ERR_PATH);
    // Exact canonical token pass-through, not an arithmetic oracle.
    for token in [
        "5e-324",
        "2.2250738585072014e-308",
        "-0.0",
        "1.7976931348623157e308",
    ] {
        code(set(handle, "position[0]", token), REFLECT_OK);
        assert_eq!(get(handle, "position[0]"), token);
    }
    let second = create();
    assert_eq!(get(second, "mass_kg"), "0.0");
    assert_eq!(get(handle, "mass_kg"), "1350.0");
    destroy(handle);
    // Reusing the slot must not revive any copy of the old generation.
    let replacement = create();
    assert_ne!(replacement, handle);
    refused_get(handle, "mass_kg", REFLECT_ERR_STALE_HANDLE);
    code(set(handle, "mass_kg", "1350.0"), REFLECT_ERR_STALE_HANDLE);
    // SAFETY: scalar handles are validated before use.
    unsafe {
        code(reflect_destroy(handle), REFLECT_ERR_STALE_HANDLE);
        code(reflect_destroy(0), REFLECT_ERR_NULL_HANDLE);
        code(reflect_destroy(u64::MAX), REFLECT_ERR_STALE_HANDLE);
        code(reflect_destroy(1), REFLECT_ERR_STALE_HANDLE);
        code(reflect_destroy(1u64 << 32), REFLECT_ERR_STALE_HANDLE);
    }
    refused_get(0, "mass_kg", REFLECT_ERR_NULL_HANDLE);
    code(set(0, "mass_kg", "1350.0"), REFLECT_ERR_NULL_HANDLE);
    destroy(second);
    destroy(replacement);
}

#[test]
fn abi_rejects_null_and_bad_utf8() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    let handle = create();
    let fields = ["name", "mass_kg", "position", "wheels", "note", "drive"];
    let before: Vec<_> = fields.iter().map(|path| get(handle, path)).collect();
    let name = b"Vehicle";
    let path = b"mass_kg";
    let token = b"1350.0";
    let bad = [0x66, 0xff];
    let mut out_handle = u64::MAX;
    let mut count = usize::MAX;
    let mut len = usize::MAX;
    // SAFETY: non-null allocations are valid, aligned and disjoint. Null and
    // oversized inputs deliberately test rejection before any dereference.
    unsafe {
        for zero_or_len in [0, name.len()] {
            code(
                reflect_create(null(), zero_or_len, &mut out_handle),
                REFLECT_ERR_NULL_POINTER,
            );
            code(
                reflect_field_count(null(), zero_or_len, &mut count),
                REFLECT_ERR_NULL_POINTER,
            );
        }
        for zero_or_len in [0, path.len()] {
            code(
                reflect_get(handle, null(), zero_or_len, null_mut(), 0, &mut len),
                REFLECT_ERR_NULL_POINTER,
            );
            code(
                reflect_set(handle, null(), zero_or_len, token.as_ptr(), token.len()),
                REFLECT_ERR_NULL_POINTER,
            );
        }
        for zero_or_len in [0, token.len()] {
            code(
                reflect_set(handle, path.as_ptr(), path.len(), null(), zero_or_len),
                REFLECT_ERR_NULL_POINTER,
            );
        }
        code(reflect_type_count(null_mut()), REFLECT_ERR_NULL_POINTER);
        code(
            reflect_field_count(name.as_ptr(), name.len(), null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_create(name.as_ptr(), name.len(), null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_create(name.as_ptr(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_field_count(name.as_ptr(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_type_name(0, null_mut(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_field_name(name.as_ptr(), name.len(), 0, null_mut(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_get(handle, path.as_ptr(), path.len(), null_mut(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_last_error(null_mut(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
        code(
            reflect_create(bad.as_ptr(), bad.len(), &mut out_handle),
            REFLECT_ERR_BAD_UTF8,
        );
        code(
            reflect_get(handle, bad.as_ptr(), bad.len(), null_mut(), 0, &mut len),
            REFLECT_ERR_BAD_UTF8,
        );
        code(
            reflect_set(handle, path.as_ptr(), path.len(), bad.as_ptr(), bad.len()),
            REFLECT_ERR_BAD_UTF8,
        );
        code(
            reflect_create(b"Nope".as_ptr(), 4, &mut out_handle),
            REFLECT_ERR_NO_TYPE,
        );
        code(
            reflect_field_count(b"Nope".as_ptr(), 4, &mut count),
            REFLECT_ERR_NO_TYPE,
        );
        code(
            reflect_field_name(b"Nope".as_ptr(), 4, 0, null_mut(), 0, &mut len),
            REFLECT_ERR_NO_TYPE,
        );
        code(
            reflect_type_name(2, null_mut(), 0, &mut len),
            REFLECT_ERR_INDEX,
        );
        code(
            reflect_field_name(name.as_ptr(), name.len(), 6, null_mut(), 0, &mut len),
            REFLECT_ERR_INDEX,
        );
        let oversized = isize::MAX as usize + 1;
        code(
            reflect_create(name.as_ptr(), oversized, &mut out_handle),
            REFLECT_ERR_LIMIT,
        );
        code(
            reflect_get(handle, path.as_ptr(), oversized, null_mut(), 0, &mut len),
            REFLECT_ERR_LIMIT,
        );
        code(
            reflect_set(handle, path.as_ptr(), path.len(), token.as_ptr(), oversized),
            REFLECT_ERR_LIMIT,
        );
        assert_eq!(out_handle, u64::MAX);
        assert_eq!(count, usize::MAX);
        assert_eq!(len, usize::MAX);
        code(
            reflect_type_name(0, null_mut(), 0, &mut len),
            REFLECT_ERR_BUFFER_TOO_SMALL,
        );
        assert_eq!(len, 7);
        let mut short = [0xAA; 7];
        code(
            reflect_type_name(0, short.as_mut_ptr(), 6, &mut len),
            REFLECT_ERR_BUFFER_TOO_SMALL,
        );
        assert_eq!(short, [0xAA; 7]);
        code(
            reflect_type_name(0, null_mut(), 7, &mut len),
            REFLECT_ERR_NULL_POINTER,
        );
    }
    for token in ["", "1350.0\n", "1350.0\r", "1350.0\nend\nv = 6000.0"] {
        code(set(handle, "mass_kg", token), REFLECT_ERR_VALUE);
    }
    for path in ["", "C:\\mass_kg", "mass_kg\r", "mass_kg\n", "wheels[-1]"] {
        code(set(handle, path, "1350.0"), REFLECT_ERR_PATH);
    }
    let limits = reflect::limits::Limits::parse(include_str!("../reflect.toml")).unwrap();
    let huge_path = "a".repeat(limits.max_path_length + 1);
    code(set(handle, &huge_path, "1.0"), REFLECT_ERR_PATH);
    let huge_token = "0".repeat(usize::try_from(limits.max_text_file_bytes).unwrap() + 1);
    code(set(handle, "mass_kg", &huge_token), REFLECT_ERR_VALUE);
    let after: Vec<_> = fields.iter().map(|path| get(handle, path)).collect();
    assert_eq!(after, before, "rejections must preserve every field");
    // A successful get and failed diagnostic queries must retain the first text.
    let diagnostic = last_error();
    assert!(!diagnostic.is_empty());
    // SAFETY: writable lengths; null buffers are permitted for size queries.
    unsafe {
        for _ in 0..2 {
            code(
                reflect_last_error(null_mut(), 0, &mut len),
                REFLECT_ERR_BUFFER_TOO_SMALL,
            );
            assert_eq!(len, diagnostic.len());
        }
        let mut short = vec![0xAA; diagnostic.len()];
        code(
            reflect_last_error(short.as_mut_ptr(), short.len() - 1, &mut len),
            REFLECT_ERR_BUFFER_TOO_SMALL,
        );
        assert!(short.iter().all(|byte| *byte == 0xAA));
        code(
            reflect_last_error(null_mut(), 0, null_mut()),
            REFLECT_ERR_NULL_POINTER,
        );
    }
    assert_eq!(last_error(), diagnostic);
    // Independent fixed ABI-bound fixture, not a reflection-data setting.
    const ABI_HANDLE_BOUND: usize = 4096;
    let handles: Vec<_> = (1..ABI_HANDLE_BOUND).map(|_| create()).collect();
    // SAFETY: valid readable type and aligned writable output.
    code(
        unsafe { reflect_create(name.as_ptr(), name.len(), &mut out_handle) },
        REFLECT_ERR_LIMIT,
    );
    assert_eq!(out_handle, u64::MAX);
    for extra in handles {
        destroy(extra);
    }
    assert_eq!(get(handle, "mass_kg"), "0.0");
    destroy(handle);
}

#[test]
fn abi_panic_never_unwinds() {
    let _serial = ABI_TESTS.lock().expect("ABI test lock");
    // SAFETY: no pointer arguments; panic is contained inside the boundary.
    code(unsafe { reflect_selftest_panic() }, REFLECT_ERR_PANIC);
    assert_eq!(last_error(), "reflect selftest panic");
    let handle = create();
    assert_eq!(get(handle, "mass_kg"), "0.0");
    destroy(handle);
}

#[test]
fn registry_lists_and_creates_types() {
    let mut registry = Registry::new();
    assert!(registry.is_empty());
    assert!(registry.desc(0).is_none());
    register_samples(&mut registry).unwrap();
    let before = registry.desc(0).unwrap().clone();
    assert_eq!(
        registry.register::<Vehicle>(),
        Err(RegistryError::Duplicate {
            name: "Vehicle".into()
        })
    );
    assert_eq!(registry.len(), 2);
    assert_eq!(registry.desc(0), Some(&before));
    assert_eq!(registry.find("Wheel"), Some(1));
    assert_eq!(registry.find("Nope"), None);
    let object = registry.create("Wheel").unwrap();
    assert_eq!(object.get(0), Some(Value::F64(0.0)));
    assert_eq!(object.get(1), Some(Value::F64(0.0)));
    assert_eq!(object.get(2), Some(Value::Bool(false)));
    let unknown = match registry.create("Nope") {
        Ok(_) => panic!("unknown type was created"),
        Err(error) => error,
    };
    assert_eq!(
        unknown,
        CreateError::NoType {
            name: "Nope".into()
        }
    );
    assert_eq!(unknown.to_string(), "no type Nope");
    assert_eq!(
        zero_value(&Kind::Array(Box::new(Kind::U64), 3)),
        Value::List(vec![Value::U64(0); 3])
    );
    assert_eq!(zero_value(&DriveMode::kind()), Value::Enum("Front".into()));
    assert_eq!(zero_value(&Kind::Bool), Value::Bool(false));
    assert_eq!(zero_value(&Kind::I64), Value::I64(0));
    assert_eq!(zero_value(&Kind::Text), Value::Text(String::new()));
    assert_eq!(
        zero_value(&Kind::List(Box::new(Kind::F64))),
        Value::List(vec![])
    );
    assert_eq!(
        zero_value(&Kind::Option(Box::new(Kind::F64))),
        Value::Option(None)
    );
    let value = zero_value(&Kind::Struct(Box::new(Vehicle::desc())));
    let Value::Struct(fields) = value else {
        panic!("expected struct zero value");
    };
    assert_eq!(fields.last(), Some(&Value::Enum("Rear".into())));

    // A real constructor refusal must be reported, not replaced with an object.
    struct Refuses;
    impl Field for Refuses {
        fn kind() -> Kind {
            Kind::Struct(Box::new(Self::desc()))
        }
        fn to_value(&self) -> Value {
            Value::Struct(vec![])
        }
        fn from_value(_: Value) -> Option<Self> {
            None
        }
    }
    impl Reflect for Refuses {
        fn desc() -> reflect::desc::TypeDesc {
            reflect::desc::TypeDesc::new("Refuses", 1, vec![]).unwrap()
        }
        fn dyn_desc(&self) -> reflect::desc::TypeDesc {
            Self::desc()
        }
        fn get(&self, _: usize) -> Option<Value> {
            None
        }
        fn set(&mut self, index: usize, _: Value) -> Result<(), reflect::desc::SetError> {
            Err(reflect::desc::SetError::NoField { index, count: 0 })
        }
    }
    let mut refusing = Registry::default();
    refusing.register::<Refuses>().unwrap();
    let error = match refusing.create("Refuses") {
        Ok(_) => panic!("refused constructor succeeded"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        CreateError::Build {
            name: "Refuses".into()
        }
    );
    assert_eq!(error.to_string(), "cannot build Refuses from its zero value");
}
