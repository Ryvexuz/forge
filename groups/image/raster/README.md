# raster

`raster` is the Rust 2021, standard-library-only raster library in forge's
`groups/image/raster/` workspace member. This first brief provides the module
skeleton, not pixel operations. Its dependency table is empty; it has no private
workspace and no dependency on another group or repository.

## Planned layers

1. **Data:** `tile` will hold a premultiplied `f32` RGBA `Tile` (256 x 256)
   and sparse `TileStore`. `layer` will hold `Layer` (opacity, blend mode,
   optional mask, offset) and `Document` (canvas size and layers). No window
   is involved.
2. **Maths:** `blend` will provide the twelve separable blend modes, `mask`
   mask and selection values, and `layer` pure document compositing.
3. **Edits:** `paint` will stamp strokes and report dirty tiles; `adjust`
   will provide pure per-tile brightness/contrast, curves, and hue/saturation
   operations. Modes will edit the shell-supplied document through its API,
   rather than own separate pixel buffers or run private compositor steps.
4. **Glue:** `gpu_compose` and `graph_bridge` reserve the integration points
   for the GPU and graph nodes (R9 and R10). They contain no integration code
   or imports now. Those briefs must satisfy the cross-node architecture
   decision before adding integrations; no GPU or graph implementation is copied
   here.

The module files are documentation-only stubs. `lib.rs` declares them, and the
structural tests use only `std`. No spilling tiles to disk is implemented.

## Build and test

Build and test **only through Broadsword**, from its controlling checkout:

```text
build/bin/broadsword.exe build forge --task t-20261122-042017-9
build/bin/broadsword.exe check forge --task t-20261122-042017-9
build/bin/broadsword.exe test forge --task t-20261122-042017-9
```

Do not invoke Cargo directly. The workspace root owns workspace membership.

`cargo_toml_has_no_dependencies` checks the manifest using a deliberately small,
fail-closed syntax: `[package]` with single-line string metadata and an
empty `[dependencies]`. Comments and blank lines are accepted; other tables,
duplicate keys/tables, and unsupported syntax fail instead of being ignored.
`src_has_no_path_use_outside_the_crate` recursively reads every file under `src`
and checks `use` roots against `std`, `core`, `alloc`, `crate`, `self`, `super`,
and an initially empty external allow-list. It handles grouped/multiline imports
and skips comments and literals.

The checker regression tests exercise rejected dependency declarations and
external imports without changing files. For the separate live mutation proof,
the orchestrator must temporarily add a dependency entry under `[dependencies]`,
run the Broadsword test command above and observe
`cargo_toml_has_no_dependencies` fail, then restore the empty table and rerun.
Dependency resolution may reject a live mutation before tests start; that is not
the required test-failure evidence and must not be reported as such.
