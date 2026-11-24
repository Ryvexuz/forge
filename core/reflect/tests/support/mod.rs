#![allow(dead_code)]

use reflect::desc::{FieldDesc, Kind, Reflect, SetError, TypeDesc};
use reflect::value::Value;

pub struct Tiny {
    pub title: String,
    pub scale: f64,
    pub count: u64,
}

impl Reflect for Tiny {
    fn desc() -> TypeDesc {
        let mut title = FieldDesc::new("title", Kind::Text);
        title.label = Some("Title".into());
        let mut scale = FieldDesc::new("scale", Kind::F64);
        scale.label = Some("Scale".into());
        scale.unit = Some("m".into());
        scale.min = Some(0.0);
        scale.max = Some(10.0);
        scale.default = Some(Value::F64(1.0));
        let mut count = FieldDesc::new("count", Kind::U64);
        count.readonly = true;
        TypeDesc::new("Tiny", 1, vec![title, scale, count]).expect("valid test descriptor")
    }

    fn dyn_desc(&self) -> TypeDesc {
        Self::desc()
    }

    fn get(&self, index: usize) -> Option<Value> {
        match index {
            0 => Some(Value::Text(self.title.clone())),
            1 => Some(Value::F64(self.scale)),
            2 => Some(Value::U64(self.count)),
            _ => None,
        }
    }

    fn set(&mut self, index: usize, value: Value) -> Result<(), SetError> {
        let desc = Self::desc();
        let field = desc.field(index).ok_or(SetError::NoField {
            index,
            count: desc.fields().len(),
        })?;
        if !field.kind.accepts(&value) {
            return Err(SetError::WrongKind {
                field: field.name.clone(),
                expected: field.kind.name(),
                got: value.kind_name(),
            });
        }
        match (index, value) {
            (0, Value::Text(v)) => self.title = v,
            (1, Value::F64(v)) => self.scale = v,
            (2, Value::U64(v)) => self.count = v,
            _ => unreachable!("descriptor and field mapping agree"),
        }
        Ok(())
    }
}

pub fn tiny() -> Tiny {
    Tiny {
        title: "probe".to_string(),
        scale: 2.5,
        count: 3,
    }
}

pub struct XorShift64(u64);

impl XorShift64 {
    pub fn new(seed: u64) -> XorShift64 {
        assert_ne!(seed, 0, "xorshift requires a nonzero seed");
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
}

pub fn random_value(kind: &Kind, rng: &mut XorShift64, depth: u32) -> Value {
    let next_depth = depth.saturating_sub(1);
    match kind {
        Kind::Bool => Value::Bool(!rng.next_u64().is_multiple_of(2)),
        Kind::I64 => Value::I64(rng.next_u64() as i64),
        Kind::U64 => Value::U64(rng.next_u64()),
        Kind::F64 => Value::F64((rng.next_u64() as i64) as f64 / 1024.0),
        Kind::Text => {
            let alphabet = ['a', 'Z', ' ', '"', '\\', 'é'];
            let len = rng.next_u64() % 12;
            Value::Text(
                (0..len)
                    .map(|_| alphabet[(rng.next_u64() % alphabet.len() as u64) as usize])
                    .collect(),
            )
        }
        Kind::List(element) => {
            let len = if depth == 0 {
                0
            } else {
                rng.next_u64() % 4
            };
            Value::List(
                (0..len)
                    .map(|_| random_value(element, rng, next_depth))
                    .collect(),
            )
        }
        Kind::Array(element, len) => Value::List(
            (0..*len)
                .map(|_| random_value(element, rng, next_depth))
                .collect(),
        ),
        Kind::Struct(desc) => Value::Struct(
            desc.fields()
                .iter()
                .map(|f| random_value(&f.kind, rng, next_depth))
                .collect(),
        ),
        Kind::Option(element) => Value::Option(if depth == 0 || rng.next_u64().is_multiple_of(2) {
            None
        } else {
            Some(Box::new(random_value(element, rng, next_depth)))
        }),
        Kind::Enum { variants, .. } => {
            let index = (rng.next_u64() % variants.len() as u64) as usize;
            Value::Enum(variants[index].clone())
        }
    }
}
