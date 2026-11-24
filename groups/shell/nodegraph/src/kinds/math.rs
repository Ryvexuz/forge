//! Scalar arithmetic with shared validation before any operation.

use crate::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use crate::value::{Value, ValueType};

use super::{check_args, error, result, BAD_ARGUMENTS, CLAMP_RANGE, DIVIDE_BY_ZERO};

pub const MATH_TYPES: [&str; 9] = [
    "math.constant",
    "math.add",
    "math.subtract",
    "math.multiply",
    "math.divide",
    "math.negate",
    "math.min",
    "math.max",
    "math.clamp",
];

struct MathNode {
    op: usize,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

pub fn register(registry: &mut KindRegistry) -> Result<(), NodeError> {
    for (op, _) in MATH_TYPES.iter().enumerate() {
        let (inputs, params): (&[&str], &[&str]) = match op {
            0 => (&[], &["value"]),
            5 => (&["a"], &[]),
            8 => (&["x"], &["lo", "hi"]),
            _ => (&["a", "b"], &[]),
        };
        let ports = |names: &[&str]| {
            names
                .iter()
                .map(|name| PortDesc {
                    name: (*name).into(),
                    value_type: ValueType::F64,
                })
                .collect()
        };
        registry.add(Box::new(MathNode {
            op,
            inputs: ports(inputs),
            outputs: ports(&["out"]),
            params: params
                .iter()
                .map(|name| ParamDesc {
                    name: (*name).into(),
                    value_type: ValueType::F64,
                })
                .collect(),
        }))?;
    }
    Ok(())
}

impl NodeKind for MathNode {
    fn type_name(&self) -> &str {
        MATH_TYPES[self.op]
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
        let scalar = |value: &Value| match value {
            Value::F64(v) => *v,
            _ => unreachable!("validated scalar"),
        };
        let a = inputs.first().map(scalar).unwrap_or(0.0);
        let b = inputs.get(1).map(scalar).unwrap_or(0.0);
        let value = match self.op {
            0 => scalar(&params[0]),
            1 => a + b,
            2 => a - b,
            3 => a * b,
            4 => {
                if b == 0.0 {
                    return Err(error(DIVIDE_BY_ZERO));
                }
                a / b
            }
            5 => -a,
            6 => a.min(b),
            7 => a.max(b),
            8 => {
                let lo = scalar(&params[0]);
                let hi = scalar(&params[1]);
                if lo > hi {
                    return Err(error(CLAMP_RANGE));
                }
                a.max(lo).min(hi)
            }
            _ => return Err(error(BAD_ARGUMENTS)),
        };
        result(Value::F64(value))
    }
}
