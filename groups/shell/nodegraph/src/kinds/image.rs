//! Gray f32 pixels are row-major: (x, y) is at y * width + x.
//! Gradient samples pixel centres: (2*x + 1) / (2*width).
//! Blur coordinates outside the image clamp to the nearest border pixel.

use crate::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use crate::value::{ImageBuf, Value, ValueType};

use super::{check_args, error, result, BLEND_FACTOR_RANGE, IMAGE_SIZE_RANGE};

pub const IMAGE_TYPES: [&str; 8] = [
    "image.solid",
    "image.gradient",
    "image.invert",
    "image.add",
    "image.multiply",
    "image.threshold",
    "image.blur",
    "image.blend",
];

struct ImageNode {
    op: usize,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

pub fn register(registry: &mut KindRegistry) -> Result<(), NodeError> {
    for (op, _) in IMAGE_TYPES.iter().enumerate() {
        let (inputs, params): (&[&str], &[&str]) = match op {
            0 => (&[], &["width", "height", "value"]),
            1 => (&[], &["width", "height"]),
            3 | 4 => (&["a", "b"], &[]),
            5 => (&["a"], &["level"]),
            7 => (&["a", "b"], &["factor"]),
            _ => (&["a"], &[]),
        };
        let ports = |names: &[&str]| {
            names
                .iter()
                .map(|name| PortDesc {
                    name: (*name).into(),
                    value_type: ValueType::Image,
                })
                .collect()
        };
        registry.add(Box::new(ImageNode {
            op,
            inputs: ports(inputs),
            outputs: ports(&["out"]),
            params: params
                .iter()
                .map(|name| ParamDesc {
                    name: (*name).into(),
                    value_type: if matches!(*name, "width" | "height") {
                        ValueType::Int
                    } else {
                        ValueType::F64
                    },
                })
                .collect(),
        }))?;
    }
    Ok(())
}

impl NodeKind for ImageNode {
    fn type_name(&self) -> &str {
        IMAGE_TYPES[self.op]
    }
    fn inputs(&self) -> &[PortDesc] {
        &self.inputs
    }
    fn outputs(&self) -> &[PortDesc] {
        &self.outputs
    }
    fn params(&self) -> &[ParamDesc] {
        &self.params
    }
    fn eval(&self, params: &[Value], inputs: &[Value]) -> Result<Vec<Value>, NodeError> {
        check_args(&self.params, &self.inputs, params, inputs)?;
        let number = |index: usize| match params[index] {
            Value::F64(v) => v as f32,
            _ => unreachable!("validated parameter"),
        };
        if self.op == 7 {
            if let Value::F64(t) = params[0] {
                if !(0.0..=1.0).contains(&t) {
                    return Err(error(BLEND_FACTOR_RANGE));
                }
            }
        }
        let (width, height) = if self.op <= 1 {
            let side = |index: usize| match params[index] {
                Value::Int(v) => v as u32,
                _ => unreachable!("validated dimension"),
            };
            (side(0), side(1))
        } else {
            match &inputs[0] {
                Value::Image(a) => (a.width, a.height),
                _ => unreachable!("validated image"),
            }
        };
        let w = usize::try_from(width).map_err(|_| error(IMAGE_SIZE_RANGE))?;
        let h = usize::try_from(height).map_err(|_| error(IMAGE_SIZE_RANGE))?;
        let count = w.checked_mul(h).ok_or_else(|| error(IMAGE_SIZE_RANGE))?;
        let image = |index: usize| match &inputs[index] {
            Value::Image(a) => a,
            _ => unreachable!("validated image"),
        };
        let mut pixels = Vec::with_capacity(count);
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let p = if self.op > 1 { image(0).pixels[i] } else { 0.0 };
                let value = match self.op {
                    0 => number(2),
                    1 => (2 * x + 1) as f32 / (2 * w) as f32,
                    2 => 1.0 - p,
                    3 => p + image(1).pixels[i],
                    4 => p * image(1).pixels[i],
                    5 => {
                        if p >= number(0) {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    6 => {
                        let mut sum = 0.0_f32;
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let xx = x.saturating_add_signed(dx).min(w - 1);
                                let yy = y.saturating_add_signed(dy).min(h - 1);
                                sum += image(0).pixels[yy * w + xx];
                            }
                        }
                        sum / 9.0
                    }
                    7 => p * (1.0 - number(0)) + image(1).pixels[i] * number(0),
                    _ => unreachable!("registered operation"),
                };
                pixels.push(value);
            }
        }
        result(Value::Image(ImageBuf {
            width,
            height,
            pixels,
        }))
    }
}
