//! Checks n2/n3/n4: independent call counts and values for two 500-node shapes.
//! The real --time budget is for a quiet board only, not a published benchmark.
//! The n5 oracle omits multiply/divide/clamp to avoid in-call arithmetic errors.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;
use std::time::Instant;

use nodegraph::eval::{EvalError, Evaluator};
use nodegraph::graph::{Graph, NodeId, Position};
use nodegraph::kind::{KindRegistry, NodeError, NodeKind, ParamDesc, PortDesc};
use nodegraph::kinds::register_all;
use nodegraph::value::{Value, ValueType};

const CHAINS: usize = 50; // Section 5: fifty independent chains in shape A.
const CHAIN_LEN: usize = 10; // One head and nine additions per chain.
const LAYERS: usize = 5; // Section 5: five layers in shape B.
const WIDTH: usize = 100; // Each layer contains one hundred nodes.
const REPEATS: usize = 11; // An odd sample count gives an unambiguous median.
const NODES: usize = CHAINS * CHAIN_LEN;
const ORIGIN: Position = Position { x: 0.0, y: 0.0 };

#[derive(Debug, PartialEq, Eq)]
struct Args {
    force_full: bool,
    time_budget_ms: Option<u64>,
}

fn collect_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Vec<String>, String> {
    args.into_iter()
        .map(|arg| arg.into_string().map_err(|_| "non-UTF-8 argument".to_owned()))
        .collect()
}

fn parse_args(args: &[String]) -> Result<Args, String> {
    let (mut force_full, mut time, mut budget) = (false, false, None);
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force-full" if !force_full => force_full = true,
            "--time" if !time => time = true,
            "--max-incremental-ms" if budget.is_none() => {
                let text = args.next().ok_or("missing --max-incremental-ms value")?;
                if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
                    return Err("non-digit --max-incremental-ms value".into());
                }
                let value = text
                    .parse::<u64>()
                    .map_err(|_| "overflow in --max-incremental-ms value")?;
                if !(1..=10000).contains(&value) {
                    return Err("--max-incremental-ms outside 1..=10000".into());
                }
                budget = Some(value);
            }
            "--force-full" | "--time" | "--max-incremental-ms" => {
                return Err(format!("repeated flag {arg}"));
            }
            _ => return Err(format!("unknown flag {arg:?}")),
        }
    }
    if time != budget.is_some() {
        return Err("--time and --max-incremental-ms must occur together".into());
    }
    if time && force_full {
        return Err("--time cannot be combined with --force-full".into());
    }
    Ok(Args {
        force_full,
        time_budget_ms: budget,
    })
}

struct Counted {
    name: &'static str,
    inner: Rc<KindRegistry>,
    counter: Rc<Cell<u64>>,
    inputs: Vec<PortDesc>,
    outputs: Vec<PortDesc>,
    params: Vec<ParamDesc>,
}

impl NodeKind for Counted {
    fn type_name(&self) -> &str {
        self.name
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
        self.counter.set(self.counter.get() + 1);
        if self.name != "bench.add_constant" {
            return self.inner.lookup(self.name)?.eval(params, inputs);
        }
        match (params, inputs) {
            ([Value::F64(k)], [Value::F64(a)]) if k.is_finite() && a.is_finite() => {
                let sum = a + k;
                if sum.is_finite() {
                    Ok(vec![Value::F64(sum)])
                } else {
                    Err(NodeError::Evaluation(
                        "non-finite add-constant result".into(),
                    ))
                }
            }
            _ => Err(NodeError::Evaluation(
                "invalid add-constant arguments".into(),
            )),
        }
    }
}

fn registry() -> (KindRegistry, Rc<Cell<u64>>) {
    let mut inner = KindRegistry::new();
    register_all(&mut inner).expect("unique built-in kinds");
    let inner = Rc::new(inner);
    let counter = Rc::new(Cell::new(0));
    let mut registry = KindRegistry::new();
    for name in ["math.constant", "math.add", "bench.add_constant"] {
        let (inputs, outputs, params) = if name == "bench.add_constant" {
            let port = |name: &str| PortDesc {
                name: name.into(),
                value_type: ValueType::F64,
            };
            (
                vec![port("a")],
                vec![port("out")],
                vec![ParamDesc {
                    name: "k".into(),
                    value_type: ValueType::F64,
                }],
            )
        } else {
            let kind = inner.lookup(name).expect("built-in kind");
            (
                kind.inputs().to_vec(),
                kind.outputs().to_vec(),
                kind.params().to_vec(),
            )
        };
        registry
            .add(Box::new(Counted {
                name,
                inner: Rc::clone(&inner),
                counter: Rc::clone(&counter),
                inputs,
                outputs,
                params,
            }))
            .expect("unique counted kinds");
    }
    (registry, counter)
}

struct Shape {
    name: char,
    graph: Graph,
    outputs: Vec<NodeId>,
    edited: NodeId,
    edit_value: f64,
    adjacency: BTreeMap<NodeId, Vec<NodeId>>,
}

fn add(g: &mut Graph, r: &KindRegistry, kind: &str, value: Option<f64>) -> NodeId {
    let params = value.into_iter().map(Value::F64).collect();
    g.add_node(r, kind, params, ORIGIN)
        .expect("valid fixture node")
}

fn wire(shape: &mut Shape, from: NodeId, to: NodeId, port: &str) {
    shape
        .graph
        .connect(from, "out", to, port)
        .expect("valid fixture edge");
    shape.adjacency.entry(from).or_default().push(to);
}

fn build_a(r: &KindRegistry) -> Shape {
    let mut shape = Shape {
        name: 'A',
        graph: Graph::new(),
        outputs: Vec::new(),
        edited: NodeId(0),
        edit_value: 100.0,
        adjacency: BTreeMap::new(),
    };
    for c in 0..CHAINS {
        let mut tail = add(&mut shape.graph, r, "math.constant", Some(c as f64));
        if c == 7 {
            shape.edited = tail;
        }
        for _ in 1..CHAIN_LEN {
            let next = add(&mut shape.graph, r, "bench.add_constant", Some(1.0));
            wire(&mut shape, tail, next, "a");
            tail = next;
        }
        shape.outputs.push(tail);
    }
    shape
}

fn build_b(r: &KindRegistry) -> Shape {
    let mut shape = Shape {
        name: 'B',
        graph: Graph::new(),
        outputs: Vec::new(),
        edited: NodeId(0),
        edit_value: 1000.0,
        adjacency: BTreeMap::new(),
    };
    let mut previous: Vec<_> = (0..WIDTH)
        .map(|i| add(&mut shape.graph, r, "math.constant", Some(i as f64)))
        .collect();
    shape.edited = previous[0];
    for _ in 1..LAYERS {
        let mut next = Vec::new();
        for i in 0..WIDTH {
            let id = add(&mut shape.graph, r, "math.add", None);
            wire(&mut shape, previous[i], id, "a");
            wire(&mut shape, previous[(i + 1) % WIDTH], id, "b");
            next.push(id);
        }
        previous = next;
    }
    shape.outputs = previous;
    shape
}

fn descendant_count(shape: &Shape) -> usize {
    let mut seen = BTreeSet::from([shape.edited]);
    let mut queue = VecDeque::from([shape.edited]);
    while let Some(id) = queue.pop_front() {
        if let Some(readers) = shape.adjacency.get(&id) {
            for &reader in readers {
                if seen.insert(reader) {
                    queue.push_back(reader);
                }
            }
        }
    }
    seen.len()
}

// All fixture arithmetic uses small integer-valued F64s, compared exactly.
fn expected_values(shape: &Shape, edited: bool) -> Vec<f64> {
    if shape.name == 'A' {
        let mut values = [0.0; CHAINS];
        for (c, value) in values.iter_mut().enumerate() {
            *value = if edited && c == 7 {
                shape.edit_value
            } else {
                c as f64
            };
            for _ in 1..CHAIN_LEN {
                *value += 1.0;
            }
        }
        values.to_vec()
    } else {
        let mut values = std::array::from_fn::<_, WIDTH, _>(|i| i as f64);
        if edited {
            values[0] = shape.edit_value;
        }
        for _ in 1..LAYERS {
            values = std::array::from_fn(|i| values[i] + values[(i + 1) % WIDTH]);
        }
        values.to_vec()
    }
}

struct Pass {
    calls: u64,
    kind_calls: u64,
    values: Vec<Result<Value, EvalError>>,
}

fn demand(shape: &Shape, r: &KindRegistry, counter: &Cell<u64>, e: &mut Evaluator) -> Pass {
    let (before, kind_before) = (e.stats().calls, counter.get());
    let values = shape
        .outputs
        .iter()
        .map(|&id| e.evaluate(&shape.graph, r, (id, "out")))
        .collect();
    Pass {
        calls: e.stats().calls - before,
        kind_calls: counter.get() - kind_before,
        values,
    }
}

fn check_pass(
    shape: &Shape,
    pass: &Pass,
    number: usize,
    expected: usize,
    edited: bool,
) -> Option<String> {
    let prefix = format!("bench500: FAIL shape={}", shape.name);
    if pass.calls != expected as u64 {
        let label = ["first", "after_edit", "third"][number - 1];
        return Some(format!(
            "{prefix} {label}={} expected={expected}",
            pass.calls
        ));
    }
    for (i, (got, want)) in pass
        .values
        .iter()
        .zip(expected_values(shape, edited))
        .enumerate()
    {
        if got != &Ok(Value::F64(want)) {
            let node = if shape.name == 'A' {
                format!("{i}.{}", CHAIN_LEN - 1)
            } else {
                format!("{}.{i}", LAYERS - 1)
            };
            let got = match got {
                Ok(Value::F64(value)) => value.to_string(),
                other => format!("{other:?}"),
            };
            return Some(format!(
                "{prefix} value pass={number} node={node} got={got} want={want}"
            ));
        }
    }
    if pass.calls != pass.kind_calls {
        return Some(format!(
            "{prefix} counters stats={} kind={}",
            pass.calls, pass.kind_calls
        ));
    }
    None
}

fn run_shape(
    shape: &mut Shape,
    r: &KindRegistry,
    counter: &Cell<u64>,
    force_full: bool,
) -> (String, bool) {
    let mut evaluator = Evaluator::new();
    evaluator.set_force_full(force_full);
    let expected = descendant_count(shape);
    let mut counts = Vec::new();
    for (index, expected_calls) in [NODES, expected, 0].into_iter().enumerate() {
        if index == 1 {
            let edit = shape
                .graph
                .set_param(shape.edited, "value", Value::F64(shape.edit_value));
            if let Err(error) = edit {
                return (
                    format!("bench500: FAIL shape={} edit={error}", shape.name),
                    false,
                );
            }
        }
        let pass = demand(shape, r, counter, &mut evaluator);
        if let Some(line) = check_pass(shape, &pass, index + 1, expected_calls, index != 0) {
            return (line, false);
        }
        counts.push(pass.calls);
    }
    (
        format!(
            "bench500: ok shape={} nodes={NODES} first={} after_edit={} expected={expected}",
            shape.name, counts[0], counts[1]
        ),
        true,
    )
}

fn time_shape(r: &KindRegistry, counter: &Cell<u64>, budget: u64) -> (String, bool) {
    let mut shape = build_a(r);
    let mut evaluator = Evaluator::new();
    let warm = demand(&shape, r, counter, &mut evaluator);
    if let Some(line) = check_pass(&shape, &warm, 1, NODES, false) {
        return (line, false);
    }
    let mut incremental = Vec::new();
    let mut full = Vec::new();
    for sample in 0..REPEATS {
        let edited = sample % 2 == 0;
        let value = if edited { shape.edit_value } else { 7.0 };
        let edit = shape
            .graph
            .set_param(shape.edited, "value", Value::F64(value));
        if let Err(error) = edit {
            return (format!("bench500: FAIL shape=A edit={error}"), false);
        }
        let start = Instant::now();
        let pass = demand(&shape, r, counter, &mut evaluator);
        incremental.push(start.elapsed().as_nanos());
        if let Some(line) = check_pass(&shape, &pass, 2, descendant_count(&shape), edited) {
            return (line, false);
        }
    }
    for _ in 0..REPEATS {
        let mut fresh = Evaluator::new();
        let start = Instant::now();
        let pass = demand(&shape, r, counter, &mut fresh);
        full.push(start.elapsed().as_nanos());
        if let Some(line) = check_pass(&shape, &pass, 1, NODES, true) {
            return (line, false);
        }
    }
    incremental.sort_unstable();
    full.sort_unstable();
    let i = incremental[REPEATS / 2].div_ceil(1000);
    let f = full[REPEATS / 2].div_ceil(1000);
    let reason = if i > u128::from(budget) * 1000 {
        Some("budget")
    } else if 2 * i > f {
        Some("ratio")
    } else {
        None
    };
    let status = if reason.is_some() { "FAIL" } else { "ok" };
    let mut line = format!(
        "bench500: {status} time shape=A incremental_us={i} full_us={f} \
         max_incremental_ms={budget}"
    );
    if let Some(reason) = reason {
        line.push_str(&format!(" reason={reason}"));
    }
    (line, reason.is_none())
}

fn main() {
    let args = match collect_args(std::env::args_os().skip(1)).and_then(|a| parse_args(&a)) {
        Ok(args) => args,
        Err(reason) => {
            println!("bench500: refused {reason}");
            std::process::exit(2);
        }
    };
    let (r, counter) = registry();
    for mut shape in [build_a(&r), build_b(&r)] {
        let (line, ok) = run_shape(&mut shape, &r, &counter, args.force_full);
        println!("{line}");
        if !ok {
            std::process::exit(1);
        }
    }
    if let Some(budget) = args.time_budget_ms {
        let (line, ok) = time_shape(&r, &counter, budget);
        println!("{line}");
        if !ok {
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(flags: &str) -> Result<Args, String> {
        let args: Vec<_> = flags.split_whitespace().map(str::to_owned).collect();
        parse_args(&args)
    }

    #[test]
    fn parse_args_refuses_bad_input() {
        let expected = |force_full, time_budget_ms| {
            Ok(Args {
                force_full,
                time_budget_ms,
            })
        };
        assert_eq!(parse(""), expected(false, None));
        for flags in [
            "--time --max-incremental-ms 50",
            "--max-incremental-ms 50 --time",
        ] {
            assert_eq!(parse(flags), expected(false, Some(50)));
        }
        assert_eq!(parse("--force-full"), expected(true, None));
        for value in ["1", "10000"] {
            let flags = format!("--time --max-incremental-ms {value}");
            assert_eq!(parse(&flags), expected(false, Some(value.parse().unwrap())));
        }
        for flags in [
            "--time",
            "--max-incremental-ms 50",
            "--time --max-incremental-ms 0",
            "--time --max-incremental-ms 10001",
            "--time --max-incremental-ms +5",
            "--time --max-incremental-ms -5",
            "--time --max-incremental-ms 18446744073709551616",
            "--time --max-incremental-ms",
            "--force-full --force-full",
            "--force-full --time --max-incremental-ms 50",
            "--clips 5",
            "--time --time --max-incremental-ms 50",
            "--time --max-incremental-ms 50 --max-incremental-ms 50",
        ] {
            assert!(parse(flags).is_err(), "{flags}");
        }
        for value in [
            "", " 5", "5 ", "5\r", "5\n", "\u{ff15}", "1.0", "NaN", "C:\\5",
        ] {
            let mut flags = vec!["--time".into(), "--max-incremental-ms".into()];
            flags.push(value.into());
            assert!(parse_args(&flags).is_err(), "{value:?}");
        }
    }

    #[test]
    fn shapes_have_500_nodes_and_hand_counts() {
        let (r, _) = registry();
        for (shape, descendants, edges) in [(build_a(&r), 10, 450), (build_b(&r), 15, 800)] {
            assert_eq!(shape.graph.node_count(), 500);
            assert_eq!(descendant_count(&shape), descendants);
            assert_eq!(shape.graph.edges().count(), edges);
        }
    }

    #[test]
    fn run_shape_counts_and_force_full_fails() {
        let (r, counter) = registry();
        let ok_a = "bench500: ok shape=A nodes=500 first=500 after_edit=10 expected=10";
        let ok_b = "bench500: ok shape=B nodes=500 first=500 after_edit=15 expected=15";
        let fail = "bench500: FAIL shape=A after_edit=500 expected=10";
        assert_eq!(
            run_shape(&mut build_a(&r), &r, &counter, false),
            (ok_a.into(), true)
        );
        assert_eq!(
            run_shape(&mut build_b(&r), &r, &counter, false),
            (ok_b.into(), true)
        );
        assert_eq!(
            run_shape(&mut build_a(&r), &r, &counter, true),
            (fail.into(), false)
        );
        // A separate, untouched Cell must be rejected, not silently ignored.
        assert_eq!(
            run_shape(&mut build_a(&r), &r, &Cell::new(0), false),
            (
                "bench500: FAIL shape=A counters stats=500 kind=0".into(),
                false
            )
        );
    }

    #[test]
    fn collect_args_refuses_non_utf8() {
        use std::ffi::OsString;
        let valid = ["--time", "--max-incremental-ms", "50"];
        assert_eq!(
            collect_args(valid.map(OsString::from)),
            Ok(valid.map(str::to_owned).to_vec())
        );
        assert_eq!(collect_args(Vec::<OsString>::new()), Ok(vec![]));
        assert_eq!(
            collect_args(["\u{ff15}"].map(OsString::from)),
            Ok(vec!["\u{ff15}".to_owned()])
        );
        #[cfg(windows)]
        let bad = <OsString as std::os::windows::ffi::OsStringExt>::from_wide(&[0x0035, 0xD800]);
        #[cfg(unix)]
        let bad = <OsString as std::os::unix::ffi::OsStringExt>::from_vec(vec![0x35, 0xFF]);
        #[cfg(any(windows, unix))]
        for args in [
            vec![bad.clone()],
            vec![OsString::from("--time"), bad.clone()],
            vec![
                OsString::from("--time"),
                OsString::from("--max-incremental-ms"),
                bad,
            ],
        ] {
            assert_eq!(collect_args(args), Err("non-UTF-8 argument".to_owned()));
        }
    }
}
