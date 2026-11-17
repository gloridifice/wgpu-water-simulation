<!-- doco:lifecycle v=1 created-at=2026-10-28T10:33:00Z completed-at=2026-10-29T06:25:42Z archived-at=2026-10-29T06:25:51Z -->
# Reorganize GPU Ocean Modules by Responsibility

## Purpose

The baseline ocean directory contained 44 Rust files at one level, mixing device wrappers, FFT, nearshore solving, rendering, UI and verification. `gpu.rs` defined Gpu, Ocean and Renderer together; configuration referenced the window's Camera, and sky calculations contained GUI charts. Organize directories by responsibility according to the approved audit recommendations to make the code easier to navigate and maintain without changing functionality.

## Scope and acceptance

- Organize ocean into app, gpu, parameters, simulation, scene, render, ui, settings and labs; separate the shared camera. Retain the five existing src/bin entry points and do not change the workspace or features.
- Separate device, FFT and Renderer; extract camera, snapshots, parameter definitions, UI panels and nearshore verification. WGSL follows its actual owner, with cross-module references to the single original.
- Preserve original public module paths through compatibility re-exports; do not add a trait/mock framework or widen internal field visibility for the migration.
- Keep type names, serialized fields, defaults, validation ranges, CLI precedence, workspace identifiers, save paths and behavior unchanged; sessions, named libraries and layouts remain independent.
- Keep GPU uniform/storage ABI, shader bytes and concatenation order, physical algorithms, call order, resource ownership, reset conditions and timing scopes unchanged. Retain separate window and snapshot scheduling rather than unifying them here.
- Acceptance: formatting, Clippy, all-target compilation and existing library tests pass; real GPU --verify passes; repeatable fixed-input render snapshots match pixel for pixel and the window layout smoke check passes. Spray transparency retains the original unsorted parallel drawing; compare repeated runs of the same version and production GPU behavior rather than adding sorting to force bitwise determinism. The CPU package remains independently testable without GPU dependencies.
- Update current architecture, path documentation and relevant spray specification. After verification, commit only this change and push the existing branch.

Non-goals: solver or material optimization, new parameters, configuration migration, UI layout redesign, splitting crates, cleaning up other active changes or deleting existing tests.

## Result

Implemented, verified and pushed to origin/master in commit 85de9dc. Rust files at the ocean directory's top level fell from 44 to 3; old public paths remain compatible. Existing library, tool and CPU tests and real GPU verification passed; 21 WGSL files were byte-identical, and five repeatable scenarios and spray-source debug snapshots matched the baseline. Particle transparency retains the very small nondeterminism present in repeated runs of the same version; the algorithm was not changed for screenshots. The default window layout was inspected, but not every tab interaction was manually verified.
