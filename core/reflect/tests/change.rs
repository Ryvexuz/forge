use reflect::change::{Change, ChangeError, ChangeSink};
use reflect::value::Value;

#[test]
fn change_record_refuses_kind_change() {
    let first = Change::new("T", "scale", Value::F64(1.0), Value::F64(2.0)).unwrap();
    assert_eq!(first.type_name(), "T");
    assert_eq!(first.path(), "scale");
    assert_eq!(first.old(), &Value::F64(1.0));
    assert_eq!(first.new_value(), &Value::F64(2.0));
    assert_eq!(
        Change::new("", "p", Value::Bool(false), Value::Bool(true)),
        Err(ChangeError::EmptyTypeName)
    );
    assert_eq!(
        Change::new("", "", Value::I64(1), Value::U64(1)),
        Err(ChangeError::EmptyTypeName)
    );
    assert_eq!(
        Change::new("T", "", Value::I64(1), Value::U64(1)),
        Err(ChangeError::EmptyPath)
    );
    assert_eq!(
        Change::new("T", "p", Value::I64(1), Value::U64(1)),
        Err(ChangeError::KindChanged {
            old: "i64",
            new: "u64"
        })
    );
    struct Collect(Vec<Change>);
    impl ChangeSink for Collect {
        fn changed(&mut self, change: &Change) {
            self.0.push(change.clone());
        }
    }
    let second = Change::new("T", "enabled", Value::Bool(false), Value::Bool(true)).unwrap();
    let mut collected = Collect(Vec::new());
    let sink: &mut dyn ChangeSink = &mut collected;
    sink.changed(&first);
    sink.changed(&second);
    assert_eq!(collected.0, vec![first, second]);
}
