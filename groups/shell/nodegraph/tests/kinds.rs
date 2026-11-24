use nodegraph::kind::{KindRegistry, NodeError, ParamDesc, PortDesc};
use nodegraph::kinds::image::IMAGE_TYPES;
use nodegraph::kinds::math::MATH_TYPES;
use nodegraph::kinds::{
    register_all, BAD_ARGUMENTS, BAD_IMAGE, BLEND_FACTOR_RANGE, CLAMP_RANGE, DIVIDE_BY_ZERO,
    IMAGE_SIZE_MISMATCH, IMAGE_SIZE_RANGE, NOT_FINITE_INPUT, NOT_FINITE_RESULT,
};
use nodegraph::value::{ImageBuf, Value, ValueType};

fn registry() -> KindRegistry {
    let mut registry = KindRegistry::new();
    register_all(&mut registry).expect("register built-ins");
    registry
}

fn eval(r: &KindRegistry, name: &str, p: &[Value], i: &[Value]) -> Result<Vec<Value>, NodeError> {
    r.lookup(name)?.eval(p, i)
}

fn error(message: &str) -> Result<Vec<Value>, NodeError> {
    Err(NodeError::Evaluation(message.to_owned()))
}

fn image(width: u32, height: u32, pixels: Vec<f32>) -> Value {
    Value::Image(ImageBuf {
        width,
        height,
        pixels,
    })
}

fn solid(width: u32, height: u32, value: f32) -> Value {
    image(width, height, vec![value; (width * height) as usize])
}

fn rows(row: &[f32; 8]) -> Value {
    image(8, 8, row.repeat(8))
}

#[test]
fn math_nodes_reference_values() {
    let r = registry();
    assert_eq!(
        eval(&r, "math.constant", &[Value::F64(1.25)], &[]),
        Ok(vec![Value::F64(1.25)])
    );
    for (name, a, b, expected) in [
        ("math.add", 2.5, 4.0, 6.5),
        ("math.subtract", 2.5, 4.0, -1.5),
        ("math.subtract", 4.0, 2.5, 1.5),
        ("math.multiply", 2.5, 4.0, 10.0),
        ("math.divide", 10.0, 4.0, 2.5),
        ("math.min", 2.5, 4.0, 2.5),
        ("math.max", 2.5, 4.0, 4.0),
    ] {
        assert_eq!(
            eval(&r, name, &[], &[Value::F64(a), Value::F64(b)]),
            Ok(vec![Value::F64(expected)]),
            "{name}"
        );
    }
    assert_eq!(
        eval(&r, "math.negate", &[], &[Value::F64(2.5)]),
        Ok(vec![Value::F64(-2.5)])
    );
    for (x, lo, hi, expected) in [
        (7.0, 0.0, 5.0, 5.0),
        (-1.0, 0.0, 5.0, 0.0),
        (3.0, 0.0, 5.0, 3.0),
        (7.0, 2.0, 2.0, 2.0),
    ] {
        assert_eq!(
            eval(
                &r,
                "math.clamp",
                &[Value::F64(lo), Value::F64(hi)],
                &[Value::F64(x)]
            ),
            Ok(vec![Value::F64(expected)])
        );
    }
    // Exact binary references, including subnormal, underflow and cancellation.
    for (name, a, b, expected) in [
        ("math.add", f64::from_bits(1), 0.0, f64::from_bits(1)),
        ("math.multiply", f64::from_bits(1), 0.5, 0.0),
        ("math.add", f64::MAX, -f64::MAX, 0.0),
        ("math.subtract", 1.0000000000000002, 1.0, f64::EPSILON),
        ("math.multiply", f64::MAX, 1.0, f64::MAX),
    ] {
        assert_eq!(
            eval(&r, name, &[], &[Value::F64(a), Value::F64(b)]),
            Ok(vec![Value::F64(expected)])
        );
    }
}

#[test]
fn math_divide_by_zero_is_named_error() {
    let r = registry();
    for (a, b) in [(1.0, 0.0), (1.0, -0.0), (0.0, 0.0)] {
        assert_eq!(
            eval(&r, "math.divide", &[], &[Value::F64(a), Value::F64(b)]),
            error(DIVIDE_BY_ZERO)
        );
    }
    assert_eq!(
        eval(&r, "math.divide", &[], &[Value::F64(-3.0), Value::F64(2.0)]),
        Ok(vec![Value::F64(-1.5)])
    );
}

#[test]
fn image_nodes_reference_values() {
    let r = registry();
    let g = rows(&[
        0.0625, 0.1875, 0.3125, 0.4375, 0.5625, 0.6875, 0.8125, 0.9375,
    ]);
    let inverted = rows(&[
        0.9375, 0.8125, 0.6875, 0.5625, 0.4375, 0.3125, 0.1875, 0.0625,
    ]);
    assert_eq!(
        eval(&r, "image.gradient", &[Value::Int(8), Value::Int(8)], &[]),
        Ok(vec![g.clone()])
    );
    assert_eq!(
        eval(
            &r,
            "image.solid",
            &[Value::Int(8), Value::Int(8), Value::F64(0.25)],
            &[]
        ),
        Ok(vec![solid(8, 8, 0.25)])
    );
    assert_eq!(
        eval(&r, "image.invert", &[], std::slice::from_ref(&g)),
        Ok(vec![inverted.clone()])
    );
    assert_eq!(
        eval(&r, "image.add", &[], &[g.clone(), inverted]),
        Ok(vec![solid(8, 8, 1.0)])
    );
    assert_eq!(
        eval(&r, "image.multiply", &[], &[solid(8, 8, 0.5), g.clone()]),
        Ok(vec![rows(&[
            0.03125, 0.09375, 0.15625, 0.21875, 0.28125, 0.34375, 0.40625, 0.46875,
        ])])
    );
    for level in [0.5, 0.5625] {
        assert_eq!(
            eval(
                &r,
                "image.threshold",
                &[Value::F64(level)],
                std::slice::from_ref(&g)
            ),
            Ok(vec![rows(&[0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])])
        );
    }
    for (factor, expected) in [(0.25, 0.25), (0.0, 0.0), (1.0, 1.0)] {
        assert_eq!(
            eval(
                &r,
                "image.blend",
                &[Value::F64(factor)],
                &[solid(8, 8, 0.0), solid(8, 8, 1.0)]
            ),
            Ok(vec![solid(8, 8, expected)])
        );
    }
    assert_eq!(
        eval(&r, "image.blur", &[], &[solid(8, 8, 0.5)]),
        Ok(vec![solid(8, 8, 0.5)])
    );
    let mut impulse = vec![0.0; 64];
    impulse[27] = 9.0;
    let mut expected = vec![0.0; 64];
    for i in [18, 19, 20, 26, 27, 28, 34, 35, 36] {
        expected[i] = 1.0;
    }
    assert_eq!(
        eval(&r, "image.blur", &[], &[image(8, 8, impulse)]),
        Ok(vec![image(8, 8, expected)])
    );
    let mut corner = vec![0.0; 64];
    corner[0] = 9.0;
    let mut expected = vec![0.0; 64];
    expected[0] = 4.0;
    expected[1] = 2.0;
    expected[8] = 2.0;
    expected[9] = 1.0;
    assert_eq!(
        eval(&r, "image.blur", &[], &[image(8, 8, corner)]),
        Ok(vec![image(8, 8, expected)])
    );
    assert_eq!(
        eval(&r, "image.gradient", &[Value::Int(1), Value::Int(1)], &[]),
        Ok(vec![solid(1, 1, 0.5)])
    );
    assert_eq!(
        eval(&r, "image.blur", &[], &[solid(1, 1, 0.5)]),
        Ok(vec![solid(1, 1, 0.5)])
    );
}

#[test]
fn image_size_mismatch_is_named_error() {
    let r = registry();
    for (a, b) in [
        (solid(8, 8, 0.0), solid(8, 7, 0.0)),
        (solid(8, 2, 0.0), solid(4, 4, 0.0)),
    ] {
        for inputs in [[a.clone(), b.clone()], [b, a]] {
            for name in ["image.add", "image.multiply", "image.blend"] {
                let params = if name == "image.blend" {
                    vec![Value::F64(0.5)]
                } else {
                    vec![]
                };
                assert_eq!(eval(&r, name, &params, &inputs), error(IMAGE_SIZE_MISMATCH));
            }
        }
    }
}

#[test]
fn kinds_refuse_nan_bad_arguments_and_bad_images() {
    let r = registry();
    for name in ["math.add", "math.min", "math.max"] {
        for inputs in [
            [Value::F64(f64::NAN), Value::F64(1.0)],
            [Value::F64(1.0), Value::F64(f64::NAN)],
        ] {
            assert_eq!(eval(&r, name, &[], &inputs), error(NOT_FINITE_INPUT));
        }
    }
    for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            eval(&r, "math.constant", &[Value::F64(v)], &[]),
            error(NOT_FINITE_INPUT)
        );
        assert_eq!(
            eval(&r, "math.negate", &[], &[Value::F64(v)]),
            error(NOT_FINITE_INPUT)
        );
        assert_eq!(
            eval(
                &r,
                "math.clamp",
                &[Value::F64(v), Value::F64(5.0)],
                &[Value::F64(0.0)]
            ),
            error(NOT_FINITE_INPUT)
        );
    }
    for name in ["image.invert", "image.blur"] {
        assert_eq!(
            eval(&r, name, &[], &[solid(8, 8, f32::NAN)]),
            error(NOT_FINITE_INPUT)
        );
    }
    assert_eq!(
        eval(
            &r,
            "image.solid",
            &[Value::Int(8), Value::Int(8), Value::F64(1e300)],
            &[]
        ),
        error(NOT_FINITE_INPUT)
    );
    for name in ["image.threshold", "image.blend"] {
        let inputs = if name == "image.blend" {
            vec![solid(1, 1, 0.0), solid(1, 1, 1.0)]
        } else {
            vec![solid(1, 1, 0.0)]
        };
        assert_eq!(
            eval(&r, name, &[Value::F64(1e300)], &inputs),
            error(NOT_FINITE_INPUT)
        );
    }
    assert_eq!(
        eval(
            &r,
            "math.add",
            &[],
            &[Value::F64(f64::MAX), Value::F64(f64::MAX)]
        ),
        error(NOT_FINITE_RESULT)
    );
    for name in ["image.add", "image.multiply"] {
        assert_eq!(
            eval(&r, name, &[], &[solid(2, 2, f32::MAX), solid(2, 2, f32::MAX)]),
            error(NOT_FINITE_RESULT)
        );
    }
    assert_eq!(
        eval(&r, "image.blur", &[], &[solid(2, 2, f32::MAX)]),
        error(NOT_FINITE_RESULT)
    );
    for inputs in [
        vec![Value::F64(1.0)],
        vec![Value::F64(1.0); 3],
        vec![Value::Int(1), Value::F64(1.0)],
    ] {
        assert_eq!(eval(&r, "math.add", &[], &inputs), error(BAD_ARGUMENTS));
    }
    assert_eq!(
        eval(
            &r,
            "math.add",
            &[Value::F64(0.0)],
            &[Value::F64(1.0), Value::F64(2.0)]
        ),
        error(BAD_ARGUMENTS)
    );
    for malformed in [
        image(2, 2, vec![0.0; 3]),
        image(0, 1, vec![]),
        image(u32::MAX, u32::MAX, vec![]),
    ] {
        assert_eq!(eval(&r, "image.invert", &[], &[malformed]), error(BAD_IMAGE));
    }
    for (w, h) in [(0, 8), (8, 4097), (-1, 8), (i64::MAX, 8)] {
        assert_eq!(
            eval(&r, "image.gradient", &[Value::Int(w), Value::Int(h)], &[]),
            error(IMAGE_SIZE_RANGE)
        );
    }
    assert_eq!(
        eval(
            &r,
            "math.clamp",
            &[Value::F64(3.0), Value::F64(1.0)],
            &[Value::F64(2.0)]
        ),
        error(CLAMP_RANGE)
    );
    for factor in [-0.25, 1.5, 1.0000000000000002] {
        assert_eq!(
            eval(
                &r,
                "image.blend",
                &[Value::F64(factor)],
                &[solid(8, 8, 0.0), solid(8, 8, 1.0)]
            ),
            error(BLEND_FACTOR_RANGE)
        );
    }
    assert_eq!(
        eval(
            &r,
            "image.add",
            &[],
            &[solid(8, 8, f32::NAN), solid(8, 7, 0.0)]
        ),
        error(NOT_FINITE_INPUT)
    );
    assert_eq!(
        eval(
            &r,
            "image.add",
            &[Value::F64(f64::NAN)],
            &[solid(8, 8, f32::NAN), solid(8, 7, 0.0)]
        ),
        error(BAD_ARGUMENTS)
    );
}

#[test]
fn register_all_adds_every_type_once() {
    let mut r = registry();
    for name in MATH_TYPES.into_iter().chain(IMAGE_TYPES) {
        let kind = r.lookup(name).expect("registered type");
        assert_eq!(kind.type_name(), name);
        let output_type = if name.starts_with("math.") {
            ValueType::F64
        } else {
            ValueType::Image
        };
        assert_eq!(
            kind.outputs(),
            &[PortDesc {
                name: "out".into(),
                value_type: output_type,
            }]
        );
    }
    let port = |name: &str, value_type| PortDesc {
        name: name.into(),
        value_type,
    };
    let param = |name: &str, value_type| ParamDesc {
        name: name.into(),
        value_type,
    };
    let clamp = r.lookup("math.clamp").expect("clamp");
    assert_eq!(clamp.inputs(), &[port("x", ValueType::F64)]);
    assert_eq!(
        clamp.params(),
        &[param("lo", ValueType::F64), param("hi", ValueType::F64)]
    );
    let blend = r.lookup("image.blend").expect("blend");
    assert_eq!(
        blend.inputs(),
        &[port("a", ValueType::Image), port("b", ValueType::Image)]
    );
    assert_eq!(blend.params(), &[param("factor", ValueType::F64)]);
    assert_eq!(
        r.lookup("image.solid").expect("solid").params(),
        &[
            param("width", ValueType::Int),
            param("height", ValueType::Int),
            param("value", ValueType::F64),
        ]
    );
    assert_eq!(
        register_all(&mut r),
        Err(NodeError::DuplicateType("math.constant".into()))
    );
    assert!(matches!(
        r.lookup("math.Add"),
        Err(NodeError::UnknownType(name)) if name == "math.Add"
    ));
}
