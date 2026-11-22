# brush-engine

The dependency-free Rust 2021 brush-engine skeleton in forge's image group.
The crate is a member of the root workspace, not a standalone workspace. It
currently exposes only empty, documented modules; no brush algorithms or public
data types are implemented.

## Module layering

The intended responsibilities are:

- Input processing: `pressure` maps pressure response and `stabilizer` smooths
  stroke input.
- Brush processing: `dab` generates brush marks; `smudge` handles sampling and
  paint mixing.
- Output: `tiles` is the boundary for tile-oriented brush output.
- Cross-cutting support: `replay` records and replays strokes; `lag` measures
  input-to-output latency.

These are module boundaries, not implemented calls or dependency edges.
The source plan `docs/nodes/2.2.md`, section 2, is not present in this worktree;
the grouping above must be checked against it before treating it as the plan's
authoritative layering.

## Build and test

Build, check, and test **only through Broadsword**, from the forge root, using
the Broadsword executable supplied by the orchestrator:

```text
broadsword build forge --task t-20261122-042017-10
broadsword check forge --task t-20261122-042017-10
broadsword test forge --task t-20261122-042017-10
```

Do not invoke Cargo directly. Broadsword owns the build route and shared build
configuration.

## Dependency guard

`tests/structure.rs` contains `cargo_toml_has_no_dependencies`. It compares the
manifest's nonblank, non-comment lines against this skeleton's explicit
allowlist. This deliberately conservative check is not a general TOML parser:
changes to package metadata or formatting also require updating the allowlist.
It rejects added dependency lines and extra tables, including dev, build,
target-specific, and workspace dependencies.

Negative-control tests insert dependency declarations into the actual manifest
text and verify rejection without resolving or downloading any dependency.
The one-line mutation is `unexpected = "0"` beneath `[dependencies]`. It is
applied to an in-memory copy so Cargo does not attempt to resolve it; the test
also checks that the original manifest is still accepted afterward.
