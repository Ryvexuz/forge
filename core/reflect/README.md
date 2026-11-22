# reflect

Standard-library-only reflection crate skeleton for the forge workspace.
The public modules are documentation-only stubs; no reflection behavior or
runtime limit enforcement is implemented yet.

## Build and test

Build and test only through Broadsword, never by invoking Cargo directly.
For this task, the orchestrator runs:

- `build/bin/broadsword.exe check forge --task t-20261122-042017-2`
- `build/bin/broadsword.exe test forge --task t-20261122-042017-2`

The crate produces both an `rlib` and a `cdylib`. The cdylib lands in
Broadsword's configured shared Cargo target directory, under the selected
profile: `<shared-target>/<profile>/reflect.dll` on Windows,
`<shared-target>/<profile>/libreflect.so` on Linux, or
`<shared-target>/<profile>/libreflect.dylib` on macOS. An explicit target
triple adds a `<target-triple>/` directory before the profile. The shared
directory is controlled by Broadsword, not by this crate; this checkout
does not contain its configuration.

## Limits configuration

`reflect.toml` holds the `[limits]` table as required keys, with no default
in code: `max_path_length`, `max_text_file_bytes`, `max_list_length` and
`max_nesting_depth` (the four limits named in the node 1.6 plan). The stub
modules do not read this file yet; the `Limits` type arrives in a later brief.

## Skeleton test

`lib_exports_all_modules` imports every public module and checks the source
declarations and documentation-only module files at runtime. It requires no
artifacts from later implementation tasks.
