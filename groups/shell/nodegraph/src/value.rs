//! Shared type tags and data-only value payloads.

/// Built-in value categories, plus an open tag for kind-defined descriptions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValueType {
    Bool,
    Int,
    F64,
    Text,
    Image,
    Named(String),
}

/// Image storage without a prescribed channel layout or dimension validation.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageBuf {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<f32>,
}

/// Built-in payloads. Floating-point equality follows Rust's `PartialEq`.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    F64(f64),
    Text(String),
    Image(ImageBuf),
}

impl Value {
    /// Returns the built-in type tag corresponding to this payload.
    pub fn type_of(&self) -> ValueType {
        match self {
            Self::Bool(_) => ValueType::Bool,
            Self::Int(_) => ValueType::Int,
            Self::F64(_) => ValueType::F64,
            Self::Text(_) => ValueType::Text,
            Self::Image(_) => ValueType::Image,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ImageBuf, Value, ValueType};

    #[test]
    fn values_report_types_and_compare_payloads() {
        let cases = [
            (Value::Bool(true), ValueType::Bool),
            (Value::Int(i64::MIN), ValueType::Int),
            (Value::F64(1.5), ValueType::F64),
            (Value::Text("hello".into()), ValueType::Text),
            (
                Value::Image(ImageBuf {
                    width: 1,
                    height: 1,
                    pixels: vec![0.0, 0.5, 1.0],
                }),
                ValueType::Image,
            ),
        ];
        for (value, expected_type) in cases {
            assert_eq!(value.type_of(), expected_type);
            assert_eq!(value.clone(), value);
        }
        assert_ne!(Value::Bool(false), Value::Bool(true));
        assert_ne!(Value::Int(0), Value::F64(0.0));
        let named = ValueType::Named("custom".into());
        let same_name = ValueType::Named(String::from("custom"));
        assert_eq!(named, same_name);
        assert_ne!(ValueType::Named("custom".into()), ValueType::Text);
    }

    #[test]
    fn data_only_floats_preserve_ieee_equality() {
        for number in [f64::MIN, f64::MAX, f64::from_bits(1), f64::INFINITY] {
            let left = Value::F64(number);
            let right = Value::F64(number);
            assert_eq!(left, right);
        }
        assert_eq!(Value::F64(-0.0), Value::F64(0.0));
        let left_nan = Value::F64(f64::NAN);
        let right_nan = Value::F64(f64::NAN);
        assert_ne!(left_nan, right_nan);
        let image = ImageBuf {
            width: 0,
            height: u32::MAX,
            pixels: vec![f32::MIN, f32::MAX, f32::from_bits(1), f32::INFINITY],
        };
        assert_eq!(image.clone(), image);
        let left_nan_image = ImageBuf {
            width: 1,
            height: 1,
            pixels: vec![f32::NAN],
        };
        let right_nan_image = ImageBuf {
            width: 1,
            height: 1,
            pixels: vec![f32::NAN],
        };
        assert_ne!(left_nan_image, right_nan_image);
    }
}
