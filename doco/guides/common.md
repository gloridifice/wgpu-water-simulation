# Common Desktop Framework

## Ownership

`crates/common/` is a workspace library shared by GPU desktop programs. It does not depend on `water-simulation` or own FFT, SWE, foam, or scene parameters. The root package uses it under the `ocean` feature; the CPU package does not depend on it, and `tall-cell-fluid` remains a separate workspace.

- `app::run` accepts window attributes, an optional presentation frame limit, and an initialization closure; applications implement `WindowScene`. The application controls input consumption and compute/draw order. The common loop does not impose a simulation cadence or substitute wall time for actual simulation time. Optional `poll_commands` runs during event-loop waits, even when the window cannot present.
- `app::Desktop` creates the window Surface and GPU; `app::acquire` handles timeouts, occlusion, loss, and invalidation. Zero-size windows do not draw or count toward the presentation limit.
- `gpu::Gpu` provides a device context for windowed and offscreen use; explicit blocking readback is reserved for verification and offscreen export. `gpu::timer` asynchronously samples real timestamps with at most one request in flight and returns None when unsupported. Applications attach their own phase, order, or simulation-step metadata.
- `ui::Egui` manages platform input state, the default theme, and GPU drawing; the host installs fonts and image loaders. `ui::Viewport` manages a resizable native egui texture and color view, not camera input.
- `ui::layout::{Layout,Workspace}` is parameterized by the application's `PanelId` and retains temporarily hidden panels separately from explicitly closed ones. Applications provide panels, availability conditions, default layouts, and migrations; new programs need not copy Ocean's category registry.
- `settings::SettingsStore<T>` uses the application's serde type and `Settings::validate`. It saves working state and named libraries and retains a read-only default snapshot. Edits are persisted only on explicit save. `load_config` is strictly read-only and does not replace corrupt files.
- `settings::sidecar::Sidecar<T>` is independent of session saving and handles layout read limits, throttling, and failure reporting. Corrupt files can only be replaced after explicit repair. Applications create it only in writable mode.
- `camera::Orbit` computes only the 3D target, eye, projection, and pan. Character motion, camera collisions, and wave display switches belong to the application.
- The optional `mcp` feature provides the `control` MCP stdio adapter and bounded command queue. Applications own their scenes, validate fields, implement actions, and determine execution order. Ocean's `--headless` and `--no-ui` scene window use this adapter; the original Dock desktop window does not. See [Ocean headless debugging](ocean.md#headless-mcp-debugging).

## Ocean Compatibility

The original `water_simulation::ocean` public entry points remain available. The GPU device is re-exported, and timing and camera have Ocean adapters. Ocean JSON top-level fields, default paths, named configurations, manual-save/automatic-layout boundaries, and v1/v2/v3 layout compatibility retain their semantics. Ocean simulation, rendering WGSL, parameter defaults, FFT→SWE order, and offscreen warmup are unchanged. Independent lab windows still have their own application adapters; not all windows are claimed to share the common loop.

## Verification

Run from the repository root:

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --no-default-features
cargo build --release --bin ocean
.\target\release\ocean.exe --verify
.\target\release\ocean.exe --beach --no-settings --frames 600
```

These checks were run during framework extraction: 68 Ocean library tests and 33 CPU package tests passed, and full GPU verification and a 600-frame window run succeeded. Screenshot inspection showed the beach, water, five-category navigation, Dock, and persistent toolbar. Offscreen before/after images with fixed personal configuration, camera, and one simulated second differed at only 411 pixels by at most one color level (1/255); they were not byte-identical.

Logs and images reside in Git-ignored `target/migration-check/` and are not source assets. Verification used read-only configuration or `--no-settings` and did not write personal sessions. Screenshots establish only the display from that viewpoint, not Surface failure handling on other platforms or all real Dock interactions.
