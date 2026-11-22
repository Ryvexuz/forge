# mesh-kernel

A dependency-free Rust 2021 mesh-processing kernel in forge. M1 provides the
module skeleton and deterministic indexed-triangle fixtures, not production
mesh operations. The generator's small `Mesh` type is fixture data, not the
future half-edge representation.

## Layers

The skeleton groups the planned work into three layers:

1. **Representation and foundations:** `vec3`, `halfedge`, and `attr` hold
   geometry, connectivity, and attributes; `gen` supplies repeatable fixtures.
2. **Geometry algorithms:** `subdiv`, `remesh`, `predicates`, `broadphase`,
   `intersect`, `split`, `classify`, and `boolean`.
3. **Editing integration:** `modifier` and `undo` compose operations and track
   reversible edits.

Apart from the generator and crate-root module declarations, the source modules
are documentation-only stubs. The referenced node plan (`docs/nodes/2.3.md`,
section 2) is not present in this worktree; the grouping above describes this
skeleton and still needs comparison with that source.

## Build and test

Build, check, and test **only through Broadsword**, from its managed environment:

- `broadsword build forge --task t-20261122-042017-3`
- `broadsword check forge --task t-20261122-042017-3`
- `broadsword test forge --task t-20261122-042017-3`

Do not invoke Cargo directly. No third-party, cross-group, or cross-repository
dependencies are used, and this crate has no nested workspace.

## Deterministic fixtures

All generator functions return `Result<Mesh, GenerateError>`. Counts and
allocation sizes are checked; no configurable resource cap is imposed here.
An unrepresentable count, failed reservation, or unrepresentable geometric
refinement returns a named error rather than truncating the mesh.

- `closed_box_with_triangles(n)` starts with an outward-oriented cube and
  replaces one triangle by its three centroid triangles per refinement.
  This preserves shared boundary edges and adds exactly two faces, so every
  even count starting at twelve is supported subject to representability and
  available memory. Odd counts and smaller counts are errors. A growing
  triangle-index cursor avoids repeatedly refining just one child.
- `jittered_closed_mesh(seed)` uses SplitMix64 to choose an even face count in
  the inclusive range 12 to 2,000. It perturbs the eight cube corners by less
  than or equal to 1/16 along each axis before centroid refinement. Subdivision
  therefore stays on the initial triangles rather than perturbing fine faces
  independently. All seeds, including zero, are valid.
- `icosphere(level)` subdivides a unit icosahedron, sharing normalized edge
  midpoints between adjacent faces. It produces `20 * 4^level` triangles;
  level 6 produces 81,920.

These routines take integers only. Their internal coordinates are bounded:
cube coordinates are in `[-1, 1]`, jittered coordinates in `[-17/16, 17/16]`,
and sphere coordinates in `[-1, 1]`. Normalization and newly computed
coordinates are checked before insertion. There is no clock or OS randomness;
edge ordering and output order are deterministic.

The structure test's manifest guard deliberately accepts only the small,
single-line TOML subset needed by this skeleton: package assignments, empty
dependency tables, and empty target containers. Unknown tables, multiline
strings, escaped table keys, and duplicate tables fail closed. Its negative
fixtures exercise dependency mutations without modifying the on-disk manifest.
The requested on-disk mutation demonstration must additionally be performed
through Broadsword: add a dependency entry, observe
`cargo_toml_has_no_dependencies` fail, restore the manifest, and rerun.
