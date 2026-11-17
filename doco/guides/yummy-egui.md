# yummy_egui Integration and Default Theme

Common controls use the crates.io [`yummy_egui`](https://crates.io/crates/yummy_egui) 0.1 series, minimum version 0.1.2. This repository no longer maintains component library source, theme TOML, or external theme loading/saving entry points.

## Dependencies and Boundaries

- `yummy_egui` 0.1.2 uses egui 0.34.3 and optional egui_dock 0.19.1, and requires Rust 1.92 or newer.
- Hosts use egui/egui_extras/egui-winit/egui-wgpu 0.34.3, egui_dock 0.19.1, and wgpu 29 to avoid mixing UI or GPU types from different versions.
- The root package enables `yummy_egui` only with the `ocean` feature and enables the library's `dock` feature. The independent `crates/tall-cell-fluid` workspace enables this UI dependency only under `demo`.
- The CPU package `tui-2d-prototype` does not depend on egui, yummy_egui, or GPU; the root workspace contains the root package, `common` library, and CPU package.
- Configuration selection, load-failure rollback, simulation parameter saving, and Dock layout belong to the host, not the component library.

## Default Theme

Ocean, slider-lab, foam-weight-lab, and the tall-cell-fluid demo apply the library's default theme directly:

```rust
let theme = yummy_egui::Theme::default();
theme.apply(&context);
// The host installs fonts; theme.apply does not replace them.
let dock_style = theme.dock_style().clone();
// Pass .style(dock_style.clone()) when creating DockArea.
```

Theme assets ship with the crates.io dependency; no files need to be read from this repository or the working directory. The library theme is not copied, and theme configuration is not saved. Ocean still installs MiSans through `src/ocean/ui/fonts.rs`; the fluid demo installs the same font independently.

Not saving the theme does not affect simulation sessions, named configurations, or the `.workspaces.json` layout sidecar. Dock margin debugging and slider-lab appearance adjustments remain in memory and are discarded on exit, never written back to the library default theme.

## Controls and Documentation

Business panels call `yummy_egui::ParameterSlider`, `ParameterList`, `SelectionList`, `PanelButton`, `group`, `parameter`, `choices`, and h1/h2/h3 directly. Logical parameter groups share a two-column grid with 42%/58% widths and name hover tooltips; selection lists return interaction responses only, with business state committed by the caller.

Ocean uses `ParameterSlider` for floating-point parameters, retaining valid ranges and precise entry on the right. Wide-range nonnegative parameters can use logarithmic scales; signed coordinates and angles use linear scales. Newly replaced inputs constrain values only while editing, without rewriting loaded values merely for display or changing defaults, explicit apply, reset, or manual-save semantics. The Ocean toolbar time multiplier slider has a limited width to avoid occupying the entire toolbar.

Ocean's left navigation uses `IconSegmentedControl::vertical()` with five SVG icons in a continuous segmented bar; the host saves workspace selection and supplies hover text, without changing simulation or Dock layout semantics.

See [docs.rs](https://docs.rs/yummy_egui) and the [upstream repository](https://github.com/gloridifice/yummy-egui) for the library API and theme implementation; this project does not maintain a second API manual.

## Verification

```powershell
cargo test --features ocean --lib
cargo clippy --features ocean --all-targets -- -D warnings
cargo test -p tui-2d-prototype
cargo check --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --all-targets
```

The upstream library maintains component tests; the host retains configuration business logic, Dock layout, and camera interaction tests. After GPU backend upgrades, also build and run `ocean --verify` and inspect the real UI with a limited-frame window; an Ocean snapshot without UI cannot substitute for this.

If an existing Windows lockfile resolves `gpu-allocator`'s broad dependency to an old `windows` version while wgpu 29 uses `windows` 0.62, Direct3D12 types may mismatch. Update the old version in the affected workspace, for example `cargo update -p windows@0.58.0 --precise 0.62.2`; do not modify third-party source in the Cargo cache or disable the graphics backend as a workaround.

The MIT license and theme attribution for `yummy_egui` are maintained by the upstream package; they do not change the licenses of this project or other bundled assets.
