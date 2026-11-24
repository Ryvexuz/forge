# Node graph file format, schema 1

This document wins over the plan's draft paragraph.
A change to this format is a cross-shard change: bump `schema`.

## Layout

UTF-8 text, LF after every line including the last. No blank lines, comments,
CR bytes or escaping. The first line is `schema=1`; the second is `kind=graph`
or `kind=subgraph`. Next comes `nodes`, followed by zero or more node lines,
then `edges`, followed by zero or more edge lines. Only a subgraph has a final
`exports` section, which must contain at least one export line.
An empty graph is exactly `schema=1\nkind=graph\nnodes\nedges\n`.
The format functions consume and return strings; they do not access files.

## Fields

- Node: `id,type,x,y,params`, exactly five comma-separated fields.
  `id` is an unsigned 64-bit decimal NodeId (the graph refuses the exhausted
  maximum ID). `type` is the registry's exact type name. `x` and `y` are finite
  f32 positions. `params` lists every parameter in the kind's declared order,
  as `name=value` joined with `;`. A parameterless node ends with a comma.
  Each assignment splits at its first `=`, so Text can contain `=`.
  Nodes are written in ascending numeric ID order; IDs need not be contiguous.
- Edge: `from_id.from_port,to_id.to_port`, exactly two comma-separated fields.
  Each endpoint splits at its first `.`; the rest is the port name.
  Edges sort by numeric source ID, source port, numeric destination ID,
  destination port, using the graph's edge order.
- Export: `name,node_id.port,dir`, exactly three comma-separated fields.
  `dir` is `in` for Input or `out` for Output. Exports sort by name.
  `write` chooses `kind=subgraph` and emits `exports` exactly when the graph
  has exports; otherwise it chooses `kind=graph` and omits that section.

Values are interpreted using the kind's parameter descriptions:

- Bool: exactly `true` or `false`.
- Int: signed 64-bit decimal, using `i64::to_string()`.
- F64: finite f64, using Rust Display (`to_string()`), shortest round-trip
  decimals without an exponent. Negative zero is `-0`.
- Text: raw UTF-8, including empty text, subject to the text rule below.
- Image and Named parameters: unsupported; both reader and writer refuse them.
  Value has no Named payload, so a Named parameter cannot currently enter a
  valid Graph; the reader still explicitly refuses that declared type.

Positions use `f32::to_string()` with the same shortest round-trip rule.
No floating-point arithmetic is performed by persistence. The graph validates
finiteness before applying positions and parameter values, including floats
that parse successfully as NaN or infinity.

## Text, names and canonical form

The shared public `text_allowed` rule forbids commas, semicolons, backslashes,
any `char::is_control` character (including CR, LF, tab), and an ASCII letter
immediately followed by `:` anywhere. Thus `C:x` and `ratio:2` are forbidden;
`10:30` is allowed. Empty Text is allowed. A name (type, port, parameter,
export) must additionally be nonempty and have no `=`.
The writer checks every name it emits and every Text value.
The reader checks Text and export names directly; type, port and parameter
names are matched against the registry/graph, then checked by the writer.

After parsing all items, the reader calls `write` and compares the complete
result with the input. The first differing line is `NotCanonical`.
This single comparison refuses alternate numeric forms (`+1`, `01`, Int `-0`,
float `2.50`, `2.5e0`, `120.0`, ID `00`) and alternate item ordering.
The reader does not separately impose numeric spelling or ordering rules.
Nothing partial is returned on any error.

## Named errors and precedence

`FormatError` implements Display (variant and its fields, including line where
present) and `std::error::Error`. Lines are 1-based.

- `MissingFinalNewline`: empty input or no final LF.
- `CarriageReturn { line }`: first line containing a CR byte.
- `EmptyLine { line }`: empty line encountered in the section layout or items.
- `UnknownVersion { found }`: line 1 starts `schema=` but its suffix is not `1`.
- `BadHeader { line }`: malformed schema/kind header, or missing, extra or
  misplaced section header; a missing final header uses line count plus one.
- `BadField { line, field }`: unparseable field; field is exactly `fields`,
  `id`, `x`, `y`, `param`, `value`, `edge` or `dir`. `fields` means wrong column
  count; `param` means wrong count, name or assignment structure.
- `BadName { line, text }`: forbidden name or Text; writing uses the line
  where the item would appear.
- `Unsupported { line, param }`: Image or Named parameter.
- `NoExports`: subgraph has an exports section but no export items.
- `Graph { line, error }`: unchanged GraphError from adding a node, connecting
  an edge or exporting a port (including registry lookup and non-finite values).
- `NotCanonical { line }`: first difference from the successfully parsed
  graph's canonical writing.

Precedence is final LF, CR scan, schema, kind, then section/item lines in input
order, then NoExports, then canonical comparison. Unknown versions are refused
before reading later headers/items. A type is looked up before its parameters
are read. Parameter values parse by declared type (Bool, i64, f64 or Text);
malformed numbers are BadField, parsed non-finite numbers are Graph errors.
Graph validation also refuses duplicate IDs/exports, unknown nodes/ports,
type mismatches, occupied inputs and cycles.

## Changes from the plan draft

1. Omit the node kind column: Node has no kind field, and the type already
   identifies its family; duplicating it would allow disagreement.
2. Use shortest round-trip decimals rather than fixed precision: fixed decimal
   counts cannot preserve every f32/f64 exactly. Timeline uses this rule too.
3. `write` returns `Result<String, FormatError>`, not String: a Graph can contain
   text delimiters or Image parameters which must fail loudly, not produce an
   unreadable file.
4. Derive kind from exports: Graph has no graph/subgraph flag. Nonempty exports
   mean subgraph with an exports section; otherwise graph without one.
