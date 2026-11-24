# Scene file, schema 1

The first line is exactly `schema=1`. `CURRENT_SCHEMA` is 1 and the named
`MIGRATIONS` list is empty:

| From | To | Step |
| --- | --- | --- |

Older canonical decimal versions are accepted only if every intervening step
exists. Steps convert full file text; the result is parsed as current schema.
All other header values, including `01`, `0`, `2`, and `1` followed by CR,
are unknown schemas.

Files use LF, never raw CR, and every line including the last ends in LF.
There are no blank lines or trailing spaces. Rows appear in kind order:
all nodes, all transforms, all layers, then all instances.
Nodes and transforms appear in contiguous arena-index order starting at zero.
Layers retain list order and instances retain insertion order.

## Fields

Comma fields are numbered from zero:

| Kind (field 0) | Field 1 | Field 2 | Field 3 | Field 4 |
| --- | --- | --- | --- | --- |
| `node` | node arena index | parent index or `-` | escaped name | absent |
| `xform` | node index | translation x y z | quaternion x y z w | scale x y z |
| `layer` | layer id | escaped name | visible: `0` or `1` | absent |
| `inst` | layer id | node index | angle in radians | speed per frame |

Ids are canonical decimal u32: no sign, whitespace, or leading zero except
`0` itself. Parent indices may refer forward. Node names are held separately
in `SceneFile.node_names`, parallel to the arena.

Each float is exactly eight lowercase hexadecimal digits encoding its IEEE
f32 bits (`00000000` through `ffffffff`, subject to finiteness). Vector
components have exactly one separating ASCII space, with no leading or
trailing space. No decimal conversion is performed: `-0.0` is `80000000`
and retains its sign. Infinities and every NaN payload are refused.
Rotations must be accepted by `Quat::new` and its normalized stored components
must have exactly the input bits; otherwise the rotation is refused.

The instance matrix is not stored. Instances are constructed through
`Instances::push`, which rebuilds it using `scene::instance::matrix_from_angle`.
Meshes and node mesh links are not stored; the writer refuses them rather than
silently dropping them. Cycles and duplicate layer ids are not refused.
Cycle detection belongs to hierarchy world evaluation.

## Names

Names may be empty. All other characters are raw except:

| Character | Encoding |
| --- | --- |
| `%` | `%25` |
| `,` | `%2C` |
| LF | `%0A` |
| CR | `%0D` |
| Each ASCII space in the trailing run | `%20` |

Escapes are case-sensitive. `%20` is accepted only when all remaining bytes
in the name field are also `%20`. Interior spaces remain raw.
Raw trailing spaces, raw CR, incomplete escapes and all other escapes are errors.
The entire trailing space run must be escaped: `a %20` is not canonical either.

## Validation and errors

The writer checks name count, scene meshes, then the first node mesh link.
It renders and calls the reader, the sole validator for all remaining rules.
The reader checks header presence, schema, final LF, then rows in line order.
Each row checks kind, count, fields left to right, then ordering.
After parsing rows, references are checked in line order (instance node before
layer), then missing transforms. Error lines are one-based; fields are zero-based.

| `FormatError` | Condition |
| --- | --- |
| `MissingHeader` | Empty input or first line does not start `schema=` |
| `UnknownSchema { found }` | Header suffix is not a supported canonical schema |
| `MissingFinalNewline` | Nonempty supported-schema input lacks final LF |
| `UnknownKind { line }` | Unknown kind, including an empty row |
| `FieldCount { line, expected, found }` | Wrong comma-field count |
| `BadNumber { line, field }` | Noncanonical id, float, vector or visibility |
| `NonFinite { line, field }` | Infinity or NaN float bits |
| `NotUnitQuat { line }` | Quaternion refused or normalized to different bits |
| `BadEscape { line }` | Invalid name encoding |
| `UnknownNode { line, id }` | Parent, transform or instance refers beyond arena |
| `UnknownLayer { line, id }` | Instance layer has no matching layer row |
| `OutOfOrder { line }` | Kind goes backward or node/transform index skips position |
| `MissingTransform { node }` | First node without a transform row |
| `NameCount { nodes, names }` | Writer names and arena lengths differ |
| `MeshNotStored { node }` | Writer has scene meshes (`None`) or node mesh link (`Some(index)`) |

## Hand-written v1 golden example

```text
schema=1
node,0,-,root
node,1,0,Walls%2C exterior
node,2,0,50%25 done%0Aline two
xform,0,00000000 00000000 00000000,00000000 00000000 00000000 3f800000,3f800000 3f800000 3f800000
xform,1,3f800000 80000000 c0000000,3f800000 00000000 00000000 00000000,3f800000 3f800000 3f800000
xform,2,00000000 40400000 00000000,80000000 00000000 00000000 3f800000,40000000 40000000 40000000
layer,7,Props,1
layer,3,Hidden%2C for now,0
inst,7,1,3f000000,3dcccccd
inst,3,2,80000000,bf800000
```
