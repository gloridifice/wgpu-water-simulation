# Foam Shape Transport Comparison

`foam-lab` is an independent offline GPU comparison tool. By default, analytical flow fields distinguish “the flow did not deform the shape” from “numerical transport erased the deformation”; `--benchmark` separately compares performance of the full SWE kernel with fixed inputs. It neither reads nor writes personal configuration and does not open a window.

## Running

```powershell
# Default: three scenarios × three resolutions × two spatial orders, fixed RK2, snapshots at 0, 2, 4 and 8 simulation seconds.
cargo run --release --features ocean --bin foam-lab -- --output target/foam-lab

# Inspect vortex alone; the output directory must not exist, avoiding overwriting previous results.
cargo run --release --features ocean --bin foam-lab -- --scenario vortex --resolutions 256,512 --orders 2 --space-orders 1,2 --times 0,1,2,4,8 --output target/foam-lab-vortex
```

Open `index.html` in the output directory and consult `metrics.csv` for numerical results. Run arguments, GPU and production shader content fingerprints are stored in `manifest.json`. See `--help` for details.

Snapshots are taken at **simulation times**, not after waiting wall-clock seconds. Each interval is split according to the stable timestep, then GPU state is read back; CSV records requested time, actual accumulated time, interval step count and timestep. Devices supporting GPU timestamps also report interval GPU milliseconds and average GPU milliseconds per substep; otherwise those fields are empty. Timings accumulate real GPU time for batches of up to 64 substeps, including transport, sources and texture motion in the diagnostic entry point, but excluding idle time between submissions, readback and image export. This is not full Ocean frame time. The first interval may include warmup effects; repeat performance comparisons and inspect later intervals. Timestep is not compute time.

## Full SWE Kernel Performance Benchmark

```powershell
cargo run --release --features ocean --bin foam-lab -- --benchmark --resolutions 256,512 --output target/foam-perf
```

This mode is separate from the shape-preservation experiment and uses only `--resolutions` and `--output`; scenarios, orders, timestep and repetition counts are fixed, and `--scenario`, `--orders`, `--space-orders` and `--times` are not used. It calls the production `main` entry point of `Shore::solver_pipeline` and compares three combinations: first-order water plus first-order foam, second-order water plus first-order foam, and second-order water plus MUSCL foam. **There is no “water only” case with foam transport disabled.** In single-concentration scenarios the latter two combinations compare foam spatial-order cost. Dual-concentration scenarios do not use this spatial-order option: the latter two combinations check the same dual-concentration algorithm under different specialized pipelines, not two transport methods.

| Fixed scenario | Content |
| --- | --- |
| `beach-sparse` | Sloping bed, foam only within 4 m of the center |
| `beach-dense` | Same bed, continuously varying foam density across wet cells |
| `model-dense` | Uploaded synthetic height field with wet/dry boundary; not a real-scene benchmark of imported coastal assets |
| `obstacles-dense` | Sloping bed, eight static obstacles and dense foam |
| `model-dual-inlet` | Same synthetic model, dual concentrations, front/background emission and relaxation zones on all four sides; nonzero incident water levels and matching momentum for both phases |

The three combinations for each scenario and resolution share initial state, terrain and fixed timestep. The first four scenarios use zero FFT input and disable foam generation while retaining inertia, drag and decay. The added scenario uses dual-concentration lifetimes, initial R at 40% of total, FFT interpolation at 0.25/0.75 between previous and next fields in the two stages, and 0.75 for first order. It is not a real-scene benchmark with personal model assets or spray sources. Each combination warms up for eight substeps, then samples seven rounds of 32 repeated substeps, alternating execution order between rounds. Each repetition starts from the same initial state and recomputes the RK2 predictor; timing does not wait for foam to dissipate gradually.

`benchmark.json` records GPU milliseconds/substep per round, medians, timestep, inputs/terrain/outputs, complete parameters per stage, FFT previous/next field fingerprints and device details. Times are `null` without timestamp support. Each case also writes a `.bin` containing the complete five-block `vec4<f32>` state after **one fixed substep**, for comparing density, momentum and texture state across versions; it is not the state after 224 evolving steps. Files contain 80 bytes per cell; the 15 cases at 512² total approximately 300 MiB. `fixture_version=2` adds the dual-concentration incident-wave scenario and parameter/FFT fingerprints; inputs and timesteps of the original four scenarios are unchanged.

Timing covers the production SWE kernel and both RK2 stages, excluding parameter uploads, end-of-frame state synchronization in normal frames, FFT, spray, drawing, readback and first pipeline compilation. It compares fixed work, not UI frame rate, and cannot directly replace the panel's complete SWE timestamp. To compare cost per simulation second, divide milliseconds/substep by `dt_s`, but identify it as equivalent cost for repeated fixed inputs, not a real scene advanced continuously for one second. Across versions, first verify matching scenario, timestep, input/terrain, parameters and FFT fingerprints, then compare errors in all five output blocks and medians from repeated runs. Do not use shape-experiment sparse-flow timings to draw conclusions about dense coastal cost.

## Production-Path Offscreen Profiling

```powershell
cargo run --release --features ocean --bin ocean-profile -- --config configs/ocean-settings.json --output target/ocean-profile
# Interleave optical switch combinations in the same process and simulation state.
cargo run --release --features ocean --bin ocean-profile -- --config configs/experiments/reef-front-ridge.json --output target/optics-ab --optics-ab
cargo run --release --features ocean --bin ocean-profile -- --config configs/experiments/reef-front-ridge.json --output target/screen-optics-ab --screen-optics-ab
# Freeze effective parameters for a repeat run so personal configuration cannot change between runs.
cargo run --release --features ocean --bin ocean-profile -- --config target/ocean-profile/effective.json --resolution 512 --output target/ocean-profile-512
```

`--screen-optics-ab` rotates four variants each frame under normal model-scene shading: baseline, physical refraction only, SSR only, and both enabled. Caustics and simulation parameters remain fixed, while sky reflection and sun highlights remain enabled. Use `screen_depth_pyramid` for depth pyramid cost and each variant's `water_shading` difference from the same-frame baseline to estimate incremental optical cost. Ordinary render statistics in the output describe the baseline; raw timing for all four variants is in `samples[].optics_ab_gpu_ms`.

`ocean-profile` loads the explicit configuration strictly read-only and uses actual model assets and production simulation/drawing calls; it does not overwrite personal settings. By default it runs 30 warmup frames and samples 120 frames; `--frames` accepts 1–1000. Every frame advances exactly 1/60 simulated second at 1280×800, ignoring saved pause state and playback speed, without window, GUI, vertical sync or presentation. `--resolution` overrides only SWE resolution; `--no-spray` disables spray for same-state A/B without disabling surface foam.

The output directory must not exist and its parent must exist. Outputs include:

- `effective.json`: a copy of the effective configuration.
- `profile.json`: device, actual substep count and advancement per frame, GPU milliseconds for five phases and their medians. The order is FFT, offshore foam, full SWE, spray particles and drawing. Full SWE includes substep parameter copies, spray sources, end-of-frame synchronization and FFT history copies; drawing includes scene and spray overlay. Devices supporting encoder/pass timestamps also report `render_components`, `render_median_gpu_ms` and per-frame `render_gpu_ms`, separating sky LUT, height-field hierarchy update, caustic tracing, caustic rasterization, opaque prepass, screen-depth hierarchy, sky/opaque final drawing, water shading and wireframe/spray overlay; unsupported devices retain the five-phase statistics. Initialization is separately stored in `initial_frame_gpu_ms`/`initial_frame_render_gpu_ms` and excluded from warmed-up medians. The static height-field hierarchy is reused on ordinary frames; inspect initialization for its rebuild cost.
- `final-state.bin`: final complete five-block SWE state, little-endian f32, 80 bytes per cell.
- `final.rgba`: final raw RGBA8 sRGB pixels, 1280×800, without header or row padding; not PNG.

The tool requires GPU timestamps and uses empty compute passes for coarse phase boundaries. Detailed drawing timings write timestamps directly without splitting production render passes, though measurement has a small cost. Queries are batched according to device limits for 1–1000 sample frames; readback happens only after all frames are submitted. Water shading includes refraction tracing, optical integration and foam material; compare refraction on/off with the same configuration to estimate incremental refraction, rather than attributing all water shading to it. Dry-block statistics only check whether each 16×16 block and its two-cell neighborhood are entirely dry in the last frame; they do not prove it is safe to skip that block in both RK stages or later. Zero foam density does not imply inactive hydrodynamics.

For A/B, first compare effective configuration, actual substep counts and advancement, then compare state and timing. Images with spray can differ by a few pixels at transparent overlaps due to GPU atomic particle-compaction order; use `--no-spray` for a fixed water-rendering comparison. Background GPU load affects timestamp intervals; repeat with interleaving and do not treat a disturbed first result as a speedup.

`--optics-methods` uses the same interleaved timing procedure to compare “all off, physical caustics with fast refraction, physical caustics with physical refraction, original configuration”; see `optics_ab_variants` for output order. The render entry in `gpu_ms` corresponds to the original configuration. This compares fast/physical modes, or uses a mixed original configuration as the fourth variant. Disabled passes incur only timestamp-boundary overhead, which should not be interpreted as actual intersection cost.

`--optics-ab` requires encoder/pass timestamps. Each simulation frame advances physics once, then interleaves drawing of “all off, caustics only, refraction only, all on” at the same state, rotating order each frame to reduce cache/frequency bias. `optics_ab_median_gpu_ms` and `optics_ab_render_median_ms` record component and total drawing medians for all four; `samples[].optics_ab_gpu_ms` retains paired samples, while `initial_frame_optics_ab_gpu_ms` records first-run details (the height-field hierarchy is first built in the first combination enabling caustics). The render entry among the five `gpu_ms` phases is the variant selected by configuration, not all four draws added together, so this is not actual window frame rate. `final_image_variant` identifies the final image variant (0–3 in the order above). A/B does not save parameters or advance extra physics steps; its final state can be compared byte-for-byte with the same frame in ordinary mode.

Panel SWE timing ends at the last solver/spray-source pass and excludes end-of-frame synchronization; this tool's SWE phase covers all of `Shore::encode`, so the two are not directly equivalent. Fixed-input `foam-lab --benchmark` does not run production scheduling and cannot measure end-of-frame copy optimizations.

## Scenarios and Controlled Variables

The simulation domain is fixed at −16 to 16 m in X and Z, with 2 m water depth. Initial foam density is 0 or 1, area-averaged using 8×8 subsampling where boundaries cross cells. All three velocity fields are continuously incompressible; their continuous solutions do not spontaneously lose density.

| Scenario | Initial foam | Prescribed flow |
| --- | --- | --- |
| `translation` | Strip centered at (−5,0) m, 0.5 m wide and 8 m long | X velocity 0.75 m/s, Z velocity 0.25 m/s |
| `shear` | Annulus at origin, outer radius 2.5 m, inner radius 1.5 m | X velocity = 0.5 /s × Z coordinate, zero Z velocity |
| `vortex` | Eccentric annulus centered at (3,0) m, outer radius 1.75 m, inner radius 1 m | Rotation around origin; angular velocity = 0.8 × exp(−radius squared / 25 m²) rad/s |

Common settings:

- Generation and decay are zero; drag is enabled, following multiplier 1, response time 0, so foam immediately follows prescribed velocity, excluding differences in inertial response.
- Default resolutions are 128², 256² and 512², corresponding to 0.25, 0.125 and 0.0625 m/cell; existing levels from 64 to 1024 are supported.
- `--orders` selects **time integration**: 1 is Euler, 2 is RK2, default 2 only. `--space-orders` selects **spatial transport**: 1 is the original first-order Rusanov, 2 is limited MUSCL, default 1,2. The default comparison changes only the spatial method. The tool also allows Euler + MUSCL, whose overall temporal accuracy remains first order.
- Both spatial methods use the same two-dimensional advection CFL limit of **0.2**, without the SWE wave-speed limit; the same resolution and flow use the same timestep. Older CFL=0.4 results are not a strict A/B baseline for the new scheme; rerun the first-order comparison.
- Times start at 0, strictly increase and do not exceed 8 seconds; analytical structures remain far from closed boundaries in this range. There are no random seeds or perturbations.

## Production Code Used

`src/bin/foam_lab.rs` concatenates `src/ocean/simulation/nearshore/shaders/shore.wgsl`, `shore_compute.wgsl` in the same directory, and the diagnostic entry `src/bin/foam_lab/shaders/transport.wgsl` in their original order, embedded at compile time with `include_str!`. Production shader fingerprints also read the new paths.

The diagnostic entry calls production `transport_foam`, `foam_sources` and `advance_foam_motion`; density and momentum use production fluxes, and texture displacement advances with simulation. The diagnostic entry orchestrates RK2 prediction/correction. Depth and flow remain prescribed; **full SWE, FFT input and nearshore foam generation are not executed**. No separate CPU transport algorithm is copied. Spatial order uses the same uniform control as normal Ocean; second-order foam transport in normal Ocean requires second-order SWE and defaults to first order.

The independent analytical reference uses exact reverse streamline mapping: translation subtracts velocity times time, shear subtracts the corresponding lateral displacement, and vortex rotates backward by the radius at the current position. Reference images and error comparisons use area subsampling and do not depend on numerical simulation results.

## Images and Metrics

All images use a fixed 32×32 m top-down view, +X right and +Z up. Black is density 0, white is density 1, with **no per-image contrast enhancement**. PNGs are GPU density snapshots, not ordinary Ocean material or window screenshots: they do not pass through NSH, opacity curves, lighting or two-second texture updates.

- First row: analytical reference.
- Subsequent rows: GPU density at each resolution, time order and spatial order; `-o2-s1-` in a filename means RK2 time integration and first-order spatial transport.
- Red images: absolute error against the analytical reference at the same resolution, with a fixed 0–1 scale.
- Click an image for its original 512×512 view; low resolutions are enlarged by cell, and higher resolutions are downsampled by area averaging. Image display clips to 0–1, but CSV metrics use unclipped floating-point density.

| CSV metric | Meaning |
| --- | --- |
| `time_order` / `space_order` | Time and space methods recorded independently to avoid confusion |
| `gpu_ms` / `gpu_ms_per_step` | Real GPU milliseconds for this interval / average per substep; empty without timestamp support |
| `relative_mass_error` | Change relative to initial total foam, not peak change |
| `relative_l1` | Total absolute density error divided by total analytical reference; can exceed 1 and does not mean more than 100% of foam was lost |
| `peak` | Current maximum density, initially 1 |
| `area_density_ge_half_m2` | Area with density at least 0.5; indicates whether dense structures remain, not used in simulation |
| `centroid_error_m` | Density centroid deviation from the analytical reference |
| `x_sigma_m` | Density standard deviation along X; measures numerical diffusion in translation, but also includes real deformation in other scenarios |

Runtime checks require finite values, nonnegative density, total error at most 0.1%, and accumulated simulation-time error at most 10 microseconds. Shape error and peak have no pass thresholds, avoiding tuning the problem under examination merely to pass.

## Interpreting Results

Compare translation first: the continuous solution should not change strip width or peak. If mass is conserved and the centroid correct, but the strip widens and its peak falls, that is clear evidence of numerical diffusion.

Then inspect shear and vortex: the analytical reference shows the shapes the prescribed flow can produce. If the reference forms clear filaments but the numerical result becomes a blurred band, the issue cannot be attributed solely to “lack of complex flow.” Compare resolutions to assess refinement and time orders to see whether RK2 is sufficient.

This experiment cannot quantify the individual effects of flow resolution, generation distribution, inertia, decay and material in a real coastal scene, nor does it validate real foam cohesion/breakup physics. It isolates the shape-preservation capability of existing transport. MUSCL reduces spatial diffusion; it does not create turbulence or new holes, and cannot preserve sub-grid filaments forever. Real coasts still require comparisons at the same time and camera.

## Automated Shape-Preservation Verification

```powershell
# GPU tests are run explicitly; ordinary library unit tests do not require a GPU.
cargo test --release --features ocean --bin foam-lab -- --ignored
```

Compare the three analytical flows at 256², RK2 and 8 simulation seconds. Second order must reduce relative L1 error by at least 20%, total error must be less than 0.01%, and these incompressible scenarios must not exhibit significant density overshoot. Nonnegative and finite checks reuse the tool's readback path. Compression in real coasts can raise density above 1; do not apply this upper bound to the production solver. Momentum, near-vacuum, obstacles, wet/dry boundaries and full coastal evolution are separately covered by `ocean --verify`.
