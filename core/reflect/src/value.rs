//! Reflected value module.

/// One reflected value. Struct fields are in descriptor order; lists represent
/// vectors or arrays, and enums hold unit variant names.
/// Derived equality is numeric, not bit-exact: NaN != NaN and -0.0 == 0.0.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    Text(String),
    List(Vec<Value>),
    Struct(Vec<Value>),
    Option(Option<Box<Value>>),
    Enum(String),
}

impl Value {
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Bool(_) => "bool",
            Self::I64(_) => "i64",
            Self::U64(_) => "u64",
            Self::F64(_) => "f64",
            Self::Text(_) => "text",
            Self::List(_) => "list",
            Self::Struct(_) => "struct",
            Self::Option(_) => "option",
            Self::Enum(_) => "enum",
        }
    }
}
