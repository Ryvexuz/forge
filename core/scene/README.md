# scene

`scene` is Forge's edition-2021, dependency-free scene crate. This skeleton
declares documented modules only; it does not yet implement scene operations or
file readers and writers.

The planned responsibilities are grouped into three layers:

1. **Foundations:** `math`, `transform`, and `hierarchy` will hold the mathematical
   primitives, transforms, and parent/child relationships.
2. **Scene composition:** `instance`, `layer`, and `scene` will hold instances,
   layers, and the scene container.
3. **File formats:** `format`, `json`, `gltf`, `gltf_write`, and `usd` will hold
   format interfaces and scene interchange readers and writers.

The future Vec3/Mat4 provenance belongs in `src/math.rs`; the future instance
update order belongs in `src/instance.rs`. `docs/FORMAT.md` is only a placeholder.

## Build and test

Build, test, and check only through Broadsword, from the Broadsword command
environment; do not invoke Cargo directly:

```text
broadsword build forge --task t-20261122-042017-4
broadsword test forge --task t-20261122-042017-4
broadsword check forge --task t-20261122-042017-4
```

The structure tests check the skeleton manifest's empty dependency tables and
scan every Rust source file under `src/` for imports rooted outside `std`,
`core`, `alloc`, `crate`, `self`, or `super`. Their regression cases exercise
dependency insertion and disallowed imports without adding actual dependencies.
