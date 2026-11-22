# brush-engine

The dependency-free Rust 2021 brush-engine skeleton in forge's image group.
The crate is a member of the root workspace, not a standalone workspace. It
currently exposes only empty, documented modules; no brush algorithms or public
data types are implemented.

## Module layering

Per `docs/nodes/2.2.md`, section 2 ("Fit with the product intent"), the brush
has three layers:

1. Data: `Dab`, `Stroke`, `BrushSettings` and the file formats; portable and
   std only.
2. Maths: pure functions from pen samples to dabs and from dabs to pixels, with
   no window and no clock.
3. I/O: the event-log reader and the tile file reader.

Layers 1 and 2 are testable with no window, so a mode can call them without
Win32. Modes receive samples from the shell; the brush never opens a window
itself. The planned modules are `dab`, `pressure`, `stabilizer`, `smudge`,
`replay`, `tiles` and `lag`; the plan does not assign each module to a layer,
and this skeleton does not either.

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
