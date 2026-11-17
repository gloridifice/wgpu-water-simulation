# tall-cell-fluid

An independent three-dimensional incompressible Eulerian liquid library using Rust 2024. The **wgpu compute backend** is enabled by default, with an f64 CPU reference implementation also retained. It provides a complete call chain from liquid interface initialization through pressure projection and time stepping to triangle surface output. The core library does not create a window; the optional `demo` feature provides a standalone visualization program. Neither depends on the main project or reads or saves personal configuration.

It implements the tall-cell concept with horizontal quadtree partitioning, adaptive optical layers, and upper and lower pressure samples. The pressure operator uses Galerkin reduction on a background MAC grid; it is **not a term-by-term reproduction of the paper's dedicated variational discretization/virtual face structure**. This library makes no claims about the paper's performance or visual accuracy.

## Standalone visualization demo

Run from the repository root:

```powershell
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo
```

By default, a droplet falls into a basin; the other scene is a water column dam break. The demonstration domain is a cube with a side length of 2 meters and defaults to 16 cells per axis. Rendering uses the triangle water surface **actually extracted on the GPU** after each successful simulation substep, with no separate animated waveform.

| Action | Key |
| --- | --- |
| Play / pause | Space |
| Advance one CFL substep while paused | N |
| Reset the current scene | R |
| Switch droplet / dam break and reset time | 1 / 2 |
| Switch water material / triangle mesh / normals | V |
| Orbit / zoom | Left mouse drag / scroll wheel |
| Reset camera / exit | Home / Esc |

The UI uses the default theme, Dock style, and h3 headings from the crates.io `yummy_egui` 0.1 series (minimum 0.1.1), along with egui 0.34, egui_dock 0.19, and wgpu 29. It retains the MiSans font installed by the demo and does not store, read, or write theme configuration. `yummy_egui` is enabled only with the `demo` feature and does not affect the standalone workspace or core solver.

The window uses egui + egui_dock: the liquid view is in the center, and timing and control panels are on the right. Tabs and dividers can be dragged, and the layout can be restored at the top. The panels support play/pause, single stepping, reset, switching scenes and display modes, and resetting the camera. Mouse orbit and zoom work only in the liquid view. A numerical failure pauses the simulation and displays an error; press R to reset. Scene changes and pause commands are acknowledged after the current in-flight substep completes.

| Timing block | Units and scope |
| --- | --- |
| Simulation time | Wall-clock ms/substep, including CFL step size selection, CPU topology and scheduling, GPU solving, and synchronous readback waits; excludes initialization and surface generation |
| Mesh time | Wall-clock ms/surface update, including the display halo, GPU triangle/normal extraction, readback, and CPU processing; excludes uploading the Mesh to the rendering device |
| Shading time | Actual GPU timestamp, ms/rendered frame, measuring the complete scene render pass for the water surface, ground, and tank frame, including vertex processing, rasterization, and fragment shading; excludes egui, uploads, and presentation waits |

The most recently completed sample is displayed; simulation and Mesh values are retained while paused. Initially and after reset, simulation time displays “Waiting for sample”. Shading timing is read back asynchronously, without waiting for the GPU every frame for timing; “Device not supported” is displayed if the device does not support timestamps, and read failures are explicitly reported. The three blocks update at different rates and **must not be added together as the window frame time**. Hover over a name for its detailed timing scope.

```powershell
# Dam break; mesh preview. Per-axis resolution can be 8..40; increasing it increases solver wait time.
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --scene dam --view mesh --resolution 16

# Real-window smoke run for a finite number of frames.
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --frames 300

# Fixed camera; save an image and exit after advancing exactly 18 substeps. The output file must not exist.
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --steps 18 --snapshot crates/tall-cell-fluid/target/drop-step18.png

# Headless verification of motion, surface generation, CFL, and the pause/single-step/reset protocol for both scenes.
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --verify --steps 6
```

`--paused` can be used to inspect the initial state; `--snapshot` without `--steps` saves step 0. The `--steps N` mode requests one substep per frame, preventing the latest background snapshot from skipping the specified screenshot moment; this mode does not respond to play toggles from Space or the panel. See `--help` for all arguments. Screenshots include only the scene, not the egui panels or operating system title bar.

The simulation advances on a background thread while the window refreshes independently; each substep still involves readback and CPU topology assembly, so **simulation time is not guaranteed to stay synchronized with real time**. The window and simulation create separate devices, and the water surface is uploaded to the preview renderer after readback. This version is intended for observing and inspecting the complete workflow.

The water preview uses two-sided lighting, Fresnel sky reflection, and an opaque material, without transparent refraction or full volumetric scattering. The display-only implicit surface adds a layer of air samples and intersects with the physical tank domain to close the tank edges; it does not change the simulation's phi, pressure, or volume. This demo is independent of Ocean's FFT/SWE, window, and settings.

## Verification and usage

Run from the repository root:

```powershell
cargo fmt --manifest-path crates/tall-cell-fluid/Cargo.toml --check
cargo clippy --manifest-path crates/tall-cell-fluid/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml
# Actual GPU end-to-end verification: exits with a nonzero status if no adapter is available or any assertion fails.
cargo run --manifest-path crates/tall-cell-fluid/Cargo.toml --example gpu_verify
# Additional GPU numerical and boundary tests (default unit tests explicitly ignore these hardware tests).
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml --lib -- --ignored --nocapture --test-threads=1
# CPU reference verification without building wgpu.
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml --no-default-features
```

The crate uses a standalone workspace and is not part of the main program's default build; future consumers can add a local path dependency. `gpu_verify` is a headless offline example; the `fluid-demo` binary is built only when `demo` is enabled. When dependencies are cached, `--offline` can be added to the commands; no scene resources need to be downloaded.

GPU usage:

```rust,no_run
use tall_cell_fluid::{Domain, Grid};
use tall_cell_fluid::gpu::{self, GpuContext, GpuSimulation};
use tall_cell_fluid::surface::SurfaceLimits;

fn simulate_gpu() -> Result<(), String> {
    let context = pollster::block_on(GpuContext::new())?;
    let mut grid = Grid::new(Domain::new([16, 32, 16], 0.1)?);
    grid.set_liquid_sdf(|p| p[1] - 1.8);
    let mut simulation = GpuSimulation::from_grid(&context, grid, gpu::default_config())?;
    let reports = simulation.advance(0.05)?;
    let mesh = gpu::extract_surface(&context, &simulation.grid, SurfaceLimits::default())?;
    assert!(!reports.is_empty() && !mesh.indices.is_empty());
    Ok(())
}
```

CPU reference usage:

```rust
use tall_cell_fluid::{Domain, Grid, Simulation, SimulationConfig};
use tall_cell_fluid::surface::extract_surface;

fn simulate() -> Result<(), Box<dyn std::error::Error>> {
    let domain = Domain::new([16, 32, 16], 0.1)?;
    let mut grid = Grid::new(domain);
    // y is vertical, the water surface is at 1.8 m, and phi<0 denotes liquid.
    grid.set_liquid_sdf(|p| p[1] - 1.8);
    let mut simulation = Simulation::from_grid(grid, SimulationConfig::default())?;
    let reports = simulation.advance(0.05)?;
    let mesh = extract_surface(&simulation.grid)?;
    assert!(!reports.is_empty());
    assert!(!mesh.indices.is_empty());
    Ok(())
}
```

`set_liquid_sdf` can initialize arbitrary three-dimensional shapes resolvable by sampling, such as spherical droplets, liquid columns, or unions of multiple shapes. `set_solid_sdf` defines static obstacles; solid_phi<0 denotes solid. Both use meters. When initializing a closed droplet, leave a sampling margin from the computational domain boundary.

## Data and modules

| Module | Contents |
| --- | --- |
| `grid` | Cell-centered phi, solid_phi, and pressure; three separate MAC face velocity arrays; boundary, sampling, volume, and divergence diagnostics |
| `adaptivity` | Water-column velocity fitting error q, integrated evaluation I, L1 dilation, optical-layer smoothing, quadtree leaves, tall cells, pressure interpolation basis |
| `pressure` | Zero-pressure air boundary, closed-domain pressure reference, Galerkin operator, Jacobi-PCG, velocity correction, fine-grid fallback |
| `simulation` | CFL substeps, RK2 semi-Lagrangian advection, gravity, air velocity extrapolation, level-set reconstruction, and optional volume correction |
| `surface` | Consistent six-tetrahedron decomposition, shared intersection deduplication, outward normals, indexed triangles, output resource limits |
| `optics` | Reflection, Snell refraction, dielectric Fresnel, Beer–Lambert absorption, and simplified single-interface color composition |
| `gpu/transport.rs`, `gpu/shaders/transport.wgsl` | WGSL RK2 advection, velocity extrapolation, boundaries and gravity, Jacobi Eikonal distance reconstruction, parallel volume reduction and bisection correction |
| `gpu/shaders/volume.wgsl` | Independent two-stage volume reduction kernel, masking solids and invalid cells in the last workgroup |
| `gpu/adaptivity.rs`, `gpu/shaders/adaptivity.wgsl` | WGSL q/I evaluation, layer thickness seeds, L1 dilation and height smoothing; CPU topology processing after readback |
| `gpu/pressure.rs`, `gpu/shaders/pressure.wgsl` | WGSL divergence/RHS, CSR multiplication, Jacobi-PCG, dot-product reduction, true residual, pressure reconstruction and face velocity correction |
| `gpu/surface.rs`, `gpu/shaders/surface.wgsl` | WGSL Marching Tetrahedra, gradient normals, atomic output counting and capacity checks |
| `gpu/simulation` | Complete GPU substep scheduling, transactional state commit and CPU-inspectable snapshots |
| `demo` | Standalone window, egui dock panels, three-stage timing, background simulation, two initial scenes, surface/mesh/normal previews, PNG and run protocol verification |

Shaders are organized by module in `src/gpu/shaders/` and `src/demo/shaders/` and embedded at compile time via `include_str!`, with no runtime file reads.

`Domain` dimensions are cell counts, `cell_size` is in meters, and `origin` is in world coordinates. Each field is stored with x fastest, then y, then z: `x + Nx * (y + Ny * z)`. The three face arrays have dimensions `(Nx+1,Ny,Nz)`, `(Nx,Ny+1,Nz)`, and `(Nx,Ny,Nz+1)`, respectively.

The CPU reference solver uses `f64`, while the GPU uses `f32`; reading GPU results back into the f64 fields of `Grid` is only a conversion and does not increase precision. `gpu::default_config()` provides residual and divergence tolerances suitable for single precision; do not directly apply the CPU's default `1e-9` relative residual tolerance.

`SurfaceVertex` is a 24-byte `repr(C)` vertex format: `position: [f32;3]`, `normal: [f32;3]`, corresponding to two `Float32x3` vertex attributes. GPU surface output first uses a 32-byte storage format with two `vec4<f32>` values, then converts to this vertex format after readback. Uniform parameters use 16-byte groups; state, flags, CSR indices, and coefficients use packed arrays with explicit offsets, with matching Rust and WGSL alignment.

## GPU execution boundary

The GPU actually performs velocity/interface updates, evaluation functions and optical layers, pressure linear iterations and velocity correction, and surface vertex and normal generation. The CPU handles input validation, CFL decisions, quadtree and sparse connectivity processing, CSR coefficient assembly for `BᵀLB`, and result diagnostics; the CPU pressure solver is not substituted when the GPU path fails.

The first version retains synchronous readback between stages. PCG reads back convergence scalars each iteration, with dot products using segmented reduction in a single workgroup; volume correction uses two-stage parallel reduction. It is suitable for offline verification, but **has not yet been optimized for full GPU residency or real-time performance**. Pipelines are cached, but each stage still allocates temporary buffers. `GpuContext::new` returns an error if no adapter is available rather than silently skipping hardware verification.

`gpu_verify` checks CPU/GPU q/I consistency, optical layers under different topologies, closed-domain pressure and CPU velocity comparison, GPU fine-grid fallback, multistep hydrostatic equilibrium, droplets/static solids/reconstruction/volume, empty domains and CFL rejection, and surface planes/spheres/capacity overflow. Hardware unit tests additionally cover analytic advection, inclined-plane distance reconstruction, exact-zero surfaces, and closure after sphere welding.

## Each time step

1. Validate state and parameters, determine the CFL step size, save the old state, and extrapolate air velocity near the surface.
2. Backtrace to advect velocity and phi on the regular background grid, handling the closed domain and static solids.
3. Reconstruct signed distance as configured, optionally correcting the volume estimate with a uniform phi offset.
4. Apply gravity and enforce zero normal velocity at static solids and domain boundaries.
5. Build the quadtree/optical layers using the latest velocity and interface to form the adaptive pressure basis.
6. Solve the reduced pressure, map it to background cells, and correct MAC velocities.
7. Check **fine-grid divergence**; if necessary, redo full fine-grid pressure projection from the same temporary velocity.
8. Extrapolate air velocity, validate finite values, commit the new state, and return diagnostics. The surface is extracted on demand, not necessarily generated every substep.

The background grid is always regular, so horizontal grid reconstruction does not handle advection; this differs from the paper's order of first building a hierarchical grid and then advecting.

`step(dt)` rejects dt above the stability limit and does not commit a new state on failure; `advance(seconds)` automatically splits the interval into substeps. If a later substep fails, earlier successful substeps are retained, and `time` represents the time actually completed.

## Optical layers and pressure space

The local evaluation q is the sum of absolute velocity reconstruction differences at the six MAC faces, in m/s. Horizontal velocities on the two sides of a water column are each fitted linearly in height by least squares; vertical velocity is averaged. The column aggregate `I = sum(q) / Ny` still has units of m/s.

`optical_scale * I` gives the dilation distance in finest-grid cells, with the coefficient in cells/(m/s). The lower bound is `max(2*leaf_width,4)`, and the upper bound is based on `Ny/8`. For small test domains, the lower bound is capped at the domain height and the upper bound is raised to at least the lower bound, avoiding an invalid interval. The dilation distance is rounded up. Isolated airborne liquid uses a fixed dilation distance and retains a refined pressure representation.

Dilation uses Manhattan distance; height smoothing only increases fine regions. When mapping from fine cells back to coarse leaves, a conservative rule retains the entire layer if any fine cell requires it, unlike the paper's majority rule. Columns with different material boundaries or connected liquid intervals are not merged horizontally, and tall cells are not merged across air or solid.

Each fine liquid cell corresponds to one row of the pressure interpolation matrix B, with at most two nonnegative weights summing to one. Ordinary aggregate cells use one pressure degree of freedom; tall cells linearly interpolate using upper and lower degrees of freedom. The actual system solved is `Bᵀ L B p = Bᵀ b`, after which velocity is corrected using the same face operator.

A small reduced-system residual does not imply small divergence in every fine cell. `fine_fallback` is enabled by default; when `divergence_tolerance` cannot be met, the full pressure projection is repeated. `ProjectionReport` separately reports reduced and final degrees of freedom, residuals, iteration counts, fine-grid divergence, and `fallback_used`. Complex flows may fall back frequently, so a reduction in adaptive cell count cannot be taken as proof of actual speedup.

## Surface and optics

Surface extraction reads the zero isosurface of phi, independently of the pressure basis. Eight adjacent **cell centers** form a sampling cube, which is split consistently into six tetrahedra for Marching Tetrahedra; no external Marching Cubes lookup table is copied. Consistent face diagonals and shared intersection deduplication prevent seams between adjacent cubes. Normals come from the distance field gradient, with geometric normals used for degenerate cases.

The output range extends from the first to the last layer of cell centers. Liquid touching the computational domain boundary is not automatically capped; the surface is not automatically clipped by solid_phi either. When extracting a free water surface, solid occlusion should be handled by scene rendering integrated later. Boundary capping/solid clipping should be added when a closed export mesh or precise contact line is needed.

`extract_surface_with_limits` returns an error upon reaching resource limits rather than returning a truncated mesh. Surface topology changes with the fluid, and vertex indices are not guaranteed to remain stable across frames.

GPU extraction likewise reports an error on capacity overflow. It outputs a triangle soup with three independent vertices per triangle and consecutive indices; it does not perform GPU vertex welding. Atomic allocation order is not guaranteed to remain stable across calls. To inspect mesh topology, first weld by position; independent indices cannot be used directly to judge seams.

The optics module provides only testable mathematical operations. It receives caller-provided data such as reflected light, background light, normals/refractive indices, and the distance light travels through water; it does not provide scene tracing, screen-space refraction, a render pass, or full volumetric scattering. The simulation's optical layer thickness cannot be substituted for optical path length.

## Current limitations

- Both backends retain a dense background grid; synchronous readback occurs between GPU stages. The demo's simulation and Mesh timings are wall-clock times including CPU work and waits; only scene rendering uses actual GPU timestamps. The geometry preview does not yet have full water optics.
- Purely grid-based semi-Lagrangian Eulerian advection; EXNBFLIP/APIC particles, surface tension, two-way rigid body coupling, and continuous water sources are not implemented.
- Solids use static voxel no-penetration boundaries; the paper's partial face fractions and tall-cell boundaries directly embedded in solids are not implemented.
- Pressure uses a Galerkin reference scheme on the background discretization, without term-by-term reproduction of the paper's minimal-face/virtual-face operator and ICCG preconditioner.
- Semi-Lagrangian advection has numerical dissipation. Optional volume correction preserves only the **global** volume estimated by a smooth Heaviside function, shifting by at most one grid spacing; it does not guarantee per-cell mass conservation or exact geometric volume, and reports always return the actual result.
- Liquid films and droplets thinner than the background sampling spacing may be lost. The finest background data is always allocated, so fewer pressure unknowns do not imply reduced memory usage.

## Sources

- Narita & Kanai, *Beyond Horizontal Refinement: Extending Quadtree Tall Cells with Adaptive Optical Layers*, Computer Graphics Forum 45(7), 2026. DOI: https://doi.org/10.1111/cgf.70629
- Narita & Kanai, *Adaptive Optical Layers: Efficient Tall Cell Grids for Liquid Simulation*, Computer Graphics Forum 45(2), 2026. DOI: https://doi.org/10.1111/cgf.70357

The code is an independent implementation; it does not copy the paper's source code or third-party surface lookup tables, and the repository does not contain the paper PDFs.
