//! Checks run in order: arguments, finite inputs, shapes and dimension
//! parameters, matching sizes, operation rules, then finite results.
//! Named evaluation errors use the public message constants below.

#[forbid(unsafe_code)]
pub mod image;
#[forbid(unsafe_code)]
pub mod math;

use crate::kind::{KindRegistry, NodeError, ParamDesc, PortDesc};
use crate::value::{Value, ValueType};

pub const BAD_ARGUMENTS: &str = "arguments do not match the node's ports and parameters";
pub const NOT_FINITE_INPUT: &str = "input or parameter is NaN or infinite";
pub const NOT_FINITE_RESULT: &str = "result is NaN or infinite";
pub const DIVIDE_BY_ZERO: &str = "divide by zero";
pub const CLAMP_RANGE: &str = "clamp lo is above hi";
pub const BAD_IMAGE: &str = "image has a zero side or a pixel count other than width times height";
pub const IMAGE_SIZE_RANGE: &str = "image width and height must be 1 to 4096";
pub const IMAGE_SIZE_MISMATCH: &str = "input images differ in size";
pub const BLEND_FACTOR_RANGE: &str = "blend factor must be 0 to 1";

/// Registers math then image types in their published array order.
/// Stops at the first error; a second call returns DuplicateType("math.constant").
pub fn register_all(registry: &mut KindRegistry) -> Result<(), NodeError> {
    math::register(registry)?;
    image::register(registry)
}

pub(crate) fn error(message: &str) -> NodeError {
    NodeError::Evaluation(message.to_owned())
}

pub(crate) fn check_args(
    params_desc: &[ParamDesc],
    inputs_desc: &[PortDesc],
    params: &[Value],
    inputs: &[Value],
) -> Result<(), NodeError> {
    if params.len() != params_desc.len()
        || inputs.len() != inputs_desc.len()
        || params
            .iter()
            .zip(params_desc)
            .any(|(v, d)| v.type_of() != d.value_type)
        || inputs
            .iter()
            .zip(inputs_desc)
            .any(|(v, d)| v.type_of() != d.value_type)
    {
        return Err(error(BAD_ARGUMENTS));
    }
    let image_kind = inputs_desc.iter().any(|d| d.value_type == ValueType::Image)
        || params_desc.iter().any(|d| d.name == "width");
    for value in params.iter().chain(inputs) {
        if !finite(value)
            || (image_kind && matches!(value, Value::F64(v) if !(*v as f32).is_finite()))
        {
            return Err(error(NOT_FINITE_INPUT));
        }
    }
    for value in inputs {
        if let Value::Image(image) = value {
            let count = usize::try_from(image.width).ok().and_then(|w| {
                usize::try_from(image.height)
                    .ok()
                    .and_then(|h| w.checked_mul(h))
            });
            if image.width == 0 || image.height == 0 || count != Some(image.pixels.len()) {
                return Err(error(BAD_IMAGE));
            }
        }
    }
    for (value, desc) in params.iter().zip(params_desc) {
        if matches!(desc.name.as_str(), "width" | "height")
            && !matches!(value, Value::Int(1..=4096))
        {
            return Err(error(IMAGE_SIZE_RANGE));
        }
    }
    if let [Value::Image(a), Value::Image(b)] = inputs {
        if a.width != b.width || a.height != b.height {
            return Err(error(IMAGE_SIZE_MISMATCH));
        }
    }
    Ok(())
}

fn finite(value: &Value) -> bool {
    match value {
        Value::F64(v) => v.is_finite(),
        Value::Image(image) => image.pixels.iter().all(|v| v.is_finite()),
        _ => true,
    }
}

pub(crate) fn result(value: Value) -> Result<Vec<Value>, NodeError> {
    if finite(&value) {
        Ok(vec![value])
    } else {
        Err(error(NOT_FINITE_RESULT))
    }
}
