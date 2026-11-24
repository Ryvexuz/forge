//! Single-declaration reflection and recursive field conversion.

use crate::desc::Kind;
use crate::value::Value;

/// How one Rust field type maps to a reflected kind and value.
pub trait Field: Sized {
    fn kind() -> Kind;
    fn to_value(&self) -> Value;
    /// Returns `None` for a shape mismatch, never a partial result.
    fn from_value(value: Value) -> Option<Self>;
}

macro_rules! scalar_field {
    ($ty:ty, $variant:ident) => {
        impl Field for $ty {
            fn kind() -> Kind {
                Kind::$variant
            }
            fn to_value(&self) -> Value {
                Value::$variant(*self)
            }
            fn from_value(value: Value) -> Option<Self> {
                match value {
                    Value::$variant(value) => Some(value),
                    _ => None,
                }
            }
        }
    };
}

scalar_field!(bool, Bool);
scalar_field!(i64, I64);
scalar_field!(u64, U64);
scalar_field!(f64, F64);

impl Field for String {
    fn kind() -> Kind {
        Kind::Text
    }
    fn to_value(&self) -> Value {
        Value::Text(self.clone())
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Text(value) => Some(value),
            _ => None,
        }
    }
}

impl<T: Field> Field for Vec<T> {
    fn kind() -> Kind {
        Kind::List(Box::new(T::kind()))
    }
    fn to_value(&self) -> Value {
        Value::List(self.iter().map(Field::to_value).collect())
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::List(values) => values.into_iter().map(T::from_value).collect(),
            _ => None,
        }
    }
}

impl<T: Field> Field for Option<T> {
    fn kind() -> Kind {
        Kind::Option(Box::new(T::kind()))
    }
    fn to_value(&self) -> Value {
        Value::Option(self.as_ref().map(|value| Box::new(value.to_value())))
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::Option(None) => Some(None),
            Value::Option(Some(value)) => T::from_value(*value).map(Some),
            _ => None,
        }
    }
}

impl<T: Field, const N: usize> Field for [T; N] {
    fn kind() -> Kind {
        Kind::Array(Box::new(T::kind()), N)
    }
    fn to_value(&self) -> Value {
        Value::List(self.iter().map(Field::to_value).collect())
    }
    fn from_value(value: Value) -> Option<Self> {
        match value {
            Value::List(values) if values.len() == N => {
                let values: Option<Vec<T>> = values.into_iter().map(T::from_value).collect();
                values?.try_into().ok()
            }
            _ => None,
        }
    }
}

/// Applies attributes without requiring a mutable binding for a bare field.
#[doc(hidden)]
#[macro_export]
macro_rules! __reflect_attrs {
    ($desc:expr, $ty:ty;) => {
        $desc
    };
    ($desc:expr, $ty:ty; label = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        let value: &str = $value;
        field.label = ::core::option::Option::Some(value.to_string());
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; unit = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        let value: &str = $value;
        field.unit = ::core::option::Option::Some(value.to_string());
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; min = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        field.min = ::core::option::Option::Some($value);
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; max = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        field.max = ::core::option::Option::Some($value);
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; readonly $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        field.readonly = true;
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; hidden $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        field.hidden = true;
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; default = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        let value: $ty = $value;
        field.default = ::core::option::Option::Some(<$ty as $crate::macros::Field>::to_value(&value));
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; renamed_from = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        let value: &str = $value;
        field.renamed_from = ::core::option::Option::Some(value.to_string());
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; since = $value:expr $(, $($rest:tt)*)?) => {{
        let mut field = $desc;
        field.since = $value;
        $crate::__reflect_attrs!(field, $ty; $($($rest)*)?)
    }};
    ($desc:expr, $ty:ty; $unknown:ident $($rest:tt)*) => {
        ::core::compile_error!(::core::concat!("reflect!: unknown field attribute ", ::core::stringify!($unknown)))
    };
}

/// Declares a versioned named-field struct or a unit enum and its field mapping.
#[macro_export]
macro_rules! reflect {
    (
        #[reflect(version = $version:expr)]
        $vis:vis struct $name:ident {
            $($(#[reflect($($attrs:tt)*)])?
              $field_vis:vis $field:ident: $ty:ty),* $(,)?
        }
    ) => {
        #[derive(Clone, Debug, PartialEq)]
        $vis struct $name {
            $($field_vis $field: $ty),*
        }

        impl $crate::macros::Field for $name {
            fn kind() -> $crate::desc::Kind {
                $crate::desc::Kind::Struct(::std::boxed::Box::new(
                    <Self as $crate::desc::Reflect>::desc(),
                ))
            }
            fn to_value(&self) -> $crate::value::Value {
                $crate::value::Value::Struct(::std::vec![
                    $(<$ty as $crate::macros::Field>::to_value(&self.$field)),*
                ])
            }
            fn from_value(value: $crate::value::Value) -> ::core::option::Option<Self> {
                if let $crate::value::Value::Struct(values) = value {
                    let mut values = values.into_iter();
                    let result = Self {
                        $($field: <$ty as $crate::macros::Field>::from_value(values.next()?)?),*
                    };
                    if values.next().is_some() { ::core::option::Option::None } else { ::core::option::Option::Some(result) }
                } else {
                    ::core::option::Option::None
                }
            }
        }

        impl $crate::desc::Reflect for $name {
            fn desc() -> $crate::desc::TypeDesc {
                $crate::desc::TypeDesc::new(::core::stringify!($name), $version, ::std::vec![
                    $($crate::__reflect_attrs!(
                        $crate::desc::FieldDesc::new(
                            ::core::stringify!($field), <$ty as $crate::macros::Field>::kind()
                        ), $ty; $($($attrs)*)?
                    )),*
                ]).expect(::core::concat!("reflect!: invalid descriptor for ", ::core::stringify!($name)))
            }
            fn dyn_desc(&self) -> $crate::desc::TypeDesc {
                <Self as $crate::desc::Reflect>::desc()
            }
            fn get(&self, index: usize) -> ::core::option::Option<$crate::value::Value> {
                let names: &[&str] = &[$(::core::stringify!($field)),*];
                match names.get(index).copied() {
                    $(::core::option::Option::Some(::core::stringify!($field)) =>
                        ::core::option::Option::Some(<$ty as $crate::macros::Field>::to_value(&self.$field)),)*
                    _ => ::core::option::Option::None,
                }
            }
            fn set(
                &mut self, index: usize, value: $crate::value::Value,
            ) -> ::core::result::Result<(), $crate::desc::SetError> {
                let names: &[&str] = &[$(::core::stringify!($field)),*];
                match (names.get(index).copied(), value) {
                    $((::core::option::Option::Some(::core::stringify!($field)), value) => {
                        let got = value.kind_name();
                        let converted = <$ty as $crate::macros::Field>::from_value(value)
                            .ok_or_else(|| $crate::desc::SetError::WrongKind {
                                field: ::core::stringify!($field).to_string(),
                                expected: <$ty as $crate::macros::Field>::kind().name(),
                                got,
                            })?;
                        self.$field = converted;
                        ::core::result::Result::Ok(())
                    },)*
                    _ => ::core::result::Result::Err($crate::desc::SetError::NoField { index, count: names.len() }),
                }
            }
        }
    };
    ($vis:vis enum $name:ident { $($variant:ident),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name { $($variant),+ }

        impl $crate::macros::Field for $name {
            fn kind() -> $crate::desc::Kind {
                $crate::desc::Kind::Enum {
                    name: ::core::stringify!($name).to_string(),
                    variants: ::std::vec![$(::core::stringify!($variant).to_string()),+],
                }
            }
            fn to_value(&self) -> $crate::value::Value {
                $crate::value::Value::Enum(match self {
                    $(Self::$variant => ::core::stringify!($variant)),+
                }.to_string())
            }
            fn from_value(value: $crate::value::Value) -> ::core::option::Option<Self> {
                match value {
                    $crate::value::Value::Enum(name) => match name.as_str() {
                        $(::core::stringify!($variant) => ::core::option::Option::Some(Self::$variant),)+
                        _ => ::core::option::Option::None,
                    },
                    _ => ::core::option::Option::None,
                }
            }
        }
    };
}
