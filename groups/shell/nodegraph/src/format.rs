//! Canonical schema 1 graph text; see `docs/FORMAT.md`.

use crate::graph::{Graph, GraphError, NodeId, PortDir, Position};
use crate::kind::KindRegistry;
use crate::value::{Value, ValueType};

/// The schema this module writes and the only one it reads.
pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormatError {
    MissingFinalNewline,
    CarriageReturn {
        line: usize,
    },
    EmptyLine {
        line: usize,
    },
    UnknownVersion {
        found: String,
    },
    BadHeader {
        line: usize,
    },
    BadField {
        line: usize,
        field: &'static str,
    },
    BadName {
        line: usize,
        text: String,
    },
    Unsupported {
        line: usize,
        param: String,
    },
    NoExports,
    Graph {
        line: usize,
        error: GraphError,
    },
    NotCanonical {
        line: usize,
    },
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FormatError {}

/// Shared text rule, including relative asset names/paths; empty text is allowed.
pub fn text_allowed(text: &str) -> bool {
    !text
        .chars()
        .any(|c| matches!(c, ',' | ';' | '\\') || c.is_control())
        && !text
            .as_bytes()
            .windows(2)
            .any(|pair| pair[0].is_ascii_alphabetic() && pair[1] == b':')
}

fn check_text(text: &str, line: usize) -> Result<(), FormatError> {
    if text_allowed(text) {
        Ok(())
    } else {
        Err(FormatError::BadName {
            line,
            text: text.into(),
        })
    }
}

fn check_name(text: &str, line: usize) -> Result<(), FormatError> {
    check_text(text, line)?;
    if text.is_empty() || text.contains('=') {
        return Err(FormatError::BadName {
            line,
            text: text.into(),
        });
    }
    Ok(())
}

/// Canonical graph text. Refuses names and values that cannot be read back.
pub fn write(graph: &Graph) -> Result<String, FormatError> {
    let subgraph = !graph.exports().is_empty();
    let kind = if subgraph { "subgraph" } else { "graph" };
    let mut out = format!("schema={SCHEMA}\nkind={kind}\nnodes\n");
    let mut line = 4;
    for (id, node) in graph.nodes() {
        check_name(&node.type_name, line)?;
        let mut params = Vec::new();
        for (name, value) in &node.params {
            check_name(name, line)?;
            let text = match value {
                Value::Bool(v) => v.to_string(),
                Value::Int(v) => v.to_string(),
                Value::F64(v) => v.to_string(),
                Value::Text(v) => {
                    check_text(v, line)?;
                    v.clone()
                }
                Value::Image(_) => {
                    return Err(FormatError::Unsupported {
                        line,
                        param: name.clone(),
                    });
                }
            };
            params.push(format!("{name}={text}"));
        }
        out.push_str(&format!(
            "{},{},{},{},{}\n",
            id.0,
            node.type_name,
            node.position.x,
            node.position.y,
            params.join(";")
        ));
        line += 1;
    }
    out.push_str("edges\n");
    line += 1;
    for edge in graph.edges() {
        check_name(&edge.from_port, line)?;
        check_name(&edge.to_port, line)?;
        out.push_str(&format!(
            "{}.{},{}.{}\n",
            edge.from.0, edge.from_port, edge.to.0, edge.to_port
        ));
        line += 1;
    }
    if subgraph {
        out.push_str("exports\n");
        line += 1;
        for (name, export) in graph.exports() {
            check_name(name, line)?;
            check_name(&export.port, line)?;
            let dir = match export.dir {
                PortDir::Input => "in",
                PortDir::Output => "out",
            };
            out.push_str(&format!("{name},{}.{},{dir}\n", export.node.0, export.port));
            line += 1;
        }
    }
    Ok(out)
}

fn number<T: std::str::FromStr>(
    text: &str,
    line: usize,
    field: &'static str,
) -> Result<T, FormatError> {
    text.parse().map_err(|_| FormatError::BadField { line, field })
}

fn fields(text: &str, count: usize, line: usize) -> Result<Vec<&str>, FormatError> {
    let parts: Vec<_> = text.splitn(count + 1, ',').collect();
    if parts.len() != count {
        return Err(FormatError::BadField {
            line,
            field: "fields",
        });
    }
    Ok(parts)
}

fn graph_error(line: usize) -> impl FnOnce(GraphError) -> FormatError {
    move |error| FormatError::Graph { line, error }
}

fn parse_node(
    text: &str,
    line: usize,
    registry: &KindRegistry,
    graph: &mut Graph,
) -> Result<(), FormatError> {
    let cols = fields(text, 5, line)?;
    let id = NodeId(number(cols[0], line, "id")?);
    let position = Position {
        x: number(cols[2], line, "x")?,
        y: number(cols[3], line, "y")?,
    };
    let kind = registry.lookup(cols[1]).map_err(|error| FormatError::Graph {
        line,
        error: GraphError::Kind(error),
    })?;
    let bad_param = || FormatError::BadField {
        line,
        field: "param",
    };
    if (kind.params().is_empty() && !cols[4].is_empty())
        || (!kind.params().is_empty() && cols[4].split(';').count() != kind.params().len())
    {
        return Err(bad_param());
    }
    let mut parts = cols[4].split(';');
    let mut values = Vec::new();
    for desc in kind.params() {
        let (name, text) = parts
            .next()
            .and_then(|part| part.split_once('='))
            .ok_or_else(bad_param)?;
        if name != desc.name {
            return Err(bad_param());
        }
        values.push(match &desc.value_type {
            ValueType::Bool => Value::Bool(number(text, line, "value")?),
            ValueType::Int => Value::Int(number(text, line, "value")?),
            ValueType::F64 => Value::F64(number(text, line, "value")?),
            ValueType::Text => {
                check_text(text, line)?;
                Value::Text(text.into())
            }
            ValueType::Image | ValueType::Named(_) => {
                return Err(FormatError::Unsupported {
                    line,
                    param: name.into(),
                });
            }
        });
    }
    graph
        .add_node_with_id(registry, id, cols[1], values, position)
        .map_err(graph_error(line))
}

fn endpoint(text: &str, line: usize) -> Result<(NodeId, &str), FormatError> {
    let (id, port) = text.split_once('.').ok_or(FormatError::BadField {
        line,
        field: "edge",
    })?;
    Ok((NodeId(number(id, line, "id")?), port))
}

#[derive(Clone, Copy)]
enum Section {
    Nodes,
    Edges,
    Exports,
}

/// Read atomically, resolve kinds through `registry`, then enforce canonical text.
pub fn parse(text: &str, registry: &KindRegistry) -> Result<Graph, FormatError> {
    if !text.ends_with('\n') {
        return Err(FormatError::MissingFinalNewline);
    }
    if let Some((index, _)) = text
        .split_terminator('\n')
        .enumerate()
        .find(|(_, line)| line.contains('\r'))
    {
        return Err(FormatError::CarriageReturn { line: index + 1 });
    }
    let mut lines = text.split_terminator('\n');
    let version = lines
        .next()
        .and_then(|line| line.strip_prefix("schema="))
        .ok_or(FormatError::BadHeader { line: 1 })?;
    if version != SCHEMA.to_string() {
        return Err(FormatError::UnknownVersion {
            found: version.into(),
        });
    }
    let subgraph = match lines.next() {
        Some("kind=graph") => false,
        Some("kind=subgraph") => true,
        _ => return Err(FormatError::BadHeader { line: 2 }),
    };
    match lines.next() {
        Some("nodes") => {}
        Some("") => return Err(FormatError::EmptyLine { line: 3 }),
        _ => return Err(FormatError::BadHeader { line: 3 }),
    }
    let mut graph = Graph::new();
    let mut section = Section::Nodes;
    let mut last_line = 3;
    for (index, text) in lines.enumerate() {
        let line = index + 4;
        last_line = line;
        if text.is_empty() {
            return Err(FormatError::EmptyLine { line });
        }
        match (section, text) {
            (Section::Nodes, "edges") => {
                section = Section::Edges;
                continue;
            }
            (Section::Edges, "exports") if subgraph => {
                section = Section::Exports;
                continue;
            }
            (_, "nodes" | "edges" | "exports") => {
                return Err(FormatError::BadHeader { line });
            }
            _ if text.starts_with("schema=") || text.starts_with("kind=") => {
                return Err(FormatError::BadHeader { line });
            }
            _ => {}
        }
        match section {
            Section::Nodes => parse_node(text, line, registry, &mut graph)?,
            Section::Edges => {
                let cols = fields(text, 2, line)?;
                let (from, from_port) = endpoint(cols[0], line)?;
                let (to, to_port) = endpoint(cols[1], line)?;
                graph
                    .connect(from, from_port, to, to_port)
                    .map_err(graph_error(line))?;
            }
            Section::Exports => {
                let cols = fields(text, 3, line)?;
                check_name(cols[0], line)?;
                let (node, port) = endpoint(cols[1], line)?;
                let dir = match cols[2] {
                    "in" => PortDir::Input,
                    "out" => PortDir::Output,
                    _ => return Err(FormatError::BadField { line, field: "dir" }),
                };
                graph
                    .export(cols[0], node, port, dir)
                    .map_err(graph_error(line))?;
            }
        }
    }
    if matches!(section, Section::Nodes) || (subgraph && !matches!(section, Section::Exports)) {
        return Err(FormatError::BadHeader {
            line: last_line + 1,
        });
    }
    if subgraph && graph.exports().is_empty() {
        return Err(FormatError::NoExports);
    }
    let canonical = write(&graph)?;
    if canonical != text {
        let line = canonical
            .split_terminator('\n')
            .zip(text.split_terminator('\n'))
            .position(|(a, b)| a != b)
            .map_or_else(
                || canonical.lines().count().min(text.lines().count()) + 1,
                |index| index + 1,
            );
        return Err(FormatError::NotCanonical { line });
    }
    Ok(graph)
}
