> The default `cargo run` starts the 3D GPU Ocean; use `cargo run --release` for performance comparisons. See [wgpu FFT Ocean](ocean.md). This guide describes the independent CPU package `crates/tui-2d-prototype`, started with `cargo run -p tui-2d-prototype` without GPU dependencies.

The independent 3D incompressible fluid library lives in [`crates/tall-cell-fluid`](../../crates/tall-cell-fluid/README.md). It includes a wgpu compute backend, CPU reference implementation, adaptive optical layers, tall-cell pressure space, full time integration and surface generation. Its independent `fluid-demo` visualization (with the `demo` feature, egui controls and simulation / mesh / shading timing panels) and headless `gpu_verify` use its own manifest and are not integrated with Ocean.

# Two-Dimensional Side-View CPU PIC Water Simulator

A Rust free-surface liquid simulator. Particles move horizontally and vertically, allowing the surface to lean forward, overturn, fall and splash. The terminal shows actual particle positions rather than compressing them into a single height curve.

The default scene contains a shallow slope on the right, a reciprocating paddle on the left, and a local clockwise momentum impulse to make breaking waves easy to observe. The solver is pure PIC; FLIP and APIC are not yet included.

## Running and Observing Breaking Waves

```powershell
# Run from the repository root.
cargo run --release -p tui-2d-prototype
```

Windows Terminal at 120×40 or larger is recommended; the minimum is 70×22. The default grid is 112×64 with 0.1 m cells and 6736 water particles. Default playback speed is 0.25× to make the initial approximately 0.2–0.4 seconds of overturning easier to see; the exact shape varies slightly with physical timestep and resolution.

```powershell
# Start paused and press N to inspect individual physics steps.
cargo run --release -p tui-2d-prototype -- --paused

# Start from still water; press B to create a breaking wave and W to control the paddle.
cargo run --release -p tui-2d-prototype -- --no-pulse --no-waves

# Lower grid resolution; changing the grid also changes the physical tank size.
cargo run --release -p tui-2d-prototype -- --nx 80 --ny 48

# Run without a terminal and save particle trajectories at each output time; the target must not exist.
cargo run --release -p tui-2d-prototype -- --headless --time 2 --frames 40 --csv pic-frames.csv

# Use the original one-dimensional shallow-water simulator and mouse disturbances.
cargo run --release -p tui-2d-prototype -- --shallow --paused
cargo run --release -p tui-2d-prototype -- --shallow --headless --quiet

cargo run -p tui-2d-prototype -- --help
```

An interactive terminal starts the 2D TUI by default; redirected input/output or `--headless` selects batch mode. `--time / --frames / --csv / --quiet / --animate` also select batch mode. The original `--cells 41` still selects one-dimensional shallow-water mode.

## Controls

| Input | Effect |
| --- | --- |
| Space | Pause / resume |
| N | Pause and advance one CFL physics step |
| R | Reset to the initial scene and pause, retaining playback speed, paddle switch and grid display |
| W | Toggle the continuously reciprocating paddle; when disabled it stops at its current position |
| B | Apply another local clockwise momentum impulse to overturn the surface |
| Left mouse button | Apply a local upward and rightward velocity impulse to nearby particles |
| + / - | Double / halve playback speed, from 0.0625× to 4× |
| G | Show / hide MAC grid |
| Left / Right | Select previous / next particle |
| Home / End / C | Select first / last / middle-numbered particle |
| Q / Esc / Ctrl+C | Quit and restore terminal and mouse |

Both mouse coordinates map to a 2D position. Clicking air with no nearby water particles does not add water; clicks on borders, the status bar or axis labels are ignored. Impulses can be applied while paused and then inspected step by step. Light brown denotes solid slope, orange the paddle, cyan water particles, white faster particles, and yellow the selected particle. White is not a separate foam model.

The view initially focuses near still-water level, omitting some empty air above. Its range expands when particles approach the top; axis labels and mouse mapping use the same physical range. Reset restores the default view.

The continuous paddle primarily produces a rightward surge. B and the initial breaking-wave impulse are controlled external momentum inputs: they add a clockwise velocity field near the slope entrance, after which pressure, gravity and particle motion produce overturning. They do not directly bend the surface, prescribe particle trajectories or draw a breaking-wave shape in the renderer. This demo does not claim that the paddle alone can reliably generate detailed coastal breaking waves.

## Core Code

```text
crates/tui-2d-prototype/src/
  main.rs         Argument parsing and TUI / batch selection
  lib.rs          Exports pic, solver and one-dimensional solver types
  pic/
    mod.rs        Parameters, particles, stepping, wave generation, mouse impulses, diagnostics
    grid.rs       MAC staggered grid, air/water/solid, slope and boundaries
    transfer.rs   Particle→grid, extrapolation, grid→particle, RK2 motion and collisions
    pressure.rs   Sparse pressure equation, Jacobi-preconditioned conjugate gradients, velocity projection
  tui/            2D particle view and interaction; reuses pause/step/speed/mouse lifecycle
  pic_batch.rs    2D ASCII and particle CSV output
  solver.rs       Original independent 1D shallow-water core
  shallow_tui/    Original 1D water surface and velocity chart
  batch.rs        Original 1D batch output
```

Algorithm modules depend only on the Rust standard library and do not read the keyboard, clock or terminal. Ratatui and Crossterm are used only by the application UI. Library example:

```rust
use tui_2d_prototype::pic::{Config, Simulation};

fn run() -> Result<(), String> {
    let mut water = Simulation::new(Config::default())?;
    water.wave_pulse();
    water.advance_to(0.3)?;
    for particle in water.particles() {
        println!("x={} y={} u={} v={}", particle.x, particle.y, particle.u, particle.v);
    }
    Ok(())
}
```

A standalone non-interactive example is also available:

```powershell
cargo run --release -p tui-2d-prototype --example pic_probe -- 2 frames.csv pulse
```

## What a PIC Step Does

Particles store `(x, y, u, v)`. Unlike the original `(h, hu)`, multiple water boundaries can occur at the same x, allowing air gaps beneath a wave crest.

The MAC grid stores horizontal velocity u on vertical faces, vertical velocity v on horizontal faces, and pressure p and cell type at cell centers. A complete physics step:

1. Update the paddle position and mark solid, water-particle and air cells.
2. Average particle velocities onto corresponding u/v faces using bilinear weights.
3. Apply gravity to vertical-face velocities near water and set wall-normal velocity.
4. Solve pressure and correct velocity so net outflow from water cells approaches zero.
5. Extrapolate valid velocities three layers outside the free surface for interpolation near boundaries.
6. Interpolate grid velocity directly to particles: this is the current pure PIC update.
7. Move particles with midpoint RK2 and handle tank, stair-step slope and paddle collisions.

The key pressure projection relationships are:

```text
u_new = u_star - dt * grad(p)        density = 1
div(u_new) ≈ 0
```

After discretization, each water cell connects only to four neighbors. `pressure.rs` applies the matrix directly to vectors rather than creating a dense matrix and solves with Jacobi-preconditioned conjugate gradients. Air pressure is zero; solid-face normal velocity is prescribed by the boundary. The default limit is 100 iterations. The status bar shows RMS divergence before and after solving and actual iterations; reaching the limit does not imply exact convergence.

CFL adaptively selects dt based on particle velocity, maximum paddle velocity and gravity scale; each step is also limited to 1/90 second. The TUI uses a time accumulator and advances only when a complete physics step has accumulated, preventing faster refresh from increasing PIC transfers and numerical dissipation. Each UI loop performs at most eight physics steps, so actual playback can be slower than the target under load.

## Diagnostics and Limitations

`particles` should remain constant: mouse and wave generation change motion only, without creating or deleting particles. Each particle represents a fixed area; `volume()` returns their sum. This conserved quantity does not mean zero reconstructed free-surface volume error: finite grids and particle sampling still cause geometric volume errors.

`overhangs` counts grid columns with water below, at least two air cells in the middle, and water above. It helps identify overturning or splashing but is not a breaking-wave quality score and does not distinguish detached droplets from connected sheets.

This version simulates side-view 2D water, with zero-pressure air of negligible inertia, gravity and free-slip solid walls. The slope is approximated by stairs. There is no air entrainment dynamics, surface tension, 3D foam or turbulence model. PIC velocity averaging dissipates rotation and detail, so breaking waves flatten quickly after falling; this motivates a future FLIP upgrade.

The future FLIP integration point is `transfer.rs::grid_to_particles()`: save grid velocities before pressure/external-force updates, interpolate the grid velocity change and add it to old particle velocities. This path is not enabled today.

## Verification

```powershell
cargo fmt --all --check
cargo test --locked -p tui-2d-prototype
cargo clippy --locked -p tui-2d-prototype --all-targets -- -D warnings
```

Checks include constant-velocity PIC transfer, pressure projection reducing divergence, still water, CFL, particle retention and solid boundaries, actual overturning indicators, local impulses, pause/step/reset, identical stepping at different refresh rates, mouse position mapping and terminal sizes. Existing one-dimensional mass conservation, symmetry, stability and mouse disturbance tests continue to run.

A step-by-step explanation of the original one-dimensional algorithm is in [One-dimensional shallow water](shallow.md). For algorithm background, see [Matthias Müller's PIC/FLIP notes](https://matthias-research.github.io/pages/tenMinutePhysics/18-flip.pdf) and [Bridson's fluid simulation course](https://www.cs.ubc.ca/~rbridson/fluidsimulation/).
