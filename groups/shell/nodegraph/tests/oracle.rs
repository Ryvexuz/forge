//! Check n5: seeded incremental-versus-fresh evaluation, including errors.
//! The n4 --time budget belongs on a quiet board, not in this property test.
//! Multiply/divide/clamp are omitted: overflow, zero divisors and reversed
//! bounds can fail inside a node call and invalidate the broken-node count rule.

use std::collections::{BTreeSet, VecDeque};

use nodegraph::eval::Evaluator;
use nodegraph::graph::{Graph, NodeId, Position};
use nodegraph::kind::KindRegistry;
use nodegraph::kinds::register_all;
use nodegraph::value::Value;

const SEED: u64 = 0x0017_2026_1124; // Plan seed 0x1_7_2026_1124, evenly grouped.
const CASES: usize = 2000;
const EDITS: usize = 20;
const MAX_NODES: usize = 60;
const ORIGIN: Position = Position { x: 0.0, y: 0.0 };
const OPS: [&str; 5] = [
    "math.add",
    "math.subtract",
    "math.negate",
    "math.min",
    "math.max",
];

struct Lcg(u64);

impl Lcg {
    fn draw(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 33) % bound as u64) as usize
    }

    fn value(&mut self) -> Value {
        Value::F64(self.draw(21) as f64 - 10.0)
    }
}

// At most sixty nodes, integer constants in [-10,10], and add/subtract,
// negate/min/max keep magnitudes below 10 * 2^60, hence finite even after
// arbitrary acyclic rewiring. Comparisons are exact Result/Value equality.
fn add_random(g: &mut Graph, r: &KindRegistry, rng: &mut Lcg, context: &str) -> NodeId {
    let earlier: Vec<_> = g.nodes().map(|(id, _)| id).collect();
    let kind = if earlier.is_empty() || rng.draw(3) == 0 {
        "math.constant"
    } else {
        OPS[rng.draw(OPS.len())]
    };
    let params = if kind == "math.constant" {
        vec![rng.value()]
    } else {
        vec![]
    };
    let id = g
        .add_node(r, kind, params, ORIGIN)
        .unwrap_or_else(|error| panic!("{context} add: {error}"));
    let ports: Vec<_> = g
        .node(id)
        .unwrap_or_else(|| panic!("{context} missing added node"))
        .inputs
        .iter()
        .map(|port| port.name.clone())
        .collect();
    for port in ports {
        let from = earlier[rng.draw(earlier.len())];
        if rng.draw(8) != 0 {
            g.connect(from, "out", id, &port)
                .unwrap_or_else(|error| panic!("{context} build connect: {error}"));
        }
    }
    id
}

// Independent breadth-first search: neither descendants() nor dirty_nodes()
// supplies the expected set.
fn readers(g: &Graph, target: NodeId) -> BTreeSet<NodeId> {
    let mut seen = BTreeSet::from([target]);
    let mut queue = VecDeque::from([target]);
    while let Some(id) = queue.pop_front() {
        for edge in g.edges().filter(|edge| edge.from == id) {
            if seen.insert(edge.to) {
                queue.push_back(edge.to);
            }
        }
    }
    seen
}

fn edit(
    g: &mut Graph,
    r: &KindRegistry,
    rng: &mut Lcg,
    stale: &mut BTreeSet<NodeId>,
    context: &str,
) {
    let before = g.clone();
    let old_stale = stale.clone();
    let mut changed = false;
    match rng.draw(5) {
        0 => {
            let constants: Vec<_> = g
                .nodes()
                .filter_map(|(id, node)| (node.type_name == "math.constant").then_some(id))
                .collect();
            if !constants.is_empty() {
                let target = constants[rng.draw(constants.len())];
                if g.set_param(target, "value", rng.value()).is_ok() {
                    stale.extend(readers(g, target));
                    changed = true;
                }
            }
        }
        1 => {
            let ids: Vec<_> = g.nodes().map(|(id, _)| id).collect();
            let mut inputs = Vec::new();
            for (id, node) in g.nodes() {
                for port in &node.inputs {
                    if g.source_of(id, &port.name).is_none() {
                        inputs.push((id, port.name.clone()));
                    }
                }
            }
            if !inputs.is_empty() && !ids.is_empty() {
                let (target, port) = &inputs[rng.draw(inputs.len())];
                let from = ids[rng.draw(ids.len())];
                if g.connect(from, "out", *target, port).is_ok() {
                    stale.extend(readers(g, *target));
                    changed = true;
                }
            }
        }
        2 => {
            let edges: Vec<_> = g.edges().cloned().collect();
            if !edges.is_empty() {
                let edge = &edges[rng.draw(edges.len())];
                if g.disconnect(edge.to, &edge.to_port).is_ok() {
                    stale.extend(readers(g, edge.to));
                    changed = true;
                }
            }
        }
        3 => {
            if g.node_count() < MAX_NODES {
                let id = add_random(g, r, rng, context);
                stale.insert(id);
                changed = true;
            }
        }
        _ => {
            let ids: Vec<_> = g.nodes().map(|(id, _)| id).collect();
            if !ids.is_empty() {
                let id = ids[rng.draw(ids.len())];
                if g.remove_node(id).is_ok() {
                    stale.remove(&id);
                    changed = true;
                }
            }
        }
    }
    if !changed {
        assert_eq!(
            *g, before,
            "{context} refused/no-candidate edit mutated graph"
        );
        assert_eq!(
            *stale, old_stale,
            "{context} refused/no-candidate edit mutated model"
        );
    }
}

fn pass(
    g: &Graph,
    r: &KindRegistry,
    inc: &mut Evaluator,
    stale: &mut BTreeSet<NodeId>,
    context: &str,
) {
    assert_eq!(inc.dirty_nodes(g), *stale, "{context} dirty set");
    let before = inc.stats().calls;
    let mut fresh = Evaluator::new();
    let mut broken = BTreeSet::new();
    for (id, _) in g.nodes() {
        let actual = inc.evaluate(g, r, (id, "out"));
        let expected = fresh.evaluate(g, r, (id, "out"));
        assert_eq!(actual, expected, "{context} node={id:?} result");
        if expected.is_err() {
            broken.insert(id);
        }
    }
    let expected_calls = stale.difference(&broken).count() as u64;
    assert_eq!(
        inc.stats().calls - before,
        expected_calls,
        "{context} call delta"
    );
    *stale = broken;
}

#[test]
fn incremental_equals_full_recompute() {
    for case in 0..CASES {
        let seed = SEED + case as u64;
        let context = format!("seed={seed} case={case} edit=initial cases={CASES}");
        let mut r = KindRegistry::new();
        register_all(&mut r).unwrap_or_else(|error| panic!("{context} registry: {error}"));
        let mut rng = Lcg(seed);
        let mut g = Graph::new();
        let count = 1 + rng.draw(MAX_NODES);
        for _ in 0..count {
            add_random(&mut g, &r, &mut rng, &context);
        }
        let mut inc = Evaluator::new();
        let mut stale = g.nodes().map(|(id, _)| id).collect();
        pass(&g, &r, &mut inc, &mut stale, &context);
        for index in 0..EDITS {
            let context = format!("seed={seed} case={case} edit={index} cases={CASES}");
            edit(&mut g, &r, &mut rng, &mut stale, &context);
            pass(&g, &r, &mut inc, &mut stale, &context);
        }
    }
}
