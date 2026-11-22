# timeline

A dependency-free timeline skeleton for the forge workspace. This brief provides
plain data and a driver interface, not evaluation or editing implementations.

## Three layers

1. **Timeline data and operations:** `time`, `track`, `curve`, `edit`, and `format`
   describe time, tracks, interpolation, editing, and persistence. Only the time
   and track data types exist in this skeleton.
2. **Evaluation and domain drivers:** `driver` defines the caller-buffer
   evaluation contract; `anim`, `audio`, and `video` reserve domain integrations.
   No driver implementation exists yet.
3. **Playback and presentation integration:** `clock`, `view`, and `capi`
   reserve playback scheduling, the timeline view, and the C interface.
   These modules are documented stubs.

Time is stored as signed integer ticks at `254_016_000_000` ticks per second.
Rates, track identifiers, clip sources, keyframe values, and optional pairs of
interpolation handles are plain data. Construction does not validate them.
Conversions, rate validation, interpolation, and playback belong to later briefs.

## Build and test

Use Broadsword only, from the forge workspace context; do not invoke Cargo
directly:

```text
broadsword build forge --task t-20261122-042017-6
broadsword check forge --task t-20261122-042017-6
broadsword test forge --task t-20261122-042017-6
```

The `structure_no_dependencies_and_no_absolute_paths` integration test reads the
manifest and every Rust, Python, C header, and Markdown source in this crate.
Its manifest checker deliberately accepts only the skeleton's single-line
manifest layout, with empty dependency tables, and fails closed on other table
forms. The source check forbids drive-letter paths and the legacy cross-repo
reference; only the structure test's own source is exempt from that text check,
so it can contain negative fixtures. A symlink manifest or symbolic links
encountered during source enumeration cause the test to fail.
