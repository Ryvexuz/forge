# studio

The workload workspace from Broadsword's ADR 0004 (roadmap repo map), accepted 2026-11-22.

- `core/` : the shared core crates (math, data structures, job system, serialization, command/undo, plugin API, reflection, scene and data model, mesh kernel, numerics).
- `groups/<mode>/` : one folder of crates per mode (model, render, engine/physics, cad, ...). A group depends on core and never on another group.

crash-sim and render move in later (ADR 0004 F6), with their history kept. Until then no brief adds a path dependency between repos.
Build and test only through Broadsword: `broadsword build|test|check studio`.
