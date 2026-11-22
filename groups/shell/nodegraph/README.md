# nodegraph

`nodegraph` is Forge's dependency-free, edition-2021 node graph crate. This
initial skeleton implements shared values, kind descriptions, and a kind
registry. The remaining modules and C header are documented placeholders, not
working graph, editor, evaluation, persistence, or FFI implementations.

## Layering

The module boundaries separate shared data and kind implementations from the
generic graph machinery and its consumers:

- `value` owns data-only values and their type tags.
- `kind` describes ports, parameters, evaluation, errors, and kind registration.
  It depends on shared values, not on editor or presentation code.
- `kinds` reserves the built-in implementation modules, `math` and `image`.
  Implementations belong behind the `NodeKind` interface.
- `graph`, `eval`, `format`, and `subgraph` reserve graph structure, evaluation,
  persistence, and composition. These layers should use kind descriptions, not
  dispatch on built-in kind names.
- `history`, `editor`, and `view` reserve undo/history, editing, and presentation.
  They must remain independent of concrete node kinds.
- `capi` and `include/nodegraph.h` reserve the C boundary. Every other Rust
  module declaration forbids unsafe code, including the built-in submodules.

The referenced plan (`docs/nodes/1.7.md`, section 2) is not included in this
worktree. The boundaries above describe this skeleton; exact alignment with
that section still needs verification against the plan.

## Shared interfaces

`ValueType` has built-in tags and an open `Named(String)` tag for descriptions.
`Value` contains the built-in payloads only. `ImageBuf` is data only: it does
not validate dimensions, pixel layout, or sample values. Floating-point values
retain Rust's ordinary `PartialEq` semantics (including NaN inequality); no
numeric computation or finite-value guarantee is supplied by this layer.

`NodeKind` supplies ordered port and parameter descriptions and evaluates
positional parameter/input slices. Concrete kinds are responsible for checking
their arguments and reporting `NodeError::Evaluation` on failure.
`KindRegistry` owns boxed implementations, rejects duplicate type names without
replacing the original, and reports unknown names explicitly.

## Build and test

Build, check, and test only through Broadsword, from the Forge worktree root.
Do not invoke Cargo directly.

```text
build/bin/broadsword.exe build forge --task t-20261122-042017-5
build/bin/broadsword.exe check forge --task t-20261122-042017-5
build/bin/broadsword.exe test forge --task t-20261122-042017-5
```

`tests/structure.rs` checks the dependency-free manifest, the absence of
abort-on-panic settings, and the absence of built-in kind names/namespaces in
the editor, history, and view sources. Its manifest reader deliberately accepts
only simple tables and string assignments, failing closed on unsupported
syntax rather than attempting to be a general TOML parser.

For the required mutation proof, temporarily add a dependency assignment under
`[dependencies]`, run the same Broadsword test command, and verify
`cargo_toml_has_no_dependencies` fails. Restore the manifest and rerun the
command. This mutation has not been executed in the sandbox; helper-level
regressions also exercise dependency and panic mutations without editing files.
