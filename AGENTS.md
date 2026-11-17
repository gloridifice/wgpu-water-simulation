# Project collaboration guidelines

This file applies to the entire repository. Communicate with the user in Chinese; keep project UI, documentation, and comments in English. Treat the current code and the user's latest requirements as authoritative; do not describe proposed ideas as implemented features.

## Project and entry points

- The project uses Rust 2024 edition.
- The root package's `cargo run` starts GPU Ocean by default; the CPU 2D PIC terminal program and 1D shallow-water mode are separate, in workspace member `crates/tui-2d-prototype/`.
- The default `ocean` feature provides a wgpu 3D ocean: offshore FFT, local nearshore 2D SWE, foam, beach and water optics, and an egui_dock parameter panel.
- When changing GPU Ocean, preserve the independent CPU package and do not introduce GPU dependencies into it; keep `crates/tall-cell-fluid/` an independent workspace.
- Usage guides live in `doco/guides/`, indexed by `README.md`; architecture and precise contracts live in `doco/architecture.md` and `doco/specs/`. Update relevant documentation when user-visible behavior, parameters, or startup changes.

## Code locations

| Location | Responsibility |
| --- | --- |
| `crates/tui-2d-prototype/src/{main.rs,pic/,tui/}` | CPU PIC entry, solver, and terminal interaction |
| `crates/tui-2d-prototype/src/{solver.rs,shallow_tui/}` | 1D shallow-water solver and terminal interaction |
| `src/bin/ocean.rs` | GPU Ocean entry and CLI arguments |
| `crates/common/src/` | Shared window loop, GPU, egui/viewport, Dock layout, configuration persistence, and camera math; must not depend on simulation business logic |
| `src/ocean/app/`, `camera.rs` | Ocean frame scheduling, snapshots, business input, and compatible camera configuration/GPU layout |
| `src/ocean/gpu/` | Common device compatibility entry and Ocean timing metadata adapter |
| `src/ocean/parameters/` | Parameter definitions, defaults, and validation, independent of window or GPU resources |
| `src/ocean/simulation/spectral/` | Shared ocean/procedural-water spectra, GPU FFT, periodic sampling, and associated shaders |
| `src/ocean/simulation/offshore/` | Offshore foam, ocean coupling verification, and legacy FFT compatibility exports |
| `src/ocean/simulation/nearshore/` | SWE, FFT input, nearshore foam, state readback, and verification |
| `src/ocean/simulation/spray/` | Nearshore spray sources, particles, and associated shaders |
| `src/ocean/scene/` | Model geometry, terrain heightfields, and colliders |
| `src/ocean/render/` | Water surface, seabed, models, sky optics, textures, and associated shaders; `water/` provides shared ocean/procedural-water optical composition |
| `src/ocean/ui/` | Dock, business panels, color editing, application fonts, and layout persistence |
| crates.io `yummy_egui` | Shared egui controls, parameter/selection lists, and default theme; this repository does not retain library source or theme configuration |
| `src/ocean/settings/`, `ui/panels/configuration.rs` | Session, named configuration library, atomic saves, and configuration management UI |
| `src/ocean/verify.rs`, domain `verification/` or `verification.rs` | Real GPU verification entry and domain behavior checks |
| `src/ocean/labs/foam_weight_lab/` | Independent foam weight experiment |

## Common commands and validation

Run from the repository root:

```powershell
# Start GPU Ocean; omit scene presets to restore personal settings.
cargo run
# Optimized build for performance observation.
cargo run --release

# Start the original CPU program.
cargo run --release -p tui-2d-prototype

# Formatting, static checks, and unit tests including Ocean modules.
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --features ocean --lib
# CPU package tests; --workspace also covers the root and CPU packages.
cargo test -p tui-2d-prototype

# Build and verify the real GPU path.
cargo build --release --features ocean --bin ocean
target\release\ocean.exe --verify

# Finite-frame UI smoke run without overwriting personal settings.
target\release\ocean.exe --beach --no-settings --frames 600
```

- Choose validation appropriate to the change. Documentation-only changes need no build; layout-only changes need not repeat full physics verification.
- `cargo test --features ocean --bin ocean` is not a substitute for library tests; use `--lib` for Ocean module tests.
- Rust compilation does not validate WGSL. Run relevant GPU verification after changing shaders, bindings, or buffer layouts; state explicitly if unavailable.
- For CPU package or workspace boundary changes, validate with `cargo test -p tui-2d-prototype` and `cargo clippy -p tui-2d-prototype --all-targets -- -D warnings`; start with `-p tui-2d-prototype`. Both directory and Cargo package are named `tui-2d-prototype`. Root `--no-default-features` no longer selects the CPU program.
- Fix camera, time, random seed, spectrum, and solver order when comparing visuals. `--snapshot` renders the scene and cannot verify parameter-panel layout.
- Do not add tests that merely restate reversible small UI changes. Numerical boundaries, configuration compatibility, mass transport, and similar behavior need meaningful verification.

## Simulation and GPU conventions

- Put WGSL files in the owning module's `shaders/` directory and embed them at compile time using source-relative `include_str!` / `include_bytes!`; cross-module reuse must reference the owning module, not duplicate files.
- Use responsibility-based paths inside Ocean; legacy re-exports in `ocean/mod.rs` and `gpu/mod.rs` are only for existing library callers, not internal dependencies.
- Preserve the offshore FFT / nearshore SWE division; current coupling is FFT → SWE. Do not inadvertently reintroduce an independent nearshore wave generator.
- FFT input includes water level and matching momentum; check both when adjusting filtering, gain, or initialization, avoiding double application of gain.
- Keep first- and second-order SWE switching. Solver changes must check still-water balance, wet/dry boundaries, stable time steps, and nonnegative depth.
- Waterborne foam density, foam residue on sand, beach wetness history, and offshore FFT foam are distinct states; do not mix their thresholds, colors, or decay parameters.
- Nearshore waterborne foam includes generation, transport, convergence, and decay. Density is not display opacity; do not clamp simulation density directly to 0–1 for display convenience.
- Conservative transport already expresses convergence and dilution; check for double counting before adding compression enhancement.
- Preserve the density debug view so density and shading can be verified independently. Advance pore textures and wavelet improvements only when explicitly requested, without expanding scope.
- Rust uniform/storage structures and WGSL definitions, alignment, bindings, copy sizes, and substep offsets must agree. Check all relevant shaders when adding fields.
- SWE GPU timing must use real GPU timestamps and support devices without them; never label CPU submission time as GPU time or block on the GPU every frame for timing.
- State parameter units. Distinguish millimeters from meters, simulation from wall-clock time, and dimensionless thresholds from rates of change. Explain a formula's name and meaning before using undefined symbols.

## Parameter panel conventions

- Use egui_dock with six fixed left navigation workspaces: Scene, Shallow Water, Offshore, Animated SDF Ripples, Shading, and Configurations, each with an independent Dock layout. Default parameters are on the left and Render View on the right; the top toolbar remains visible. Switching changes only UI, not simulation, drafts, or camera.
- Each layout retains one non-closable Render View; parameter panels can be closed and added across categories. Layout sidecars are independent of sessions and named configurations and respect read-only/no-save mode; see `doco/specs/workspace-ui.md`.
- Use crates.io `yummy_egui` 0.1 series (minimum 0.1.2) with egui 0.34, egui_dock 0.19, and wgpu 29 for shared controls and themes; Ocean business panels depend directly on the library, without copying controls. Do not store, load, or save theme configuration in this repository. Use `Theme::default()` and its `dock_style()` for Dock; the application installs MiSans and theme application must not override fonts.
- Use one shared two-column grid per logical parameter group so rows align; do not create an independent grid for each row.
- Put names on the left and controls on the right using shared layout helpers. After column spacing, the name column takes 42% and controls 58%.
- Allow long names to wrap; controls must not expand column widths and misalign group boundaries.
- Use `yummy_egui::h3` for group headings and ordinary emphasized text (default 14 pt, high contrast, not bold); higher levels may use h1 (20 pt, bold) and h2 (16 pt, bold). Headings already have vertical margin; do not duplicate it. Keep semantic colors for errors and warnings.
- Put parameter explanations in name hover tooltips, not persistent explanatory small text. Keep necessary action buttons, current status, and errors.
- Tooltips explain purpose, units, range, and whether changes apply live or reset the simulation.
- When only adjusting panels, do not also change simulation, shading, or parameter defaults.

## Configuration and parameter integration

- Check the entire chain for new parameters: defaults, allowed range, finite-value validation, serialization compatibility, panel, CPU → GPU transfer, WGSL usage, and any required reset or rebuild.
- Give new fields defaults so old configurations load; do not discard an entire configuration merely because a new field is absent.
- Distinguish manual working-state saves, named snapshot overwrites, and automatic Dock layout saves. Only toolbar Save / Ctrl+S or Save Configuration in the configuration panel writes the session; editing, loading, focus loss, and exit do not. Failures require a manual retry. Saving simulation parameters alone does not save panel layout.
- The current parameter save entry is `settings/store.rs`; `default_path()` determines the default path. Do not hardcode developer-machine absolute paths.
- Manage named configurations through the configuration library. Preserve default snapshot, save-as, and explicit overwrite semantics; toolbar Save / Ctrl+S records working state and library without accidentally overwriting a named snapshot with current parameters. Confirmed creation, deletion, and rename still require manual saving to disk.
- For inconsistent restore after saving, check read path, save errors, CLI overrides, and `--no-settings`. In particular, `--beach` currently applies a scene preset and overrides part of saved state.
- Prefer `--no-settings` or dedicated test files for automated verification and previews; do not overwrite personal configuration under investigation.

## Workspace and delivery

- Inspect worktree status and relevant code before modifying. Preserve existing uncommitted user changes; do not revert, overwrite, or mix in unrelated refactoring.
- Do not commit or push without request; when asked to commit, inspect the diff and commit only the agreed scope.
- On Windows, a running program may lock its executable. Do not terminate the user's running program to build; use a different build artifact or explain the obstruction.
- Prefer finite-frame previews; if cleanup is necessary, only operate on processes explicitly started for this task.
- Do not version build outputs, screenshots, temporary configurations, or debug logs unless explicitly requested.
- Check licenses and origins when referencing external projects, retaining attribution as needed; copied assets must use repository-local paths, not personal download directories.
- Summarize changes, validation, and unverified limitations upon completion. Do not claim code inspection, compilation, or screenshot inference as actual runtime confirmation.

<!-- DOCO:START -->
<!-- doco:entry template=v2 -->
## Doco

For current project documentation and explicitly tracked changes, use the
`doco` skill. Read `.agents/skills/doco/SKILL.md` before creating, executing, completing,
or archiving a change. Perform only the requested phase.
A doco change package is optional for implementation and is not an
implementation-history mechanism: archiving retains only proposal.md. Routine
behavior fixes and implementation-detail changes may proceed without creating
a doco change unless the user explicitly requests tracking. Use Git, pull
requests, or release notes for implementation history. Regardless of tracking,
update current architecture and specs when their documented facts or contracts
change.
Use the doco library nearest to the working directory, or select one with
`doco --root <library-root> ...`. Child libraries keep their own documents and
changes but reuse this installed skill; do not generate child skill or Agent
entry files. Resolve the skill path above from this entry's directory and
`doco/` paths from the selected library. Parent documents describe shared
constraints; child documents describe local facts without automatic overrides.
`doco list` includes all project libraries, current first; write commands and
change IDs remain local to the selected library.
Start from its `doco/architecture.md` and relevant current specs and decisions.
When implementing a selected tracked change, use its proposal and any present
design and task files. Treat completed changes, archived changes, and
`doco/tmp/` as non-current material; consult them only when explicitly needed.
<!-- DOCO:END -->
