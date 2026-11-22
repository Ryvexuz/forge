# Sketch text format v1

This format is the contract with other crates. The first line is `schema=1`.
A new field means `schema=2` plus a migration; a reader never guesses.
Unknown numeric versions are rejected, not interpreted as v1.

Every line ends in LF (`\n`), including the last. Fields have exactly one
ASCII space between them. Blank lines, comments, tabs as separators, CR,
and extra fields are not allowed. Indices and counts are unsigned decimal
integers without a sign. Real values are read with Rust `str::parse::<f64>`;
non-finite values (including NaN, inf and overflow to infinity) are rejected.
Finite scientific notation is accepted on input.

## Layout and fields

* `schema=1`: schema version.
* `points N`: number of point rows.
* `p i target_x target_y start_x start_y`: zero-based sequential point index,
  known solution coordinates, then perturbed starting coordinates. Exactly N
  rows occur, ordered from index 0 through N-1.
* `constraints M`: number of following constraint rows, in vector order.
* `fix point x|y value`: point index, coordinate axis, pinned coordinate value.
* `distance a b distance`: two point indices, target Euclidean distance.
* `angle a b c d radians`: four point indices, signed angle from b-a to d-c
  in radians.
* `parallel a b c d`: four point indices defining directions b-a and d-c.
* `tangent start contact center`: three point indices, line start, contact
  point, and circle center.
* `coincident a b`: two point indices that should coincide.

All constraint indices must be less than N. Parsing validates syntax, indices
and finiteness, not geometric feasibility or the domain required by residual
evaluation (for example, nondegenerate directions).

The writer uses `format!("{:.12}", v)` for every real value: exactly 12 decimal
places, never scientific notation. Canonical 12-decimal values with |v| below
1e3 round-trip byte for byte. Arbitrary higher-precision input is rounded by
writing, so it is not promised byte identity. Indices and counts are written
as plain unsigned integers. The writer expects a well-formed Sketch: equally
long targets/start vectors, finite values and valid indices. It does not
validate or solve sketches; malformed in-memory sketches can yield text that
the parser rejects.

## Named errors

Line numbers are 1-based. Missing rows use the expected line, possibly one
past EOF. Extra rows use the first unexpected line.

| Error | Trigger |
| --- | --- |
| `Empty` | Empty input or an empty first line. |
| `MissingHeader` | First line is not `schema=` followed by unsigned digits. |
| `UnknownVersion(u64)` | Parsed version is not 1. |
| `BadSection { line }` | Missing or malformed points/constraints header, including an invalid or overflowing count. |
| `WrongFieldCount { line }` | Known row kind has the wrong number of space-separated fields. |
| `BadNumber { line }` | Invalid/overflowing integer, invalid real, invalid fix axis, CR anywhere on the line, overflowing version, or missing final LF. |
| `NonFinite { line }` | A real parses as NaN or infinity. |
| `UnknownKind { line }` | Row kind is not recognized (including a blank row where a row is expected). |
| `BadIndex { line }` | Constraint index is outside the point vector or point row index is out of sequence. |
| `CountMismatch { line }` | Too few/many rows, including constraints header encountered before all points. |

The fixture is literal source data, not writer-generated output. Its first
three fix rows pin the frame. Signed angle rows specify the turn sides;
tangent rows constrain contact points p5 and p7 with circle centers p6 and p8.
