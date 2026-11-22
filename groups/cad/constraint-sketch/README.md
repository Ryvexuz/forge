# constraint-sketch

A Rust 2021, standard-library-only constraint-sketch library in forge's
`groups/cad/constraint-sketch` workspace member. It has no Cargo dependencies
and no crate-local `[workspace]` table.

## Planned layers

The plan separates the crate into three layers:

1. **Data:** `Point`, constraints, sketches, and the sketch file format.
2. **Solving and analysis:** residuals, Jacobian, solving, degrees of freedom,
   and over- and under-constrained reporting, all in pure std Rust.
3. **UI:** a future sketch UI module and example, the only layer that touches
   a window. The solver must remain testable without a UI, and other modes
   must not depend on the sketch UI.

This skeleton provides only `geom::Point` and documented module stubs for
constraints, solving, degrees-of-freedom analysis, and the file format. The
remaining data types, algorithms, file contract, and UI are future work.

## Build and test

Build, check, and test only through Broadsword, never by invoking Cargo directly.
From the Broadsword checkout, use:

```text
build/bin/broadsword.exe build forge --task t-20261122-042017-7
build/bin/broadsword.exe check forge --task t-20261122-042017-7
build/bin/broadsword.exe test forge --task t-20261122-042017-7
```

The `cargo_toml_has_no_dependencies` integration test checks that normal,
development, and build dependency tables are empty or absent. Its std-only
checker intentionally accepts only simple single-line string package fields
and the package/dependency tables used by this skeleton; unsupported syntax
fails closed rather than being treated as dependency-free.
