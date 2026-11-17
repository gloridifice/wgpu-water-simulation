# One-Dimensional CPU Shallow-Water Simulator

> This guide describes the retained one-dimensional version. `cargo run` at the root starts GPU Ocean by default; the CPU program is independent in `crates/tui-2d-prototype`. Select one-dimensional mode with `cargo run -p tui-2d-prototype -- --shallow`, without GPU dependencies. Code paths below are relative to that crate.

An interactive one-dimensional CPU shallow-water simulator. A central water-level bump separates into left- and right-traveling waves that reflect from walls at either end. The core algorithm uses only the Rust standard library; the interface uses Ratatui and Crossterm.

## Running

In PowerShell:

```powershell
# Run from the repository root.
cargo test -p tui-2d-prototype
cargo run -p tui-2d-prototype -- --shallow
```

In an interactive terminal, the TUI starts by default and simulates continuously at 1× speed; press Q to quit. Windows Terminal is recommended, at least 70 columns × 22 rows; curves are clearer at 120 × 36. Redirected input or output automatically selects batch mode; `--headless` also selects it explicitly.

```powershell
# Start paused and inspect individual CFL timesteps with N.
cargo run -p tui-2d-prototype -- --shallow --paused --cells 41

# Start the interactive interface with an optimized build.
cargo run --release -p tui-2d-prototype -- --shallow

# Print final statistics only; CSV saves all cells initially and at each output time.
cargo run -p tui-2d-prototype -- --shallow --headless --quiet --csv snapshots.csv

# Longer run / more output frames / adjusted CFL.
cargo run -p tui-2d-prototype -- --shallow --headless --time 12 --frames 24 --cfl 0.4
cargo run -p tui-2d-prototype -- --help
```

`--time / --frames / --csv / --quiet / --animate` also select batch mode for compatibility with earlier commands. These batch options cannot be combined with `--tui / --paused`. `--cells` and `--cfl` apply to both modes.

## Controls

| Key | Action |
| --- | --- |
| Space | Pause / resume |
| Left mouse button | Click inside the water surface plot to add a local wave packet at the corresponding tank position |
| N | Pause and advance exactly one CFL timestep |
| R | Rebuild the initial Gaussian bump, reset time and pause |
| + / - | Double / halve playback speed, from 0.125× to 8×; equals also increases speed |
| G | Switch between 41 / 201 cells, reinitialize and pause |
| Left / Right | Select previous / next cell |
| Home / End / C | Select first / last / central cell |
| Q / Esc / Ctrl+C | Quit and restore the terminal |

The cyan curve is current water depth, the dark blue curve is the initial surface, and the gray line is still-water level. A yellow point marks the selected cell; the status bar shows its `h / hu / u`. The green curve below is velocity: positive is rightward and negative leftward.

The surface vertical axis never shrinks and expands when a crest or disturbance exceeds its range; reset restores the initial range. Velocity uses an automatic vertical axis, so inspect both its ticks and `max|u|` to understand flow. Every cell participates in the solver; window resizing changes only the display. Relative mass error uses the current initial state as reference; reset or grid switching establishes a new reference mass.

The mouse x coordinate selects the disturbance position in the tank; y only determines whether the click is inside the water plot. Clicks on borders, axes, the velocity plot or other panels are ignored; dragging does not continuously add disturbances. The yellow marker moves to the clicked cell, and `ripples` shows the disturbance count since reset. A disturbance appears immediately when paused without resuming; press N to step. During playback it continues propagating. On exit mouse capture is disabled, restoring terminal mouse behavior.

A click creates a local bump approximately 0.08 times the still-water depth, lowering the sides slightly without adding total water, directly injecting momentum or resetting time. `Simulation::add_disturbance()` in the independent solver constructs a wave packet from the difference of narrow and wide Gaussians and normalizes their discrete sums on the actual grid, conserving mass even near walls. If repeated clicks make water shallow, the entire disturbance amplitude is reduced to retain at least 75% of the original depth.

Playback speed determines simulated time advanced per wall-clock second; each physics step remains CFL-limited. Each interactive loop uses at most 0.1 seconds of wall-clock increment to avoid blocking input while catching up after suspension. Under heavy load actual playback is slower than the selected speed. The UI targets about 30 frames per second; single-step mode advances exactly one solver timestep per action.

## Module Structure

```text
src/
  lib.rs          Library entry; exports pic, solver and one-dimensional solver types
  solver.rs       Cell / Flux / Config / Simulation, Rusanov, CFL, double buffering, boundaries
  main.rs         Argument parsing and TUI / batch selection
  batch.rs        Original ASCII snapshots, animation and CSV output
  shallow_tui/
    mod.rs        One-dimensional terminal lifecycle, keys and refresh loop
    app.rs        Pause, step, speed, reset and grid selection
    view.rs       Curves, diagnostics, key hints and window layout
```

The dependency direction in one-dimensional mode is `shallow_tui / batch → solver`. `solver.rs` does not read keyboard, clock or terminal and contains no Ratatui/Crossterm types; it can be read independently or moved to another Rust project.

```rust
use tui_2d_prototype::solver::{Config, Simulation};

let mut simulation = Simulation::new(Config::default())?;
simulation.advance_to(1.0)?;
let center_depth = simulation.cells()[100].h;
```

Here `?` is used in a caller returning `Result`. The TUI drives the solver through the same public interface; plotting does not change solver state. See the [Ratatui chart example](https://ratatui.rs/examples/widgets/chart/) for UI implementation background.

## Batch Output

By default, batch mode simulates 6 seconds and outputs the initial state and 12 subsequent equally spaced snapshots. The CSV target must not already exist, to avoid overwriting results. Each row contains `t,cell,x,h,hu,u`; `x` is the cell center and `u=hu/h`. Display frames are not simulation steps: the solver automatically takes many CFL substeps between outputs and shortens the last to align with the output time. Batch animation delays each frame by 50 ms, affecting playback only.

In ASCII, `*` marks the water surface and `.` the initial still-water level. The vertical axis is fixed and exaggerates surface variation; at most 81 columns are shown, while all 201 cells still participate in the calculation. Each frame includes depth range, maximum velocity, timestep and relative mass error.

Defaults are in `Config::default()` in `src/solver.rs`:

| Parameter | Default |
| --- | --- |
| Number of cells N | 201 (41 or any other count of at least 3 is allowed) |
| Tank length L | 20 m |
| Still-water depth h₀ | 1 m |
| Central bump height | 0.2 m |
| Gaussian bump standard deviation | 0.8 m |
| Initial velocity u | 0 |
| Gravity g | 9.81 m/s² |
| CFL | 0.45, allowed range (0, 0.5] |

The bottom is fixed at zero, so depth `h` is also surface height. Initialization samples a Gaussian at cell centers to approximate cell-averaged depth; it is not an exact cell integral. All calculations use `f64`.

## Understanding an Update Through the Code

Start with `src/solver.rs`, then see how `src/shallow_tui/app.rs` calls the solver, and finally `tests/physics.rs`. `src/shallow_tui/view.rs` handles display; the solver does not depend on the terminal.

### 1. What a Cell Stores

```rust
pub struct Cell {
    pub h: f64,
    pub hu: f64,
}
```

`h` is average depth in a fixed cell; `hu` is depth times velocity, not velocity itself. Compute `hu / h` when velocity is needed. Water may flow out of the cell, but the cell stays in place.

Treating `Q=(h, hu)` as two numbers updated together, the shallow-water equations are:

```text
∂Q/∂t + ∂F(Q)/∂x = 0
F(Q) = (hu, (hu)²/h + g h²/2)
```

`Cell::physical_flux()` implements this `F`. Mass flux is `hu`; momentum flux includes momentum carried by flow and hydrostatic pressure. Even at zero initial velocity, different depths have different pressure fluxes and therefore generate motion. See the [Clawpack shallow-water textbook](https://www.clawpack.org/riemann_book/html/Shallow_water.html) for the equations and wave speeds.

### 2. How Adjacent Cells Exchange Water and Momentum

At an interface, `left` and `right` are the states on either side. They generally differ; `rusanov_flux()` supplies the common interface flux:

```rust
let alpha = left.wave_speed(gravity).max(right.wave_speed(gravity));
// Mass component; the momentum component has exactly the same structure.
mass: 0.5 * (fl.mass + fr.mass) - 0.5 * alpha * (right.h - left.h)
```

This corresponds to:

```text
F_hat = (F_L + F_R)/2 - alpha * (Q_R - Q_L)/2
alpha = max(|u_L| + sqrt(g h_L), |u_R| + sqrt(g h_R))
```

The first term averages the physical fluxes; the second adds numerical dissipation according to the state difference. When both sides agree, the difference is zero. For `h_L=1.2, h_R=1.0, hu_L=hu_R=0`, `alpha≈3.431 m/s`, and interface mass flux is approximately `+0.343 m²/s`, directed from higher water on the left to lower water on the right.

Why does this correction suppress oscillation? Temporarily treat `alpha` at neighboring interfaces as the same constant and substitute the dissipative term into the update to obtain an additional:

```text
alpha * dt/(2 dx) * (Q[i+1] - 2 Q[i] + Q[i-1])
```

The parenthesized term is a discrete second difference that smooths sharp changes between cells. At the spatial discretization level it approximates artificial diffusion with coefficient `alpha*dx/2`. This is not physical viscosity: the numerical method smooths the solution, more noticeably on coarse grids. It is a local Lax–Friedrichs / Rusanov approximate flux, not an exact Riemann solution. See also [MFEM's RusanovFlux documentation](https://docs.mfem.org/4.10/classmfem_1_1RusanovFlux.html).

### 3. Updating the Whole Tank

`Simulation::step()` performs these operations:

1. Compute the CFL timestep from the current state.
2. Compute N+1 interface fluxes, including both walls.
3. Each cell reads its left and right interface fluxes and writes to `next`.
4. Check the new state, then swap `current` and `next`.

The core lines:

```rust
h: q.h - dt_over_dx * (right.mass - left.mass),
hu: q.hu - dt_over_dx * (right.momentum - left.momentum),
```

mean:

```text
new state = old state - dt/dx * (outflow on right - inflow on left)
```

All interface fluxes are positive rightward; negative means leftward transport. An internal interface's flux is outflow for the left cell and inflow for the right, so the contributions cancel in the total. Mass conservation follows from sharing the same interface flux, without an extra total-water correction.

`std::mem::swap(&mut self.current, &mut self.next)` swaps ownership of the two `Vec`s without copying all data. This also ensures the whole update uses old states, preventing a cell on the right from reading the newly written state of its neighbor.

### 4. Walls and Timesteps

Ghost states `(h, -hu)` are used outside both walls. The left wall flux takes `(ghost, first)`; the right takes `(last, ghost)`. Equal depths and opposite momenta yield exactly zero wall mass flux, while momentum flux remains, representing the wall's effect on water. This produces reflection. See [Clawpack's reflecting boundaries](https://www.clawpack.org/bc.html).

Wave speeds relative to the grid are `u ± sqrt(g*h)`, so each step uses:

```text
dt_cfl = CFL * dx / max_i(|u_i| + sqrt(g*h_i))
dt = min(dt_cfl, time remaining until next output)
```

Higher water or faster flow requires a smaller timestep. The default initial step is approximately 0.013 seconds. Small-amplitude waves on the initial 1 m depth travel at about 3.13 m/s and take roughly 3 seconds from center to wall, so the default 6-second demo shows reflection.

## Verification Coverage

```powershell
cargo fmt --all --check
cargo test -p tui-2d-prototype
cargo clippy -p tui-2d-prototype --all-targets -- -D warnings
```

Physics tests cover equal-state flux, hand-calculated depth-jump flux, zero wall mass flux, still water, long-term mass conservation on 41/201 cells, mirrored depth and antisymmetric momentum, dynamic CFL limits, small-amplitude wave speed and reversed propagation after reflection. A steeper fully wet bump is also run for 30 seconds at CFL=0.5 to check finiteness, positive depth, boundedness and energy, along with invalid input, time alignment and command-line checks.

TUI tests cover pause, stepping, reset, grid switching, cell selection boundaries, playback speed, key and mouse connections to the solver, and drawing at 120×36, 80×24, 70×22 and undersized windows. Mouse hit tests compare actual rendered axis positions and check that only left-button presses inside the water plot add disturbances. Disturbance tests additionally check mass conservation at the center and wall, subsequent propagation, positive depth after repeated disturbances and invalid inputs. `tui-test` can further check keys, mouse, screen and normal exit in a real PTY.

Conservation checks use `sum(h_i)*dx`, water volume per unit width; with density fixed at 1 this is also mass. Floating-point rounding error is allowed. Reflecting walls exchange momentum, so total momentum is not generally required to be conserved; for symmetric initial conditions it should remain near zero. Rusanov has numerical dissipation, so energy is not required to be exactly conserved either.

This version uses first-order spatial discretization and explicit Euler time integration; it is suited to line-by-line study and basic behavioral verification. Tests cover only the stated conditions and do not prove stability for arbitrary initial states. Nonpositive depth or nonfinite states produce an error and retain the previous valid state; depth is not clipped to alter total mass.

The scope is a one-dimensional, CPU, flat-bottomed, fully wet tank, without 2D, GPU, terrain source terms or wet/dry boundaries.
