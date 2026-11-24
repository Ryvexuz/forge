use std::cell::RefCell;
use std::rc::Rc;

use reflect::change::{Change, ChangeSink};
use reflect::desc::Reflect;
use reflect::limits::Limits;
use reflect::path::{parse_path, set_path, AccessError};
use reflect::sample::{DriveMode, Vehicle, Wheel};
use reflect::value::Value;
use reflect::watch::{FanOut, Journal};

fn limits() -> Limits {
    Limits {
        max_path_length: 256,
        max_text_file_bytes: 1048576,
        max_list_length: 4096,
        max_nesting_depth: 16,
    }
}

fn vehicle() -> Vehicle {
    Vehicle {
        name: "Roadster".into(),
        mass_kg: 1200.0,
        position: [1.0, 2.0, 3.0],
        wheels: vec![
            Wheel {
                radius_m: 0.3,
                width_m: 0.2,
                steered: true,
            },
            Wheel {
                radius_m: 0.4,
                width_m: 0.25,
                steered: false,
            },
        ],
        note: None,
        drive: DriveMode::Rear,
    }
}

#[test]
fn change_event_on_set() {
    let limits = limits();
    let mut root = vehicle();
    let mut a = Journal::new();
    let mut b = Journal::new();
    {
        let mut sink = FanOut::new();
        assert_eq!(sink.add(&mut a), 0);
        assert_eq!(sink.add(&mut b), 1);
        for (text, value, expected) in [
            ("mass_kg", Value::F64(1350.0), Ok(())),
            (
                "mass_kg",
                Value::F64(6000.0),
                Err(AccessError::OutOfRange {
                    path: "mass_kg".into(),
                }),
            ),
            ("wheels[0].radius_m", Value::F64(0.35), Ok(())),
            ("name", Value::Text("Coupe".into()), Ok(())),
        ] {
            let path = parse_path(text, &limits).unwrap();
            assert_eq!(
                set_path(&mut root, &path, value, &limits, &mut sink),
                expected
            );
        }
    }
    let expected = [
        Change::new("Vehicle", "mass_kg", Value::F64(1200.0), Value::F64(1350.0)).unwrap(),
        Change::new(
            "Vehicle",
            "wheels[0].radius_m",
            Value::F64(0.3),
            Value::F64(0.35),
        )
        .unwrap(),
        Change::new(
            "Vehicle",
            "name",
            Value::Text("Roadster".into()),
            Value::Text("Coupe".into()),
        )
        .unwrap(),
    ];
    assert_eq!(a.entries(), expected.as_slice());
    assert_eq!(b.entries(), expected.as_slice());
    assert_eq!(a, b);
    assert_eq!(a.len(), 3);
    assert_eq!(root.mass_kg, 1350.0);
}

#[test]
fn rejected_set_emits_no_event() {
    let limits = limits();
    let mut root = vehicle();
    let original = vehicle();
    let mut a = Journal::new();
    let mut b = Journal::new();
    for (text, value, error) in [
        (
            "mass_kg",
            Value::F64(6000.0),
            AccessError::OutOfRange {
                path: "mass_kg".into(),
            },
        ),
        (
            "mass_kg",
            Value::F64(499.0),
            AccessError::OutOfRange {
                path: "mass_kg".into(),
            },
        ),
        (
            "mass_kg",
            Value::F64(f64::NAN),
            AccessError::NonFinite {
                path: "mass_kg".into(),
            },
        ),
        (
            "name",
            Value::I64(1),
            AccessError::WrongKind {
                path: "name".into(),
                expected: "text",
                got: "i64",
            },
        ),
        (
            "wheels[2]",
            Value::Struct(vec![Value::F64(0.3), Value::F64(0.2), Value::Bool(true)]),
            AccessError::IndexOutOfRange {
                path: "wheels[2]".into(),
                index: 2,
                len: 2,
            },
        ),
        (
            "nope",
            Value::Bool(true),
            AccessError::NoField {
                path: "nope".into(),
                name: "nope".into(),
            },
        ),
        (
            "wheels[0].radius_m",
            Value::F64(2.0),
            AccessError::OutOfRange {
                path: "wheels[0].radius_m".into(),
            },
        ),
    ] {
        {
            let mut sink = FanOut::new();
            sink.add(&mut a);
            sink.add(&mut b);
            let path = parse_path(text, &limits).unwrap();
            assert_eq!(
                set_path(&mut root, &path, value, &limits, &mut sink),
                Err(error)
            );
        }
        assert!(a.is_empty(), "first journal received refused set {text}");
        assert!(b.is_empty(), "second journal received refused set {text}");
        for i in 0..6 {
            assert_eq!(root.get(i), original.get(i), "field {i} after {text}");
        }
    }
    {
        let mut sink = FanOut::new();
        sink.add(&mut a);
        sink.add(&mut b);
        let path = parse_path("mass_kg", &limits).unwrap();
        set_path(&mut root, &path, Value::F64(5000.0), &limits, &mut sink).unwrap();
    }
    let expected =
        Change::new("Vehicle", "mass_kg", Value::F64(1200.0), Value::F64(5000.0)).unwrap();
    assert_eq!(a.entries(), &[expected]);
    assert_eq!(a, b);
}

struct Tag {
    tag: char,
    log: Rc<RefCell<Vec<(char, String)>>>,
}

impl ChangeSink for Tag {
    fn changed(&mut self, change: &Change) {
        self.log
            .borrow_mut()
            .push((self.tag, change.path().to_string()));
    }
}

#[test]
fn fan_out_delivers_to_every_watcher_in_order() {
    let limits = limits();
    let mut root = vehicle();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut a = Tag {
        tag: 'a',
        log: Rc::clone(&log),
    };
    let mut b = Tag {
        tag: 'b',
        log: Rc::clone(&log),
    };
    let mut c = Tag {
        tag: 'c',
        log: Rc::clone(&log),
    };
    {
        let mut sink = FanOut::new();
        assert_eq!(sink.add(&mut a), 0);
        assert_eq!(sink.add(&mut b), 1);
        assert_eq!(sink.add(&mut c), 2);
        assert_eq!(sink.len(), 3);
        for (text, value) in [
            ("mass_kg", Value::F64(1300.0)),
            ("name", Value::Text("X".into())),
        ] {
            let path = parse_path(text, &limits).unwrap();
            set_path(&mut root, &path, value, &limits, &mut sink).unwrap();
        }
    }
    let expected: Vec<_> = [
        ('a', "mass_kg"),
        ('b', "mass_kg"),
        ('c', "mass_kg"),
        ('a', "name"),
        ('b', "name"),
        ('c', "name"),
    ]
    .into_iter()
    .map(|(tag, path)| (tag, path.to_string()))
    .collect();
    assert_eq!(*log.borrow(), expected);

    let mut empty = FanOut::default();
    assert!(empty.is_empty());
    let path = parse_path("mass_kg", &limits).unwrap();
    set_path(&mut root, &path, Value::F64(1400.0), &limits, &mut empty).unwrap();

    let mut x = Journal::new();
    let mut y = Journal::new();
    {
        let mut inner = FanOut::new();
        inner.add(&mut y);
        let mut outer = FanOut::new();
        outer.add(&mut x);
        outer.add(&mut inner);
        set_path(&mut root, &path, Value::F64(1500.0), &limits, &mut outer).unwrap();
    }
    assert_eq!(x.len(), 1);
    assert_eq!(y.len(), 1);
    assert_eq!(x.entries(), y.entries());

    // Shared logging also makes nested delivery order observable.
    log.borrow_mut().clear();
    {
        let mut inner = FanOut::new();
        inner.add(&mut b);
        let mut outer = FanOut::new();
        outer.add(&mut a);
        outer.add(&mut inner);
        outer.add(&mut c);
        set_path(&mut root, &path, Value::F64(1600.0), &limits, &mut outer).unwrap();
    }
    assert_eq!(*log.borrow(), expected[..3]);
}

#[test]
fn journal_records_and_clears() {
    let mut journal = Journal::new();
    assert!(journal.is_empty());
    let mut expected = Vec::new();
    for k in 0..3 {
        let change = Change::new("T", "p", Value::I64(k), Value::I64(k + 1)).unwrap();
        journal.changed(&change);
        expected.push(change);
    }
    assert_eq!(journal.entries(), expected.as_slice());
    let equal = Change::new("T", "p", Value::I64(5), Value::I64(5)).unwrap();
    journal.changed(&equal);
    expected.push(equal);
    assert_eq!(journal.entries(), expected.as_slice());
    assert_eq!(journal.clone(), journal);
    journal.clear();
    assert!(journal.is_empty());
    assert_eq!(journal.len(), 0);
    assert_eq!(Journal::new(), Journal::default());
}
