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

# USD subset (.usda)

## What import accepts

`import_usda` accepts exactly `#usda 1.0` followed by LF and a body, or that header alone without LF (`src/usd.rs:405–410`). This is not full USD.
The body contains `def Xform "name" { ... }` and `def Mesh "name" { ... }`,
including nested prims and multiple roots. Names match `[A-Za-z_][A-Za-z0-9_]*`
and must be unique among siblings, including roots (`src/usd.rs:424–471`).

The four supported ops in `OPS` (`src/usd.rs:13–18`) are:
| Op | Attribute type | Value |
| --- | --- | --- |
| `xformOp:translate` | `float3` or `double3` | `(x, y, z)` |
| `xformOp:rotateXYZ` | `float3` or `double3` | `(x, y, z)` in degrees |
| `xformOp:orient` | `quatf` | `(w, x, y, z)` |
| `xformOp:scale` | `float3` or `double3` | `(x, y, z)` |

Ops are not `uniform`. `uniform token[] xformOpOrder` is a list of quoted
op names. Every authored op must occur exactly once in it, with no missing
or unknown ops; order is translate, rotation, scale, omitting absent ops.
`rotateXYZ` and `orient` cannot coexist. Attributes cannot repeat.
No ops and an absent or empty order are valid. Missing transform components retain `Transform::default()`. Orient is normalized by `Quat::new`;
zero orient is invalid (`src/usd.rs:249–303`, `src/usd.rs:323–363`).

Only Mesh accepts `point3f[] points` (triples), `int[] faceVertexCounts`, and `int[] faceVertexIndices`; all three are required and are not `uniform`.
Each count must be 3, index count must equal three times face count, and
each index must address a point. Empty arrays are valid
(`src/usd.rs:261–267`, `src/usd.rs:365–392`).

Numbers have optional `-`, digits, optional decimal point followed by digits,
and optional `e` or `E` with optional sign and digits. No leading `+`, `.5`,
`1.`, NaN or infinity literals are accepted. Both `double3` and `float3`
parse directly to finite `f32`; overflow is refused. Integer arrays require
digit-only tokens fitting `u32` (leading zeros are accepted).
Only space, tab and LF are skipped between tokens; CR is not whitespace. Tuples require commas; lists accept commas including a trailing comma.
No final body LF is required (`src/usd.rs:86–242`).

Comments after the header are unsupported (`#` is refused). Layers and
composition are unsupported: `over`, `class`, top-level metadata parentheses,
prim metadata parentheses, other prim kinds, unknown attributes, other op
names and wrong attribute types are refused (`src/usd.rs:107`,
`src/usd.rs:249–280`, `src/usd.rs:323–358`, `src/usd.rs:424–455`).
At most 128 prims may be nested, counting a root as one: `usd::MAX_DEPTH`
aliases `json::MAX_DEPTH` (`src/usd.rs:11–12`, `src/json.rs:14`);
the guard is at `src/usd.rs:428–429`. Width does not consume this limit.

## How prims become nodes

Each opening `def` appends a node in pre-order file position. Its parent is
the enclosing prim's arena index, or `None` at a root. Each Mesh opening
also appends a mesh, assigning sequential mesh indices in Mesh pre-order;
Xform has no mesh. Prim names are validated but not stored
(`src/usd.rs:438–473`). Layers and instances remain empty.
`rotate_xyz_degrees` computes half-angle sine/cosine in `f64`, multiplies
`qZ * qY * qX` (X first, then Y, then Z), casts components to `f32`, and
calls `Quat::new` (`src/usd.rs:499–524`). World matrices are validated before
return (`src/usd.rs:490–494`).

## What export writes

`export_usda` validates before writing (`src/usd_write.rs:55–129`). The header is `#usda 1.0` with LF. Each root or child block has a preceding
empty line. Names are `n<i>` using arena index; nodes with a mesh use
`def Mesh`, otherwise `def Xform`. Indentation is four spaces per level;
opening and closing braces are on separate lines (`src/usd_write.rs:131–143`,
`src/usd_write.rs:189–194`).

Every node always writes `float3 xformOp:translate`, `quatf xformOp:orient`
(w first), `float3 xformOp:scale`, then
`uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]`.
Mesh attributes follow in order: `int[] faceVertexCounts` (one 3 per triangle),
`int[] faceVertexIndices`, `point3f[] points`. Lists use brackets and tuples
parentheses, with `, ` separators and no trailing comma. Floats use shortest
round-trip `Display` text. Lines end in LF without trailing spaces
(`src/usd_write.rs:147–213`). An empty scene writes only the header and LF.

## Refusals

The first error is returned. Import errors carry one-based source lines where
applicable; comparison returns the first named difference using float bit
equality (`src/usd.rs:527–580`). Code references below identify return sites
in the source documented here.

| Error | Raised by | When | Code |
| --- | --- | --- | --- |
| `Header` | `import_usda` | Header is not the exact accepted prefix or header-only text. | `src/usd.rs:409` |
| `Syntax` | `import_usda` | Bad token, number syntax, delimiter, tuple arity, incomplete prim or unexpected input. | `src/usd.rs:183` |
| `Unsupported` | `import_usda` | Comments, composition, metadata, unknown prims/attributes/types/ops, mesh attributes on Xform, non-triangle counts, both rotation ops or unsupported op order. | `src/usd.rs:107` |
| `Invalid` | `import_usda` | Bad or duplicate sibling names; duplicate attributes/order entries; missing or unlisted ops; zero orient; missing mesh attributes; bad integers, index length/range or mesh-index overflow. | `src/usd.rs:279` |
| `NonFinite` | `import_usda` | Parsed float is non-finite, or rotateXYZ conversion fails. | `src/usd.rs:201` |
| `TooDeep` | `import_usda` | A new prim exceeds `MAX_DEPTH`. | `src/usd.rs:429` |
| `Hierarchy` | `import_usda` | World-matrix evaluation fails, including non-finite composed transforms. | `src/usd.rs:493` |
| `Mismatch` | `compare_scenes` | Node count, parents, mesh links, transform bits, meshes, layers or instance data differ. | `src/usd.rs:538` |
| `Layers` | `export_usda` | Scene layers are nonempty. | `src/usd_write.rs:57` |
| `Instances` | `export_usda` | Instance node list is nonempty. | `src/usd_write.rs:60` |
| `Hierarchy` | `export_usda` | World evaluation fails: cycle, bad parent or invalid transform. | `src/usd_write.rs:65` |
| `NodeOrder` | `export_usda` | Pre-order traversal does not visit nodes at their arena positions. | `src/usd_write.rs:83` |
| `MeshOrder` | `export_usda` | A mesh index is not the number of preceding mesh nodes; shared or out-of-order meshes cannot round-trip. | `src/usd_write.rs:94` |
| `MeshCount` | `export_usda` | Mesh-node count differs from mesh count, including unused or missing meshes. | `src/usd_write.rs:100` |
| `MeshInvalid` | `export_usda` | Index count is not divisible by 3, an index is out of range, or a position is non-finite. | `src/usd_write.rs:114` |
| `RotationNotFixed` | `export_usda` | `Quat::new` refuses the rotation or changes component bits. About 1.4% of unit `f32` quaternions are not fixed points, as the module doc states (`src/usd_write.rs:13–14`); exporting them would change a value. | `src/usd_write.rs:122` |
| `TooDeep` | `export_usda` | Node depth exceeds the shared import nesting limit (root depth is zero). | `src/usd_write.rs:127` |

## Canonical example

This is the export of the `hierarchy` USD fixture, pinned by `usd_export_canonical_text` in `tests/usd_round_trip.rs`.

```text
#usda 1.0

def Xform "n0"
{
    float3 xformOp:translate = (1, 2, 3)
    quatf xformOp:orient = (1, 0, 0, 0)
    float3 xformOp:scale = (1, 1, 1)
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]

    def Xform "n1"
    {
        float3 xformOp:translate = (0.5, 0, -0.25)
        quatf xformOp:orient = (0.70710677, 0, 0, 0.70710677)
        float3 xformOp:scale = (1, 1, 1)
        uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]

        def Mesh "n2"
        {
            float3 xformOp:translate = (0, 0, 0)
            quatf xformOp:orient = (1, 0, 0, 0)
            float3 xformOp:scale = (2, 2, 2)
            uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
            int[] faceVertexCounts = [3]
            int[] faceVertexIndices = [0, 1, 2]
            point3f[] points = [(0, 0, 0), (1, 0, 0), (0, 1, 0)]
        }
    }

    def Xform "n3"
    {
        float3 xformOp:translate = (0, 0, 0)
        quatf xformOp:orient = (1, 0, 0, 0)
        float3 xformOp:scale = (1, 1, 1)
        uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
    }
}

def Xform "n4"
{
    float3 xformOp:translate = (0, 0, 0)
    quatf xformOp:orient = (1, 0, 0, 0)
    float3 xformOp:scale = (1, 1, 1)
    uniform token[] xformOpOrder = ["xformOp:translate", "xformOp:orient", "xformOp:scale"]
}
```
