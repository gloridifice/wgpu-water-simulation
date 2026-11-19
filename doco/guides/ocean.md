# 3D FFT Ocean (wgpu + egui)

An ocean surface demonstration running in a standalone window. The CPU initializes the Phillips spectrum; the GPU performs spectral evolution, a two-dimensional inverse FFT, and surface rendering. By default it uses 128×128 frequencies and a 128-meter periodic patch, rendered as 3×3 repeated patches. `cargo run` starts this program by default; the CPU PIC / shallow-water TUI is in the separate workspace member `crates/tui-2d-prototype`, whose binary is still named `water-simulation`.

## Running

```powershell
Set-Location G:\MyProjects\Rust\water-simulation
cargo run

# Optimized build for visual and performance comparisons.
cargo run --release

# Change the spectral resolution, wind speed, and random seed.
cargo run --release --features ocean --bin ocean -- --resolution 256 --wind 18 --seed 7
```

A graphics device supporting compute shaders and vertex-stage storage buffers is required. wgpu uses a native backend, and assets are supplied with the repository. The first build downloads graphics dependencies. The `ocean` feature is enabled by default; use `cargo run -p tui-2d-prototype` to start the CPU TUI without compiling graphics dependencies, or add `-- --shallow` for the one-dimensional shallow-water mode.

| Action | Effect |
| --- | --- |
| Drag with the left mouse button | Rotate the camera around the current target |
| Drag with the middle mouse button | Pan the camera and target horizontally; the image follows the drag |
| Mouse wheel | Zoom in / out |
| W / S | Move forward / backward horizontally relative to the current orientation |
| A / D | Pan left / right horizontally relative to the current orientation |
| Space | Pause / resume |
| C | Toggle horizontal displacement to compare rounded and sharp crests |
| F | Toggle Jacobian foam display |
| ↑ / ↓ | Decrease / increase displacement amplitude, 0.1×–2× |
| R | Reset the camera, displacement amplitude, foam display, and time; preserve the pause state |
| Ctrl+S | Save the current working state and configuration library (without overwriting named snapshots) |
| Q | Quit (Esc does not quit) |

Middle-button panning must start inside the Render View; dragging into the view from a parameter panel does not move the camera. Once started, panning continues outside the view until the button is released. Camera height, orientation, and viewing distance remain unchanged; pan distance is derived from drag distance, viewing distance, and view size, independently of pause and playback speed. The pan position follows the camera's manual save rules.

WASD works when the pointer is inside the Render View, the window has focus, and no parameter is being edited. Holding a key moves continuously, translating the camera and target together without changing height or pitch; diagonal movement is not faster. Movement speed is 0.5 times the current camera-to-target distance per second of real time, independent of pause and playback speed. The pan position is saved with the camera; old configurations start at the origin. R, Reset Camera, and the beach camera preset each restore their respective initial positions.

The title bar shows frame rate, time, pause state, and amplitude. PIC click disturbances are not connected to the FFT; here the mouse controls the camera.

## Headless MCP Debugging

`--headless` starts one persistent Ocean scene without creating a window, Surface, egui, or Dock. The process communicates with its launching client over **MCP stdio**; the scene does not automatically advance with wall-clock time. `--no-ui` retains a single scene window and MCP stdio, but creates neither egui nor Dock. The window advances with wall-clock time and can still process MCP commands when its size is zero or the Surface is temporarily unavailable. Each mode controls only its own scene instance. stdout is reserved for protocol messages; GPU logs go to stderr. For automation, either specify a read-only configuration with `--config` or start from defaults without specifying a configuration; personal sessions are not implicitly read or overwritten:

```powershell
# Configure the command and arguments in an MCP client; do not enter JSON as if this were an ordinary interactive command.
command: target\debug\ocean.exe
args: ["--headless", "--lake", "--no-settings"]
# Alternatively: args: ["--headless", "--config", "configs/example.json"]
# To display a scene-only window: args: ["--no-ui", "--beach", "--no-settings"]
```

Clients discover `scene_describe`, `parameters_get`, `parameters_patch`, `action_call`, `simulation_step`, and `frame_capture` through `tools/list`. For example, `parameters_patch` accepts `{"changes":{"optics.lake.automatic":false,"camera.gain":1.5}}`; field paths come from the `settings` object returned by `parameters_get`. Only existing scalar fields can be written, with numeric indices for array elements (such as `camera.target.0`); whole objects and arrays cannot be replaced. Unknown fields, out-of-range values, invalid types, or an invalid resulting configuration cause an error and no changes are committed. `draft.*` is the offshore draft; after editing it, call `action_call({"id":"ocean.apply_draft"})`. Writes to `ocean.*` apply directly. Lake `optics.lake.*` settings apply directly in panel-free modes; changing terrain, propagation mode, or seed resets the lake, and the UI's lake drafts are not available. `speed` and `paused` can be read and edited, but the headless mode advances only by explicit steps and ignores wall-clock time and speed; one `simulation_step({"frames":60})` advances 1 simulated second (at most 600 frames). Stepping can take time; an MCP response follows GPU submission and **does not mean GPU execution has completed**. `frame_capture` uses the current instance's scene texture, writes to a PNG path that does not exist, and waits for readback; it neither initializes another scene nor includes panels. In `--no-ui`, automatic playback obeys pause and time speed, but explicit `simulation_step` remains fixed at 1/60 simulated second per step, independent of both; capture dimensions follow the window.

`action_call` currently supports `ocean.apply_draft`, `simulation.reset`, `camera.reset`, lake-mode `lake.emit` / `lake.clear`, and `configuration.save`. There is no general simulated-click capability for unlisted UI buttons; `scene_describe` returns action IDs. Saving in the default, `--config`, or `--no-settings` modes returns a read-only error. Only an explicit `--settings PATH` followed by `configuration.save` saves the working state and original configuration library; it does not overwrite named snapshots or write the Dock layout. Use a separate path for automation, not a personal session file. Only local stdio is currently supported: there is no HTTP endpoint or external attachment to a parameter-panel window started with ordinary `cargo run`. For Pi, `.pi/mcp.json` at the repository root configures a default headless service named `ocean-scene`; load `.agents/skills/ocean-scene-mcp/SKILL.md` as needed. Run `cargo build --features ocean --bin ocean` before the first connection. The configuration starts the existing debug executable directly, avoiding conflicts between Windows executable locks and rebuilding. After source changes, close the MCP subprocess before manually rebuilding and reconnecting. The project must be trusted; failures of other global MCP services do not affect this service.

## egui Parameter Panel

Shared controls, lists, and the egui / Dock theme use the `yummy_egui` 0.1 series from crates.io (minimum 0.1.2), together with egui 0.34, egui_dock 0.19, and wgpu 29. Ocean directly applies the library's default theme; this repository no longer retains the control library source or theme configuration, nor does it read, watch, or save external themes. MiSans remains in `assets/fonts/MiSans-Normal.ttf` and is explicitly installed by `ui/fonts.rs`; the theme does not replace fonts. Simulation configurations, named configurations, and Dock layouts are still saved under the existing rules and are unaffected by the theme migration. See [yummy_egui integration](yummy-egui.md).

The contents of all Dock panels, including the Render View, have **10 pt** of padding on all four sides by default; the outer Dock area has no top margin and **4 pt** on the left, right, and bottom. Use **Panels → Dock Margin Debug** in the top bar to open a floating window that is collapsed by default. Its two sliders adjust panel content padding on all four sides and Dock area margins on the left / right / bottom (the top is fixed at 0), each over 0–32 logical points (pt). Dragging or editing values previews changes live without resetting the simulation. At 100% scaling, 1 pt = 1 px; on high-DPI displays it follows UI scaling. Initial values come from the embedded theme; debug adjustments last only for the current run and are not written to sessions, named configurations, or layout files.

Parameter-panel action buttons fill their content area: a single button occupies a full row, while multiple buttons share the width equally after theme spacing is deducted. If each button would be less than 96 pt after equal division and deduction of button margins, the entire group is arranged vertically. Long text may wrap and is centered horizontally and vertically in the text content area after padding is deducted. Action buttons in parameter tables fill only the right-hand control cell, not the name column. The top toolbar, menus, selection buttons, and inline buttons such as “New” next to the seed input retain their existing layout; click, disabled, and hover-tooltip semantics are unchanged.

Panel action button padding follows the theme (4 pt left and right, 1 pt top and bottom); margin defaults to **4 pt** above and below and **0** on the left and right in the embedded component theme. Margins do not respond to clicks and are added to theme control spacing.

Parameter sliders use a **rounded background + thin vertical bar**, with numeric editing retained on the right. Slider value fields and standalone numeric fields share the theme's **22 pt** minimum interaction height and button padding; the background matches the actual value-field height, while 64 pt remains reserved for width. The theme minimum height also applies to other controls that follow theme sizing, and controls may grow when fonts or padding need more space; corner rounding follows the theme (currently 2 pt). The normal bar is 2 pt wide and inset 4.1 pt at the top and bottom; on hover / drag it is 4.8 pt wide and extends 0.5 pt at the top and bottom, with a 57 ms transition and no outline. Each side has 2 pt internal spacing, measured from the outer edge of the widest bar; there is no progress fill. Existing linear / logarithmic, integer / step, and numeric-editing semantics remain unchanged. The appearance experiment window and production panels share controls; see [slider appearance experiment](slider-lab.md).

Panels can be rearranged by dragging Dock tabs; small windows can scroll. After changing the spectrum, dimensions, or seed, click **Apply Changes**; playback and appearance parameters take effect immediately. **Restore Defaults** restores only the pending spectrum parameters, which take effect after applying.

| Control | Meaning |
| --- | --- |
| A (power) | Phillips spectrum power coefficient, default 0.002; 0 produces a flat surface, while multiplying it by 4 doubles wave amplitude |
| L (m) | Phillips large-wave scale, approximately 19.98 m by default, independent of patch size; displays equivalent wind speed sqrt(gL) |
| Wind angle (deg) | Unit wind direction omega-hat in the formula, derived from the angle as (cos θ, sin θ); not temporal angular frequency |
| Patch side (m) | Side length of the square periodic ocean patch; the scene draws 3×3 patches |
| FFT spectrum | N×N resolution of the frequency-domain array, a power of 2 from 8 to 512 |
| Offshore mesh density | 8–512, default 128; cells per axis in square mode, or a density budget for a similar triangle count in near-equilateral triangle mode; may differ from FFT resolution |
| Near-equilateral triangle mesh | Off by default; immediately switches offshore drawing topology, allowing A/B comparison while paused without applying other spectrum drafts or clearing simulation history |
| Seed / New | Fixed random seed / choose another random ocean |
| Paused / Restart time / Time speed | Pause, reset time to zero, playback multiplier |
| Wave height gain / Horizontal choppiness / Show foam | Render displacement multiplier, horizontal displacement toggle, foam display |

The mesh reads FFT height and displacement using periodic bilinear interpolation; increasing mesh density does not invent waves absent from the spectrum. A mesh coarser than the FFT may miss short-wave shapes. If only offshore mesh density changes, applying rebuilds only the drawing mesh and does not reset FFT, SWE, foam, or spray; spectrum / size / seed changes still rebuild compute buffers and mesh bindings. Ocean retains current time and pause state, adjusting camera distance proportionally when dimensions change; procedural water rebuilds and restarts its clock while preserving pause and camera. Panel input takes precedence, so it does not simultaneously rotate the camera or trigger ocean shortcuts.

**Offshore → Spectrum and Waveform → Waveform → Near-equilateral Triangle Mesh** is an immediate toggle; old configurations without `ocean.triangular_mesh` still use squares. The new mode offsets alternate rows by half a cell and alternates triangle diagonals; an even number of rows ensures top-to-bottom periodicity. Zigzag left and right boundaries are shared by adjacent patches without inserting extra narrow boundary triangles. At density 128 it uses 119×138 cells and 32,844 triangles per patch, only about 0.23% more than the square mesh's 32,768; triangle count deviation is less than 2.5% over the 8–512 density range. FFT storage / sampling, displacement, the normal algorithm, and the local SWE water mesh remain unchanged; the option compares mesh directionality and does not guarantee elimination of specular aliasing. It is saved manually with the current session / named configuration.

The command line also supports `--amplitude 0.002 --phillips-length 20 --wind-angle 45 --mesh-resolution 192`, and `--triangular-mesh` / `--square-mesh` explicitly override offshore drawing topology. The old `--wind 14` remains available and sets `L=U²/g`; if `--phillips-length` is also supplied, the later option wins.

## Procedural Lake Animation (No SWE)

```powershell
# Irregular shoreline, procedural rocks, and depth-contour ripples.
cargo run --release -- --lake --no-settings
# Use a two-dimensional Euclidean distance field with exposed-rock boundaries instead.
cargo run --release -- --lake --lake-mode sdf --no-settings
# Independent GPU compute, rendering, optical isolation, and timing verification.
target\release\ocean.exe --verify-lake
# Offscreen comparison with fixed seed, camera, and simulation time; the destination must not exist.
target\release\ocean.exe --lake --lake-mode sdf --no-settings --snapshot lake.png --time 7
```

The scene is also available under **Scene and Terrain → Lake Scene**. Terrain and FFT strength are in the **Lake Terrain** panel in the Scene category; propagation, emission, waveform, reflection, and diagnostics have moved to the separate **Animated SDF Ripples** sidebar icon (between Offshore and Shading), still supporting both depth and SDF modes. Each of the six categories retains its own layout; old layouts are not overwritten, and the new category automatically uses its default layout. It can also be opened from the Scene button or the top Panels menu. Switching categories does not reset the simulation, camera, or drafts. Ocean and procedural water share the FFT spectrum and water shading; the lake advances FFT as needed but does not advance SWE / foam / spray, and draws only the local lake surface.

The shoreline is generated from periodic radial noise, and the lakebed and rocks share a single-layer heightfield without overhangs or caves. Rock placement avoids existing rocks and skips candidates when space is insufficient; the panel shows the actual count. Every third candidate is an underwater rock. Scene and ripple seeds are separate. Terrain defaults to 256², with 128² / 512² available.

- **Depth**: the propagation coordinate is still-water depth divided by the maximum water depth in the field. Rocks change depth contours, including underwater rocks.
- **SDF**: the propagation coordinate is positive shore distance divided by the maximum positive shore distance in the field. Shore distance is the two-dimensional Euclidean distance to the outer shoreline or the waterline of exposed rocks, positive in water and negative on land; underwater rocks do not form SDF obstacles.

Both coordinates are dimensionless, with 0 at the boundary and 1 at the maximum. Incoming waves advance from near shore into the lake, increasing their coordinate over time; returning waves decrease it. **Propagation Progress / Second** in the panel is not horizontal meters / second, and **Propagation Coordinate Width** is not a width in world meters. The two modes save their inner turn coordinate, speed, width, and nearshore emission coordinate separately. Depth plateaus, the SDF medial axis, and contour splitting change ripple shapes; the SDF is not a geodesic travel-time field and does not provide real diffraction or energy transport behind obstacles.

A world direction angle of 0° points along +X and 90° along +Z; angular half-width describes a sector around the lake center and is continuous across ±180°. Ripples are thick in the center, taper and fade at both ends, and retain their angular range as they advance. Direction, sector width, thickness, and nearshore starting point are randomized independently and are reproducible with a fixed seed; **Sector Width Randomness** / **Thickness Randomness** range from 0–1, default 0.35, and **Start Random Offset** ranges from 0–0.3 in normalized coordinates, default 0.04. Setting each to 0 disables the corresponding randomness. Direction, amplitude, shape, speed, and reflection parameters affect only new incoming waves and their descendants, not the motion of existing waves.

An incoming wave is born near the **Nearshore Emission Coordinate**. On reaching the **Inner Turn Coordinate**, it briefly fades out while stationary on that contour and generates a weaker reflected wave at the exact arrival time. The reflection retains the original sector and thickness, returns toward that wave's actual nearshore starting point, and decays according to its half-life; it does not reflect a second time. This is procedural reverse animation, not specular reflection about a local normal. Clearing, expiration, and a full pool do not trigger reflection. The total pool holds 128 waves; incoming waves with reflection enabled reserve descendant slots, and a full pool rejects additions rather than overwriting visible waves.

Terrain and propagation settings have separate drafts: **Lake Terrain → Apply Terrain and Reset** applies only terrain; **Animated SDF Ripples → Apply Propagation Field / Seed and Reset** applies only depth / SDF mode and the ripple random seed, without including unapplied drafts from the other panel. Both reset the clock and wave sequence while preserving pause and camera. Emission parameter edits immediately affect subsequent waves; changing the automatic emission interval reschedules from the current time. **Emit One** can create a record while paused, but birth fade-in and movement require playback to resume; **Clear Ripples** also cancels pending reflection events without emitting an old backlog. Diagnostics provide depth, signed SDF, propagation contours, and wave height.

**Shared FFT Waves**: the **FFT Overlay Strength** of a new lake preset is 0.2 and can be adjusted live from 0–1 under **Scene → Lake Terrain** or **Offshore → Spectrum and Waveform**; 0 disables it. Spectrum strength, scale, wind direction, seed, and FFT resolution share the original `ocean` parameters; click **Apply Changes** after editing the draft. No separate lake spectrum is saved. The lake overlays vertical wave height only and does not use horizontal choppiness. **FFT Shore Fade** is enabled by default, reducing wave height in 0–0.5 meters of shallow water and within 0–1 meters of shore distance; it can be switched off live without a reset. Regardless of the toggle, dry land remains masked, final height remains constrained by the lakebed, and consistent normals are then calculated. FFT does not reflect toward shore along depth contours / SDF; that remains the responsibility of the existing shoreline wave animation. Pause freezes both; **Clear Ripples** clears only shoreline waves and retains FFT. Applying a new spectrum rebuilds and restarts the clock while preserving camera and pause. Old lake configurations without `fft_strength` default to 0, so their appearance does not change automatically.

Lake and ocean share water extinction, scattering, sky reflection, highlights, SSR, fast screen refraction, and HDR/Bloom through the same water material module; water displacement and normals share GPU wave fields. **Physical caustics and physical refraction are not yet supported**; the original settings are retained but not encoded, and the UI explains why without automatically falling back to fast refraction. Shortcuts such as C / F / ocean wave height gain do not change the lake; lake FFT overlay strength and shoreline wave amplitude are set independently.

Lake parameters and seeds are manually saved with existing sessions / named configurations; active waves, playback time, and counts are not saved, and reloading starts from the initial state. Old configurations missing these fields retain the original ocean mode. `--lake` is a complete scene preset and overrides saved scene parameters; do not repeat the preset when restoring personal settings. See the [animated lake specification](../specs/animated-lake.md) for exact behavior.

## Scanned Coastal Model and Adjustable SWE Region

A **Model and Region** Dock tab has been added. The model scene uses `assets/models/challaborough_pillbox.glb` to display a rocky coast and a prebaked `.sweh` heightfield to drive SWE; the original beach / flat scenes remain available, and the default startup scene is unchanged. See [`assets/models/README.md`](../../assets/models/README.md) for asset origins, CC BY 4.0 attribution, cut-surface cleanup, and disk format.

```powershell
# Generate the disk heightfield for the first time; existing output is not overwritten. No GPU is required.
cargo run --release --bin terrain-bake

# Start the model preset without overwriting personal settings.
cargo run --release --bin ocean -- --model --no-settings

# Set bounding-box center, size, and mesh resolution; place overrides after the scene preset.
cargo run --release --bin ocean -- --model --no-settings --swe-center 0,4,0 --swe-size 128,64,128 --swe-resolution 512

# Dedicated real GPU verification, independent of scanned assets and without reading or writing personal settings.
cargo run --release --bin ocean -- --verify-terrain
```

The model preset overrides scene parameters in the session, like `--beach`. To restore your saved model placement, use only `cargo run` and do not repeat `--model`. Model parameters, the applied bounding box, open boundaries, and display toggles are included in sessions and named configurations; old configurations without model fields default to disabled and continue using the existing scene. Unapplied drafts are not saved. Command-line snapshots use explicit parameters by default; add `--config PATH` to load a specified configuration read-only, including camera and offshore foam parameters.

### Offline Assets and Runtime Rebuilding

The offline tool generates 984×890 cell-center heights and valid-coverage flags at a spacing of approximately 0.05 model units; the result is loaded once from disk at initialization and checked against the source GLB SHA-256. Changing the region does not reread the GLB or intersect individual triangles. Currently the CPU performs O(N²) resampling of the resident source heightfield and uploads a static GPU bed buffer; solver substeps only read that buffer. This is not GPU rebaking and does not resample every frame. The rebuild time in milliseconds shown in the console is CPU resampling and upload-call time, not a GPU timestamp.

Valid scanned regions use model height rather than taking the maximum with the original sand slope everywhere. Outside the scanned bounding box, the nearest edge height transitions smoothly over the adjustable **Seabed Transition Width** to the seabed depth set in the water optics panel; invalid internal samples use that background depth. The supplemental seabed is not locally surveyed underwater terrain and does not automatically repair scanned caves. A top-down highest surface is only a single-layer terrain proxy; it cannot simulate flow under overhangs, water simultaneously in caves and on roofs, or real three-dimensional fluid breakup. Textured spray is only a visual supplement.

### Parameters and Application Semantics

| Parameter | Meaning and application |
| --- | --- |
| Enable Model | After applying, switches between the model and original scene and reinitializes nearshore history |
| Model Position X/Y/Z (m), Uniform Scale, Rotation Y (deg) | Adjust placement; uniform scale is meters / model unit. Applying resamples the source heightfield without offline rebaking. Pitch / roll are unsupported and require rebaking |
| Sea Level Elevation (m) | Uses the same elevation reference as model position Y; internal rendering and solving use this sea level as Y=0. Changes are static scene edits and reset the simulation, not dynamic tides |
| Bounding Box Center / Size X/Y/Z (m) | XZ determine the actual two-dimensional computational domain; Y is only for wireframe and range checks, does not clip the bed, and is neither a three-dimensional grid nor a closed top |
| Auto Fit to Model Bounds | After loading the asset, automatically calculates a near-square region from the transformed bounding box and sponge band, leaving room for flow around the model; first changes the draft, then requires Apply |
| Grid Cells per Axis | Select 64–1024 in the SWE tab; independent of the offline heightfield and FFT. The model scene also draws a local water mesh at SWE density |
| Four Open Edge Toggles | Ordered −X, +X, −Z, +Z; enabled means zero-gradient open boundary plus FFT relaxation input, disabled means reflective boundary. Open boundaries allow foam and foam momentum to exit without importing external foam |
| Sponge Width (m) | Transition width at each open edge, which must be less than half the smaller X/Z dimension. The model and main interaction region should avoid this band; corners use the maximum weight without repeated accumulation |
| Seabed Transition Width (m) | Default 12 meters, the transition distance for the artificial seabed outside the scan; it should remain within the computational domain and sponge band |
| Show Bounding Box | Orange three-dimensional wireframe; appearance only |

Model and region controls edit a draft and take effect only after clicking **Apply Model and Region**. Changes to XZ bounds, model transform, water level, boundaries, supplemental seabed, or resolution rebuild / reinitialize nearshore water and clear nearshore foam and wetness history; FFT time and offshore foam continue advancing. Changing only the Y display range and wireframe toggle does not reset. The seabed depth control on the water optics page still applies immediately; changing it in the model scene immediately rebuilds the supplemental seabed and resets. Applying is possible while paused; conservative history transfer between different computational grids has not yet been implemented.

The SWE surface dominates inside the model and main interaction area, blending back to FFT at the outer edge; coupling remains **one-way FFT → SWE**. The original input's water level / two momentum channels, shared gain, and short-wave filtering are retained. The current open treatment is not a strictly nonreflecting characteristic boundary; sponge width must be adjusted to the input wavelength, and reflection coefficients have not yet been calibrated. Increasing mesh resolution also does not fix the phase-speed difference between deep-water FFT and nondispersive SWE.

The model is written both to the opaque color / depth prepass and the final scene; underwater portions participate in the existing optical path integration. Nearshore water interpolation ignores dry-land heights, and foam transport is blocked by terrain that is not submerged; wetness and residue project only onto model surfaces close to the bed instead of covering entire vertical rock walls. There is no separate rock-wall water-mark model.

Use `--model-position X,Y,Z`, `--model-scale N`, `--model-yaw DEG`, `--sea-level M`, `--swe-center X,Y,Z`, `--swe-size X,Y,Z`, `--swe-resolution N`, `--swe-sponge M`, `--swe-open-edges 1,1,1,1`, and `--hide-swe-box` to set initial parameters. Model scale and default water level are for preview only and have not been calibrated against field measurements.

`--verify-terrain` uses the production GPU solver path and covers analytic sloped-bed upload for first / second order, wet/dry still water, mass conservation of a moving water pulse, Y-range / wireframe edits preserving pause state, domain-change resampling while paused, and still-water preservation at all four open edges. CPU library tests cover disk asset round trips, rejection of source hash mismatches / truncation, analytic bed sampling after rotation and scaling, and invalid coverage. The full `--verify` also includes these GPU checks.

## Prototype Holes and Continuous Deformation (Migration Phase One)

**Prototype Hole Appearance** and **Coordinate Reset Period (seconds)** have been added to “Foam Texture and Material”. Old configurations still default to the original material and a 2-second period. This phase migrates only multiscale noise holes on transported coordinates, not the prototype's fine-wave propagation or new rock-impact source; it does not change generation, density / momentum flux, drag, decay, or hydrodynamics.

- The prototype appearance samples value noise at 2.5, 7, and 17 times world coordinates on existing foam transport coordinates, with weights 0.55 / 0.30 / 0.15. It uses the project's noise function and does not guarantee a pixel-identical random pattern to the HTML. Subpixel noise fades to its mean to suppress distant flicker. Zero-density / dry regions strictly show no foam.
- In prototype mode, opacity density scale controls the response of the hole threshold to `1-exp(-density/scale)`; `1/6` corresponds to the HTML coefficient 6. This differs from NSH's “hole coverage × continuous opacity” and does not clamp simulation density. The prototype uses fixed noise scales, not NSH texture size, micro normals, or roughness; color and water optics are retained.
- Coordinate reset period is 0 or 0.25–60 simulated seconds. The default is 2; 8 seconds reveals a longer stretching history; 0 transports continuously without resetting and may overstretch over time. Coordinates still follow foam velocity rather than a separate water flow, keeping density and coordinates aligned. Changing the period does not clear foam, but may change phase blending; reset the simulation for a strict comparison.
- Material debug `--foam-material-view 4` excludes highlights and displays final coverage; `5` displays a 0.5-meter transport-coordinate grid, independent of foam density. In prototype mode, Soft is noise and Height is zero.

Read-only preview without changing personal or repository sessions:

```powershell
cargo run --release --bin ocean -- --config configs/experiments/reef-front-ridge.json --no-foam-ridges --foam-prototype --foam-texture-period 8
```

Fix the above configuration, camera, and `--time 12`, and cross-compare `--foam-prototype` / `--no-foam-prototype` with `--foam-texture-period 2` / `8`, retaining `--no-foam-ridges` on both sides to avoid mixing in source changes. Add `--snapshot target/custom-name.png --foam-material-view 4` if desired; the output file must not exist. Density debugging still uses `--shore-view 6`; density screenshots for the old material / 2 seconds and prototype / 8 seconds have the same SHA-256. Coverage images show more fine streaks and holes, but continuous white patches remain; this does not establish that the prototype or real turbulence has been reproduced.

## Reef Compression Ridge Experiment

`configs/experiments/reef-front-ridge.json` is derived from “Reef” in the original configuration library. Use `--config` for a read-only preview to avoid automated verification rewriting personal parameters:

```powershell
cargo run --release --bin ocean -- --config configs/experiments/reef-front-ridge.json --frames 600
# Compare the same configuration with the old front source; background multipliers are zero in both.
cargo run --release --bin ocean -- --config configs/experiments/reef-front-ridge.json --no-foam-ridges --frames 600
```

**Front Foam → Compression Ridge Experiment** is off by default; old configurations retain their original behavior. The experiment reads five-point velocities along the current flow direction in the same SWE stage and calculates three signed compression strengths per second. It retains only narrow bands above the speed threshold that are distinctly prominent and whose locally fitted peak remains within the neighborhood. It does not search along the tangent for two-dimensional isolated maxima, or use FFT/J, random texture, or foam density as emission locations. The neighborhood must be inside the domain and wet; the source retains thin-water suppression and edge fade. Enabling the experiment forces the background source off without rewriting background parameters.

**Narrow Band Width** is 0.05–10 meters; the local five-point fit limits effective width to one or two cells along the flow. This parameter cannot produce subcell thin lines, and changing resolution also changes the effective physical bandwidth. **Peak Relative Prominence** is 0–1, default 0.02, filtering flat compression regions. The old “Front Sampling Distance” applies only to the old source; velocity threshold and transition still control the strength of both sources. Parameters apply live without resetting the simulation.

**Enable Foam Emission** can independently stop emission; existing density / momentum continue to be transported, dragged, converged, and decayed. New quantities are still integrated according to generation rate and actual simulation timestep, without duplicate emission in the second-order prediction stage. No reverse velocity, viscosity, or cohesion has been added, and material, opacity, texture, default transport order, and density limit remain unchanged.

`--shore-view 7` displays the actual source term of the most recently completed substep (white corresponds to the configured full front generation rate), `8` displays emission weight, and `6` remains the transported density. In the old mode, one step must advance after selecting 7 / 8 before diagnostic data is available; pausing retains the last data. The experiment supports `--snapshot ... --foam-emission-window 8,9`, emitting only from 8 to 9 seconds; switching is based on warm-up frames, with a time resolution of approximately 1/60 second.

This is a source-selection experiment, not a validated reticulated foam model: flow direction is not necessarily the wavefront normal, stationary compression peaks continue to emit, and there is no wavefront tracking or temporal hysteresis yet. In the reef comparison, the generation region narrows and foam remains after emission stops, but this does not prove that a connected network forms naturally. See [configs/README.md](../../configs/README.md) for reproduction commands, fixed configuration, and measured limitations.

## GPU Spray Particles

The **Spray Particles** tab in the lower-left Dock controls breaking spray / droplets leaving the water surface, separately from surface foam. It is off by default and remains off when old configurations lack the fields.

```powershell
# Bounded preview without writing personal configuration; put spray options after the scene preset.
cargo run --release --bin ocean -- --beach --spray --no-settings --frames 600
# Show GPU emission strength (green low, red high), also enabling spray.
cargo run --release --bin ocean -- --beach --spray-debug --no-settings --frames 600
```

Use `cargo run` to restore personal parameters, without repeating `--beach`; adding `--spray` alone overrides only the toggle and does not reset saved emission parameters. The debug view shows source strength from the most recent actual simulation frame and retains it while paused; disabling debug restores particle rendering.

Emission signals are stored in a GPU storage buffer. Each completed SWE substep computes surface slope, positive rise, and effective water-depth gating, then integrates by cell area and actual simulation time; the second-order prediction stage does not generate again. Particle compute reads the signals and retains fractional remainders; the GPU updates the particle list and indirect draw arguments. Signals, particles, and counts are not read back at runtime, and the CPU does not assemble an emission list. Emission does not read surface foam density, and spray does not modify water, foam, or sand state in return.

| Parameter | Default and range | Application |
| --- | --- | --- |
| Emission Density | 150, 0–2000 particles / square meter / simulated second | Live effect on new particles; 0 stops generation |
| Slope Threshold | 0.03, 0–2 (dimensionless), fixed transition width 0.1 | Surface elevation gradient, not water-depth gradient |
| Rise Threshold | 0.02, 0–5 meters / simulated second, fixed transition width 0.2 | Only rising and steep water emits |
| Minimum Water Depth | 0.02, 0.001–1 meter | No emission below this value; reaches full weight at twice the depth |
| Upward / Forward Speed | 2 / 0.5, each 0–10 meters / simulated second | New particles also inherit horizontal water velocity and a small random perturbation |
| Maximum Lifetime | 1.2, 0.05–5 simulated seconds | Actual lifetime of new particles is 75%–100% of this |
| Size | 0.2, 0.01–2 meters | Base quad diameter; final diameter = size × random size multiplier; affects only new particles |
| Random Size Multiplier Minimum / Maximum | 0.65 / 1.35, each 0–2, dimensionless | Uniformly randomized by the GPU within the interval; minimum cannot exceed maximum, and equal values fix the multiplier. Affects new particles live and is saved without resetting; old configurations missing the fields retain the original random range. Size 2 meters with multiplier 0.5–1 gives an actual 1–2 meters; final diameter may exceed 2 meters |
| Air Resistance | 0.5, 0–10 / simulated second | Affects all particles live; 0 gives gravity-only ballistics |
| Soft Intersection Distance | 0.15, 0.001–1 meter | Affects only depth fading at scene intersections |

Parameters are saved with sessions and named configurations; live particles are not saved. Pause freezes motion, age, and emission; disabling releases spray resources. Resetting time, rebuilding the spectrum, or changing the nearshore domain clears spray (switching only offshore drawing topology / density does not); collider edits also clear old particles. Particles disappear on entering water, touching ground, exhausting their lifetime, or leaving the valid nearshore domain. The apparent height of surface foam does not change particle motion.

The particle pool holds at most 65,536 particles, retaining survivors first and filling free slots afterward; a full pool drops new requests without accumulating debt or overwriting live particles. Signals and remainders together use approximately 40 bytes per SWE cell, the double particle lists approximately 6 MiB, plus a small amount for parameters and textures. Device limits or allocation failure disable spray and display an error rather than switching to a CPU particle path.

The first version covers only pure SWE regions (FFT blend weight at most approximately 0.001) and does not emit offshore or in the FFT blend band. The texture uses CC0 `Splash.png` from the official Effekseer Pierre sample, split by the original Alpha into four random water sheets / droplets, with separate transparent borders and a full mip chain to prevent atlas bleeding and distant flicker; see [assets/spray/README.md](../../assets/spray/README.md) for the original image, author, license, and offline baking. Rendering uses premultiplied alpha, saved scene depth, and soft intersections; there is no transparent particle sorting, OIT, wind field, or separate mist layer. Heightfield collision does not represent exact collisions with arbitrary three-dimensional model surfaces and cannot generate real curling waves.

The full `--verify` includes emission gating, GPU signal layout for world coordinates / velocity, area / time integration, fractional emission, capacity saturation and reuse, pause / reset, water entry, and real particle depth occlusion and fade checks at two sizes. Existing water and foam states remain consistent with spray enabled in first / second order. Only the verification program is permitted to read back results at the end.

## Performance Panel

The **Performance** tab in the lower-left Dock sits alongside **Offshore Foam** and **Spray Particles**, displaying metrics and values in a two-column table; the GPU timing row formerly in the shallow-water panel has moved here. Data updates once per second. CPU metrics are per-frame averages within that window; GPU metrics include only newly read-back samples matching the current solver order.

| Metric | Measurement scope |
| --- | --- |
| Frame Rate, End-to-End Frame Interval | Wall-clock time between starts of adjacent draws, including vertical sync, GPU backpressure, and event-loop waiting; not input-to-screen latency and not subject to the simulation timestep's 0.1-second cap |
| CPU Frame Processing | From draw entry through submission and present return; includes UI, configuration handling, encoding, and blocking waits, and does not represent GPU completion |
| Acquire Frame | CPU wait for a swapchain image |
| UI CPU | egui layout and interaction, excluding UI draw encoding |
| Simulation CPU | CPU command encoding and parameter uploads for FFT, offshore foam, SWE, and spray sources / particles, not GPU execution time |
| Render Encoding CPU | Encoding scene and egui drawing commands |
| Submission and Presentation CPU | Command buffer completion, queue submission, and present return, without waiting for GPU completion or screen output |
| SWE GPU, SWE GPU / Substep | Real GPU timestamps; includes SWE, nearshore foam, and intermediate substep copies, plus all emission signal passes when spray is enabled; excludes FFT, offshore foam, particle update / draw, and initial and final copies |
| SWE Simulation Advance, Substeps / Frame, Solver Order, GPU Sample Count | Actual simulated duration and substep statistics corresponding to GPU samples; simulated duration is not compute time |

Time is measured in milliseconds; details appear in row-name hover tooltips. GPU timestamps are read asynchronously without blocking every frame for timing; corresponding states are shown for unsupported devices, read failures, pause / zero time speed / SWE disabled, and waiting for samples from a new solver. When switching order or entering idle, unpublished old GPU samples are discarded; the table still refreshes once per second.

## Per-Frame Data Flow

```text
CPU, once at initialization:
  A, L, wind direction, seed → Phillips spectrum → random complex coefficients h0(k)

GPU, every frame:
  h0(k), time t
      ↓ evolve.wgsl
  Height spectrum H(k,t) and horizontal displacement spectra Dx(k,t), Dz(k,t)
      ↓ ifft.wgsl, rows first, then columns
  Spatial height h(x,z), horizontal displacements dx(x,z), dz(x,z)
      ↓ foam.wgsl: Jacobian compression detection, foam generation and dissipation
  Accumulated foam coverage F and current Jacobian J
      ↓ render.wgsl
  Seabed mesh → linear scene color / depth prepass
  Mesh vertex displacement → displaced finite-difference normals → underwater optical path and analytic integration → Fresnel / sunlight / foam
  Sky, scene, water surface, and spray → complete Rgba16Float HDR frame
      ↓ post_process.wgsl
  Optional exposure / Bloom / tone mapping → SDR sRGB frame → GUI
```

### Spectrum and Time

`src/ocean/simulation/spectral/spectrum.rs` converts array indices into wave vectors `k = 2π * frequency / patch_length`. The array uses unshifted FFT order: `0,1,...,N/2-1,-N/2,...,-1`.

The Phillips spectrum controls energy at different wavelengths and directions; Gaussian random numbers provide complex initial coefficients. Initialization uses `sqrt(P/2) * (ξr+iξi)`, then multiplies by `N² * Δk`, where `Δk=2π/patch_length`: N² compensates for the normalized IFFT, and Δk comes from the square root of the two-dimensional frequency cell area. The old fixed RMS wave-height normalization is removed to avoid cancelling the effect of A. Both A and L genuinely change the ocean. Short-wave damping and the 0.07 weight for the upwind direction are retained. Zero frequency and Nyquist rows and columns are set to zero.

`evolve.wgsl` uses the deep-water gravity-wave dispersion relation `ω = sqrt(g * |k|)` to compute:

```text
H(k,t) = h0(k) * exp(iωt) + conjugate(h0(-k)) * exp(-iωt)
```

This guarantees `H(-k,t) = conjugate(H(k,t))`, so the imaginary component of the inverse transform should be close to zero. `exp(iωt)` represents rotation in the complex plane, corresponding to a wave phase change. After initialization, random noise is not generated every frame, and waveforms are not updated cumulatively from the previous frame.

Horizontal displacement uses `D(k,t) = -i * k/|k| * H(k,t)`. Under this implementation's inverse-transform sign convention, rendering uses a negative scaling coefficient λ = -1.15, bringing vertices together toward crests rather than troughs; press C to observe the difference.

### GPU Inverse FFT

`ifft.wgsl` uses radix-2 DIT butterflies: the first stage on each axis reads input using bit reversal, and subsequent stages expand the span from 2 to N. The inverse transform's twiddle factor uses a positive sign, and the final stage on each axis divides by N.

`simulation/spectral/mod.rs` alternates reads and writes between two storage buffers. Each stage submits a separate compute pass, with wgpu responsible for resource synchronization between stages. Each stage uses a separate immutable parameter buffer, preventing multiple writes to the same uniform before a single submission from making all stages read the last set of parameters.

A 128×128 grid has 7 stages per axis, or 14 FFT passes in total. Height, X/Z displacement, and the low-pass water level and X/Z momentum for nearshore input are transformed together in six channels of the same buffer. The total number of stages is even, so the final result returns to the initial output buffer and can be read directly by the vertex shader without downloading to the CPU every frame.

### Rendering

Mesh vertices are generated from `vertex_index`; the index buffer is created at initialization or Apply. Vertex positions are `(x + dx, h, z + dz)`, and normals are calculated from neighboring displaced points. Periodic sampling keeps height, horizontal displacement, and normals consistent at patch seams.

The sky uses procedural colors; the water includes Fresnel reflection, microfacet BRDF sun highlights, and water optics, without distance fog. White foam comes from the displacement Jacobian and a separate history field, with periodic procedural detail and distant detail filtering. It is a visual whitecap model, not an independent foam particle or ocean-current transport model.

## HDR and Bloom Post-Processing

Entry: **Shading → Water and Sky → HDR and Bloom → Enable HDR Post-Processing**. The master toggle is off by default; when enabled, separate **AgX Tone Mapping** and **Enable Bloom** toggles are available. Changes apply live without resetting FFT, SWE, foam, or camera, and follow the manual save rules. Old configurations lacking `optics.post_process` keep the master toggle off; when the new `tonemapping_enabled` / `bloom_enabled` fields are missing, both default to true while retaining existing exposure and bloom parameters. Old configurations that already enabled post-processing automatically use corrected AgX instead of the simplified ACES curve.

Scene lighting is calculated in linear space. Windows, snapshots, and offscreen profiling render sky, seabed, models, water, and spray together into `Rgba16Float`, retaining highlights greater than 1. Post-processing first applies exposure; when Bloom is enabled it extracts highlights, spreads them through up to six levels of downsampling / tent-filter upsampling, and composites in linear space. When AgX is enabled, it maps the result to display-linear RGB; finally the sRGB target encodes it once. Output remains **SDR**, not HDR10 / HDR display or wide-gamut output; PNG snapshots remain 8-bit.

AgX uses [Godot 4.6's AgX/allenwp approximation](https://github.com/godotengine/godot/blob/4.6/servers/rendering/renderer_rd/shaders/effects/tonemap.glsl): input / output color matrices combined with a piecewise lightness curve preserve 0.18 middle gray, compress highlights, and shift extremely bright colored light toward white without compensating for seawater or specific colors. Its general default contrast 1.25 and white point 16.29 are fixed; this is not the full Blender OCIO transform and provides no additional artistic grading. The implementation and MIT attribution are in [`agx.wgsl`](../../src/ocean/render/shaders/agx.wgsl), with no runtime external resources or LUT.

| Parameter | Meaning and range | Default |
| --- | --- | --- |
| AgX Tone Mapping | Toggle, independent of Bloom; disabling it does not compress highlights | On (subject to master toggle) |
| Exposure Compensation (stops) | -8–8; each additional stop doubles linear brightness before highlight extraction and mapping; still applies when both AgX / Bloom are off | 0 |
| Enable Bloom | Toggle; disabling hides and retains bloom parameters and skips bloom computation | On (subject to master toggle) |
| Bloom Strength | 0–2, dimensionless; 0 also skips bloom computation without changing exposure or the AgX toggle | 0.1 |
| Highlight Threshold (linear) | 0–20, extracted according to the maximum RGB component after exposure, not an sRGB value | 1 |
| Threshold Softness | 0–1, transition half-width as a proportion of the threshold; 0 is a hard threshold | 0.5 |
| Bloom Radius | 0.5–2, measured in texture pixels at each level, not meters; varies with image resolution | 1 |

Strong highlights in every scene can produce bloom; too low a threshold also causes white foam and large areas of sky to bloom. When the post-processing master toggle is off, the original linear color is output directly, without exposure, Bloom, or AgX. When enabled, exposure alone, Bloom alone in addition to exposure, AgX alone, or both can be used; with AgX off, values outside the SDR range are clipped, and enabling Bloom does not automatically enable mapping. AgX changes highlights and overall brightness but does not use global saturation compensation to hide curve color shifts; fix exposure and scene when comparing, and toggle only the relevant switch.

Water, nearshore, foam material, offshore foam, and spray source debug modes automatically bypass exposure, Bloom, and tone mapping to avoid changing numeric color scales. The GUI is drawn after post-processing and does not bloom. Window size changes rebuild HDR and bloom targets; normal frames do not synchronously read back the GPU. The `hdr_bloom_composite` GPU time in offscreen profiling includes post-processing (even when disabled, there is a final color output pass).

`--verify-optics` and the full `--verify` check actual GPU generic color references, grayscale monotonicity / neutrality, 0.18 middle gray, RGB / CMY extreme-brightness desaturation, -8 / 0 / +8 stop exposure, and single sRGB encoding; they also cover HDR highlight preservation, Bloom spread / threshold / energy, the two independent toggles, debug bypass, toggle history, and size rebuilding for odd / single-pixel / elongated targets. Physical optics verification still uses linear floating-point output without tone mapping.

## Sand Texture

The beach scene's shore sand and underwater seabed share the ambientCG **Ground061** 2K material (CC0). Assets and source information are in `assets/sand/Ground061/`, embedded at compile time without requiring access to the download directory.

The color map is decoded as sRGB; the OpenGL normal, roughness, and ambient occlusion (AO) maps are read as linear data. A complete mip chain is generated at initialization, using repeat, trilinear, and anisotropic filtering. Color maps for sand and imported models are JPEG-decoded only on the CPU and uploaded as the base level; subsequent mips are averaged level by level on the GPU in linear space and written back to sRGB. Data maps retain the existing CPU filtering. **Sand Texture Size (m)** in the beach parameter group controls the length covered by each texture tile along the sand slope and shoreline, ranging from 0.25–128 meters, default 4 meters. Larger values produce larger sand patterns and less repetition; try 16–32 meters first. Changes apply live without resetting the simulation and are saved with the current state and named configurations; old configurations missing the field still use 4 meters. This is a visual scale setting; the asset provides no measured dimensions. Normals change lighting only, not seabed geometry or the SWE bed.

**Sand Texture Rotation (deg)** in the same group ranges from -180–180 degrees, default 0 degrees. It rotates sand-slope plane coordinates around the world origin; a positive angle turns the texture's horizontal axis from downslope +X toward alongshore +Z. Color, normal, roughness, and AO rotate together, and normal lighting direction adjusts accordingly. It applies live without resetting and is saved with configurations; old configurations default to no rotation.

Wetness history still controls sand darkening and reduces material roughness; AO affects only ambient light. Residual foam and wetness / density debug views remain available, and non-beach scenes still use the optical inspection checkerboard. Current highlights remain a simplified lighting model rather than full PBR; displacement maps are not used.

## Verification and Export

```powershell
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings

# Run real GPU computation without a window and compare point by point with independent f64 direct Fourier summation.
cargo run --features ocean --bin ocean -- --verify

# Export an image using the same GPU computation and rendering pipelines; the destination must not exist.
cargo run --release --features ocean --bin ocean -- --snapshot ocean.png --time 3

# Open a real window and exit automatically after successfully presenting 120 frames, for startup checks.
cargo run --features ocean --bin ocean -- --frames 120
```

CPU tests check random-seed reproducibility, finite values, zero frequency, Nyquist handling, square-root amplitude scaling with A, a flat surface at A=0, mesh density not affecting the spectrum, and real output and time variation from the conjugate spectrum. `--verify` compares all real and imaginary components of height, two horizontal displacements, and three nearshore input channels at three times on 8×8 and 16×16 grids; it also checks zero mean and whether waveforms vary over time. Ordinary tests do not require a GPU; GPU verification is explicitly run via a separate command.

## Files

```text
src/bin/ocean.rs                         Parameters and entry point
src/ocean/camera.rs                      Camera and GPU layout shared by windowed and offscreen modes
src/ocean/app/{viewer,snapshot}.rs       Window scheduling and offscreen PNG, retaining separate scheduling
src/ocean/gpu/{context,timer}.rs         Device, readback, and asynchronous GPU timestamps
src/ocean/parameters/                    Parameter definitions, defaults, and validation
src/ocean/simulation/offshore/           Spectrum, FFT, offshore foam, and verification
src/ocean/simulation/nearshore/          SWE, nearshore foam, wetness history, and verification
src/ocean/simulation/spray/              Spray sources, particles, and verification
src/ocean/scene/                         Model geometry, terrain, and colliders
src/ocean/render/                        Scene rendering, water and sky optics, textures
src/ocean/ui/                            Dock, parameter panels, controls, and layout saving
src/ocean/settings/                      Current session, named configuration library, and saving
src/ocean/labs/foam_weight_lab/          Independent foam weight experiment
src/ocean/verify.rs                      Main real GPU verification entry point
```

Each domain's WGSL is in its owning module's `shaders/`; cross-module use references the same source file rather than copying it. `ocean/mod.rs` retains old public paths such as `ocean::shore` and `ocean::viewer`, and `gpu/mod.rs` retains `gpu::{Gpu, Ocean, Renderer, CameraUniform, Error}`; all are re-exports, not separate implementations.

The basic FFT generates a periodic ocean and does not itself solve shoreline collisions. Beach and scanned-model scenes overlay local two-dimensional SWE, foam transport, and wetness history; see the relevant sections. Coupling is FFT → SWE. Real curling waves and three-dimensional air entrainment are still not included; mesh folding caused by excessive horizontal displacement is not physical wave breaking.

Formula reference: [Jerry Tessendorf, Simulating Ocean Water, §3.2–3.4, 3.7](https://jtessen.people.clemson.edu/reports/papers_files/coursenotes2002.pdf). Graphics interfaces: [wgpu](https://docs.rs/wgpu/27.0.1/wgpu/), [winit ApplicationHandler](https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html).


The GUI uses [egui-wgpu 0.33.3](https://docs.rs/egui-wgpu/0.33.3/egui_wgpu/), paired with wgpu 27.
## Jacobian Foam: Research and Implementation

Uses **central differences after IFFT + compression-triggered generation + coverage with memory**.

References:
- [Crest's UpdateFoam.compute](https://github.com/wave-harmonic/crest/blob/master/crest/Assets/Crest/Crest/Shaders/Resources/UpdateFoam.compute): computes a determinant from horizontal displacement, generates foam in compressed regions, and decays its history; it also has separate flow transport.
- [Crest foam parameter documentation](https://crest.readthedocs.io/en/4.15.1/user/ocean-simulation.html#foam): generation intensity, coverage threshold, and dissipation rate are controlled separately.
- [Aalto ocean rendering paper, section 3.8](https://aaltodoc.aalto.fi/bitstreams/805df4ad-f08a-40cc-a9bd-3d25947da66d/download): presents the Tessendorf displacement Jacobian and frequency-domain derivative formulas.

### Why spatial differences

Multiplying by `ik` in the frequency domain and then applying IFFT yields derivatives, but requires additional derivative channels and FFT work. Dx and Dz are already available, so a single O(N²) compute pass can approximate the derivatives and is easy to verify independently. The difference step is `patch_length / FFT_resolution`, independent of mesh density; changing FFT resolution changes the resolved short waves and derivative accuracy, so visual thresholds may need readjustment.

Let the actual horizontal displacement be `d = lambda * gain * D`. Lambda uses the negative sign from rendering (default -1.15); do not take its absolute value or omit the wave height gain.

```text
J = (1 + ∂dx/∂x) * (1 + ∂dz/∂z) - (∂dx/∂z) * (∂dz/∂x)
```

All derivatives use central differences at periodic boundaries. Compute the two cross derivatives separately to avoid incorrectly writing quantities that are not necessarily equal under discrete differencing as a single square.

J=1 means horizontal area is unchanged; 0<J<1 means compression; J near 0 means the local horizontal mapping is nearly singular; J<0 means local orientation is reversed. J is the signed area ratio of the horizontal mapping, not the three-dimensional water surface area or a strict wave-breaking criterion. Using `threshold > 0` allows white foam to appear before geometric folding.

### History update

Coverage F is between 0 and 1, defined as:

```text
s = generation * clamp(threshold - J, 0, 1)
dF/dt = s*(1-F) - decay*F
r = s + decay
F_new = s/r + (F_old - s/r) * exp(-r*dt)
```

When r=0 or dt=0, the previous value is retained. The equation above is the bounded source model chosen for this project, integrated exactly for J frozen during each step; for a fixed source, results agree at 30/60/120 FPS. Moving waveforms still have temporal sampling error; this does not claim that the entire dynamic result is identical at arbitrary frame rates.

History is stored in periodic parameter coordinates on the undisplaced surface and moves with the mesh; it is not world-space ocean-current transport. Each thread reads and writes only its own F, so ping-pong is unnecessary. Adding neighborhood transport or diffusion requires double buffering. Each FFT pixel stores `(F,J)`, adding `8*N²` bytes of memory (128 KiB at 128², 2 MiB at 512²) and one extra compute pass per frame.

### Panel and lifecycle

The independent **Jacobian foam** panel appears directly to the right of the ocean parameter panel. All foam controls apply immediately without Apply.

- **Chop strength**: actual horizontal displacement strength; also affects J.
- **Show foam / F key**: controls display only; history continues to evolve.
- **J threshold**: default 0.65; higher values make foam easier to generate. The maximum is 1, so a flat surface does not generate foam on its own.
- **Generation / s**: default 4; increases generation speed. Set to 0 to observe old foam decay independently.
- **Decay / s**: default 0.6; higher values make foam disappear faster. Set to 0 to retain history.
- **Foam view**: normal shading, Jacobian (red: negative, orange: compression, white: 1, blue: stretching), Coverage (black-and-white F).
- **Clear foam history**: clears history, including while paused.

When paused or Time speed=0, foam neither grows nor decays; resetting time, resetting with R, and rebuilding with Apply clear history. After disabling choppiness or setting gain to 0, J=1 and no new foam is generated, while existing foam continues to decay. Fog-free rendering remains unchanged.

### Verification and export

`cargo test --all-features` includes tests for a flat surface with J=1, sourceless decay, pause, clearing, cross derivatives, zero displacement scale, and frame-rate consistency with a fixed source. `--verify` additionally runs the real GPU foam shader and compares F and J point by point against an f64 CPU reference, covering periodic seams and history updates.

```powershell
cargo run --features ocean --bin ocean -- --snapshot foam.png --time 3
cargo run --features ocean --bin ocean -- --snapshot jacobian.png --time 3 --foam-view 1
cargo run --features ocean --bin ocean -- --snapshot coverage.png --time 3 --foam-view 2
```

Offscreen export prewarms history at 60 Hz for up to 8 seconds before the target time (long-time exports use a finite warm-up approximation); initial coverage is zero at t=0. The output log includes minimum J, maximum F, and the proportion of grid points with F>0.02, making it easier to distinguish foam from reflection highlights.

## Underwater Ground and Analytic Water-Medium Integration

A real seabed plane mesh (two triangles) has been added, by default 6 meters below mean sea level. A checkerboard helps observe changes with distance; RGB color bands can be enabled to observe channel attenuation. The seabed uses simplified lighting. The scene is first drawn to an `Rgba16Float` linear color texture and a sampleable `Depth32Float` texture; the main pass then draws the sky, seabed, and water surface. The seabed has geometry and shading only and does not alter the FFT wave simulation.

The water surface uses seabed depth at the same screen pixel, reconstructs the world position with the inverse view-projection matrix, and computes `d=length(Pbottom-Psurface)`. Thus the water path is a straight-line distance in meters, not camera-to-surface distance, vertical water depth, or a raw depth-buffer difference. WebGPU NDC depth is reconstructed over 0–1, with texture Y and NDC Y directions converted. Refraction is disabled by default; when **Seabed refraction** is enabled, a flat seabed or sand slope instead uses the wave-surface normal to calculate the refracted direction and analytic seabed intersection. A valid screen-scene color hit uses the refracted path length; otherwise the straight-line path above is retained. See “Seabed caustics and refraction” below.

```text
T_rgb = exp(-sigma_t_rgb * d)
background_rgb = Lbg_rgb * T_rgb
manual_rgb = S_manual_rgb * (1 - T_rgb) / sigma_t_rgb
scattering_rgb = manual_rgb + integrated_sky_rgb
medium_rgb = background_rgb + scattering_rgb
water_rgb = (1-Fresnel)*medium_rgb + Fresnel*reflection_rgb
```

Sun highlights use a dielectric Cook–Torrance BRDF. Isotropic GGX describes unresolved small variations in the wave-surface normal, using a fixed dimensionless perceptual roughness of 0.28; a height-correlated Smith term computes microfacet masking and shadowing for light and view directions. Environment reflection and direct highlights share Schlick Fresnel based on the water refractive index, including when refraction mode is off; at refractive index 1 both interface reflection and highlights are zero. Sky and SSR are sampled in the specular direction, the sky halo contributes to environment reflection, and the solar disk's contribution to the water surface goes through the BRDF. The underwater background continues to use the path-length model below.

Sun highlights are added to the combined medium and environment reflection, followed by foam blending. Calculations are in linear RGB, then encoded by the sRGB output target. Do not gamma-encode inputs to the exponential calculation; sRGB display values in debug images are not linear values either. The path view maps distances in meters to grayscale.

sigma_t is the extinction coefficient (absorption + out-scattering), in 1/m; S is in-scattered radiance added per meter along the viewing direction, not sigma_s. Manual S is a directly adjustable approximate source term constant along the path; the added skylight source is solved from the sky, scattering albedo, and depth, as described below. Solar in-scattering is not yet connected. Seabed Lbg uses simple ambient and directional lighting and does not include attenuation along the underwater incident path from the sun to the seabed. This model is suitable for observing individual formula terms, not a complete multiple-scattering solution.

`optics.wgsl` uses a Taylor expansion at small optical thickness tau=sigma_t*d to avoid division by zero and cancellation error in `1-exp(-tau)`. At sigma_t=0 it uses the exact weight d, and output approaches `Lbg+S*d`; at d=0 it is Lbg. Each RGB channel is handled independently. Scene color and sampled depth are separate from the main pass write targets to avoid texture feedback; resize rebuilds the scene targets, main depth, and associated bindings together.

### Water optics panel

Controls apply immediately without Apply, and do not rebuild the wave spectrum or clear foam history.

| Control | Meaning |
| --- | --- |
| Seabed depth (m) | Seabed depth relative to mean water level, default 6 meters |
| Extinction sigma_t R/G/B | RGB extinction coefficients, default (0.18, 0.06, 0.025) 1/m |
| Manual source S (color + intensity) | Constant RGB in-scattering source, default (0.0025, 0.009, 0.006) radiance/meter |
| Water view | Debug outputs listed in the table below |
| Path view white at (m) | Distance corresponding to white in the path-length grayscale view, default 50 meters |
| Sky reflection and highlights | Show/hide sky reflection and highlights to inspect the seabed independently |
| Screen-space reflection (SSR) | Enabled in the independent SSR Tab under Shading. Off by default; applies live. Requires Sky reflection and highlights |
| Reflection distance (m) | Maximum SSR tracing distance, range 1–500 meters, default 100 meters |
| SSR intensity | Blend weight between valid hits and sky reflection, 0–1, default 1. At 0, SSR tracing stops |
| SSR trace steps | Maximum Hi-Z tiles visited per ray, 16–256, default 128. Lower values can limit cost but may miss reflections |
| SSR hit tolerance | Tolerance multiplier relative to pixel footprint, 0.25–4, default 1. Too small may miss hits; too large may cause false hits |
| SSR edge fade (pixels) | Fade width to sky near screen edges, 0–128 render pixels, default 4. 0 disables fading |
| Seabed RGB test bands | Show R/G/B test bands on the checkerboard |
| Checker size (m) | World size of each checker |
| Reset optics | Restore the defaults above |

SSR searches along the wave-surface reflection direction for camera-visible opaque objects, such as reefs and sand slopes above the water. Misses use sky reflection, fading near screen edges or the distance limit. When SSR and physical model refraction are both enabled, they share a scene depth pyramid. Rock faces outside the screen or hidden behind other objects cannot be recovered from camera depth.

The SSR Tab provides a “Normal / SSR only” segmented switch. “SSR only” displays scene colors from valid hits on the water surface, retaining SSR intensity and fade weights; misses are black. It skips water-surface sky reflection, Fresnel blending, medium, and foam composition; the opaque scene remains visible.

SSR parameters apply live; old configurations missing fields use the defaults above. Trace steps and hit tolerance control SSR only. When intensity is 0 and physical refraction does not need the pyramid, the screen depth pyramid stops updating.

| Water view / CLI number | Output |
| --- | --- |
| Shaded water / 0 | Complete water-surface composition |
| Water path (m) / 1 | clamp(d/path_range,0,1) grayscale |
| Transmittance T / 2 | RGB transmittance T |
| Attenuated seabed (absorption) / 3 | Lbg*T, showing seabed color after absorption/extinction |
| In-scattering / 4 | Sum of accumulated in-scattering from manual S and skylight |
| Extinction loss / 5 | Lbg*(1-T), color removed by extinction, including out-scattering |
| Raw seabed / 6 | Lbg without water-medium integration |
| Medium sum / 7 | background+scattering, without surface reflection and foam |

Water-medium debug views take precedence over foam debug and bypass reflection, highlights, and foam. First inspect the ground with Raw seabed, then compare Attenuated seabed and In-scattering to observe the two additive terms directly.

### Running, export, and verification

```powershell
cargo run --release --features ocean --bin ocean
cargo run --release --features ocean --bin ocean -- --seabed-depth 12 --sigma-t 0.2,0.08,0.03 --source 0.003,0.008,0.005
cargo run --release --features ocean --bin ocean -- --snapshot water-path.png --water-view 1 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot transmittance.png --water-view 2 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot attenuated-bottom.png --water-view 3 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot in-scattering.png --water-view 4 --time 3
cargo run --features ocean --bin ocean -- --verify
```

The ground covers the current 3×3 ocean patch area. When scene depth has no seabed hit (for example, beyond the ground or clipping range in the distance), a black background and 1000-meter path provide a finite deep-water approximation; it appears white in the path view. With nonzero S and sigma_t=0, the constant source accumulates linearly with path length. When radiance from large parameters exceeds 1, an SDR display target clips it if HDR post-processing or AgX mapping is disabled, or in a debug view; normal shading with both post-processing and AgX enabled instead compresses it with tone mapping. This does not mean the integration result is bounded by 1. If the ground is very shallow, wave troughs may fall below it and depth testing will make the ground occlude the water surface; horizontal seabed mode does not simulate wet/dry boundaries; beach mode uses the local SWE described below.

CPU tests cover zero distance, zero extinction, pure attenuation, RGB independence, source scaling, and comparison with numerical integration. `--verify` runs the shared production WGSL integration function and checks every RGB component across 40 parameter sets; it also uses the real seabed pre-render + water rendering pipelines at two sizes (including resize), three depths, seven debug views, and three pixel positions, comparing path length/color point by point with independently computed analytic plane intersections.

Code: `src/ocean/parameters/optics.rs` (parameters), `render/optics.rs` (uniform and reference calculations), `render/shaders/optics.wgsl` (shared integration), `render/seabed.rs` / `render/shaders/seabed.wgsl` (seabed pre-render), `render/verification/optics.rs` (raster pipeline verification). Subsequent abbreviated paths are relative to `src/ocean/`.


## Skylight Depth Lookup Table and Piecewise Analytic Integration

### Beach observation scene

Add `--beach` at startup, or click **Beach scene** in the Shallow Water Simulation (SWE) panel, to load a sand slope, water optics parameters, and a camera preset looking obliquely down toward the shoreline. The preset uses smaller wave amplitude, zero manual S, and enabled skylight, making it easy to compare the transition from shallow water with visible sand to dark deep water.

The sand slope is an actual geometric plane `y = -beach_slope * (x - shore_x)`, with land at negative X and deep water at positive X. The panel's **Beach slope (height / width)** controls the slope, **Shoreline X (m)** moves the mean-water-level shoreline, and **Beach camera** restores the viewing angle; disable **Sloped beach** to return to the horizontal checkerboard seabed. When the slope is enabled, the flat seabed depth control is disabled and actual water depth is determined by the slope. These settings support manual saving.

Sand uses dry/wet colors and antialiased fine lines, with lighting normals tilted along the slope. Depth pre-rendering and world-coordinate reconstruction compute the actual oblique path length; water color is not manually graded by distance from shore. Switch to Water path, Transmittance, or Attenuated seabed to inspect it. The landward sand slope occludes the ocean through depth testing. `--beach --no-shore` returns to an observation mode with only FFT and the sand slope; the default beach enables the local 2D SWE described below.

```powershell
cargo run --release --features ocean --bin ocean -- --beach
```

`--beach` reapplies the preset at every startup; omit it next time to continue from the adjusted state. `--snapshot beach.png --beach` exports the same view offscreen.

The **Sky scattering** section of Water optics applies immediately without Apply:

| Control | Meaning |
| --- | --- |
| Enable sky scattering | Enable the underwater skylight source; does not affect the sky background or surface reflection switch |
| Sky tint / Sky intensity | egui native linear RGB color picker and 0–20 intensity, shared by sky background, reflection, and underwater skylight; white tint retains the original sky gradient |
| Scattering albedo RGB | Per-channel α=σ_s/σ_t, range 0–1; σ_s=α σ_t, σ_a=(1-α) σ_t |
| Precise sky RGB | Direct linear RGB input for color and albedo |
| Sky integration steps | 8 / 16 / 32 segments, default 16; higher settings improve integration accuracy |
| Sky source vs depth | Show RGB local source curves from surface to seabed, vertical axis in radiance/m, automatically scaled |
| Manual source S | The original constant manual source remains available and can be set to zero to observe skylight alone |

Skylight uses the procedural sky background without the solar disk and halo; the bright sun spot still appears in the sky/reflection. An isotropic phase function `p=1/(4π)` is integrated over the upper hemisphere by solid angle; it is not multiplied by surface-lighting N·L. For a uniform sky without attenuation, the hemisphere factor is 1/2, not 1.

```text
z = max(-world.y, 0)                 # Mean sea surface y=0
mu_i = (i + 0.5) / 32               # The current sky is axisymmetric, with 32 equal-solid-angle samples
J_c(tau) = sum_i Lsky_c(mu_i) * exp(-tau / mu_i) / 64
Ssky_c(z) = sigma_t_c * albedo_c * J_c(sigma_t_c * z)
S_total = S_manual + S_sky
```

The GPU stores J in a **256×1 Rgba32Float** lookup table (4 KiB). Its horizontal axis uses optical depth tau rather than a fixed distance in meters, so RGB channels with different extinction coefficients can share one table. Samples `tau=32*(i/255)^2` are denser near the surface, and reads interpolate linearly in tau. Skylight at tau>=32 is treated as zero; its transmittance upper bound is less than 1.3e-14. The table updates only when sky color or intensity changes; changing σ_t, albedo, or seabed depth requires no rebuild, and resize rebinds the original table.

Skylight is accumulated along the view ray from the surface to the seabed. Each segment evaluates Ssky at its midpoint, uses the original analytic weight `(1-exp(-sigma_t*ds))/sigma_t` within the segment, and multiplies by transmittance from that segment to the surface entry point. Manual S still uses a separate analytic solution for the entire ray.

Each channel independently uses segments spaced quadratically along the path, and truncates the skylight integration tail at 16 view optical depths (the upper bound on omitted contribution is proportional to exp(-16)), preventing a long path from making the first segment span the entire shallow scattering region. This truncation affects only skylight, not manual S, seabed transmittance, or path-length debug values. When σ_t=0 or α=0, skylight scattering for that channel is exactly zero; manual S still follows the original `S*d` limit.

### Additional debug views

| Water view / CLI number | Output |
| --- | --- |
| Sky source at ray midpoint / 8 | Local Ssky at the midpoint of the underwater view ray (radiance/m) |
| Integrated sky scattering / 9 | Accumulated skylight color accounting for view-path attenuation |
| Integrated manual source / 10 | Accumulated in-scattering from the original manual S |
| Total source at ray midpoint / 11 | S_manual+S_sky at the midpoint (radiance/m) |

4 is accumulated skylight + accumulated manual source; 7 also adds the attenuated seabed. All debug views bypass reflection, highlights, and foam; 8 and 11 are local source terms and cannot be directly compared with accumulated colors. Local sources are usually dark; values can be read from the depth curve.

```powershell
cargo run --release --features ocean --bin ocean -- --skylight
cargo run --release --features ocean --bin ocean -- --sky-color 0.8,0.9,1 --sky-intensity 2 --scattering-albedo 0.3,0.5,0.65 --sky-steps 32
cargo run --release --features ocean --bin ocean -- --snapshot sky-scattering.png --source 0,0,0 --water-view 9
cargo run --release --features ocean --bin ocean -- --snapshot manual-scattering.png --water-view 10 --no-skylight
```

Limitations: this version uses single, isotropic scattering. Incident skylight depth is computed from the mean plane y=0, and crests above mean sea level use z=0. This sky-scattering integration does not calculate refraction into water, entry Fresnel loss, wave-surface occlusion, solar volume scattering, or multiple scattering. The separate seabed caustics module calculates sunlight refraction into water and entry loss; caustics are not added again to the skylight source. The seabed itself retains the original simplified lighting. Sky color controls only the sky background and does not change the independent sun spot color.

`--verify` compares the real GPU LUT term by term with an f64 angular sum, then checks production WGSL against an independent per-direction closed-form line integral. It covers zero/very small/large extinction, different RGB values, disabled skylight, zero albedo, intensity/color changes, 0–1000 meter paths, different depths and viewing directions, and convergence at 8/16/32 segments; raster checks include resize, LUT parameter updates, and local/accumulated/composite debug views.

Code: `sky.wgsl` shares the sky background, `skylight_lut.wgsl` generates the lookup table, `skylight.wgsl` performs lookup and piecewise integration, `skylight.rs` manages parameters and GPU resources, and `skylight_verify.rs` provides independent verification.

## Local Manual Saving and Color Pickers

All ocean parameters, unapplied spectrum drafts, foam parameters, water parameters (including skylight), camera, pause, and playback speed are saved preferentially to an existing `configs/ocean-settings.json` in the current working directory. This file is tracked by Git and contains the existing current session and complete named configuration library; running `cargo run` from the repository root restores it, and subsequent manual saves also become Git differences. If this file does not exist in another working directory, the fallback is still `%LOCALAPPDATA%\water-simulation\ocean-settings.json` (then `%APPDATA%`). Personal files at the original location are not migrated, deleted, or overwritten. Only clicking **Save** at the top or pressing **Ctrl+S** writes the current working state and configuration library; stopping editing, losing focus, loading a configuration, and exiting do not save automatically, and unsaved changes are lost on exit. The header always shows the unsaved state and **Save succeeded / Save failed**; hover over a failure for details and the path. There is no automatic retry; save again after fixing the problem. Dock layouts are still saved automatically and independently, unaffected by this rule. Time and foam history are not saved.

The **Configurations** workspace on the left uses a single-selection list; clicking a configuration loads it but does not write to disk. Below it are only **New Configuration / Delete Configuration** and **Save Configuration**: New Configuration expands a name input and **Confirm / Cancel**; confirming retains a snapshot of current parameters in memory, then the top Save or Ctrl+S writes it to disk. Delete Configuration expands a deletion prompt for the current configuration and **Confirm / Cancel**; confirming deletion switches back to the default configuration. **Save Configuration** overwrites the selected named configuration with current parameters and writes immediately. Right-click a configuration button to **Rename**, also using a name input and **Confirm / Cancel**. Configuration names cannot be empty, duplicated, or use the reserved name “Default Configuration”. Confirming creation, deletion, or rename does not implicitly write to disk; use the top Save or Ctrl+S afterward. The top Save records only the current working state and configuration library; it does not overwrite the selected named snapshot with current parameters. Named snapshots update only on creation or **Save Configuration**. Restart restores the last working state and configuration library written to disk. The panel displays an error message only on failure; operation instructions are in button hover tooltips.

On first upgrade, the existing local configuration is preserved as the read-only **Default Configuration**; if no configuration exists, the program's initial defaults are used. The default configuration cannot be renamed, deleted, or overwritten. Clicking **Default Configuration** restores all parameters; clicking the current configuration again also reloads it. Loading any configuration restores its applied spectrum and unapplied draft, and resets simulation time, foam, and shallow-water history; the draft still requires **Apply changes** to take effect. Under `--no-settings`, creation, deletion, saving, and renaming are disabled, but the initial configuration for this launch can still be restored.

Use `--settings PATH` to specify a writable session; `--no-settings` disables implicit session reading/saving. `--config PATH` strictly loads the specified file's top-level current session read-only, without automatic saving, fallback to defaults, or writing a corrupt-file backup; it cannot be combined with `--settings`. Explicit CLI arguments override corresponding saved values; other fields are still restored. `--snapshot` does not read personal configuration by default, but supports explicit `--config` and does not support writable `--settings`; `--time` is the snapshot simulation time, unaffected by the configuration's pause/playback multiplier. `--verify` never reads configuration. See [configs/README.md](../../configs/README.md) for repository configurations and reef experiment startup methods.

Each startup restores the last manually saved configuration selection and working state (which can include adjustments not overwritten into a named snapshot). Selection, creation, rename, and deletion affect the next startup only after being written to disk; after saving the deletion of the current configuration, the default configuration is selected at the next startup. If an old file has no recorded selection or the selection is invalid, the default configuration is selected while the existing working state and named configurations are retained. To restore personal parameters, run `cargo run` directly without additional scene override arguments such as `--beach`.

Old configurations without a sky section default to skylight off to preserve their original appearance; enable Sky scattering or add `--skylight`. New configurations enable skylight by default. Settings are written using a temporary file and atomic replacement; corrupt or unsupported files retain an `.invalid-timestamp.json` backup before being saved again. Write failures remain visible as an error state at the top until a manual retry, and are not silently reported as successful.

σ_t and manual S use egui native color pickers with Strength; the actual linear coefficient is `color * strength`, and precise RGB inputs can be expanded. Color represents channel proportions, while intensity is not limited by the color picker's 0–1 range. Sky color and albedo use 0–1 linear RGB directly, with a separate sky intensity.

## Local 2D shallow water, FFT offshore and waterborne foam

```powershell
cargo run --release --features ocean --bin ocean -- --beach --no-settings
cargo run --release --features ocean --bin ocean -- --beach --snapshot shore.png --time 24
cargo run --release --features ocean --bin ocean -- --beach --no-settings --shore-view 6 --snapshot foam-density.png --time 27
cargo run --release --features ocean --bin ocean -- --beach --no-shore
```

The independent **Shallow Water Simulation (SWE)** panel contains the SWE, foam, wet-sand and beach controls and opens by default. FFT Phillips power, wind direction and spectral length now drive both the ocean and the SWE inlet. Wave height gain scales inlet elevation and both momentum components together. The independent shore amplitude/period controls have been removed; old saved fields are ignored. The retired `--shore-amplitude` / `--shore-period` flags report a migration message. The entire SWE patch starts with FFT elevation and momentum; repeated run-up and foam still need time to develop. CLI overrides belong after `--beach`, which installs the beach preset.

| Shallow water debug view | Meaning |
| --- | --- |
| 0 | Shaded |
| 1 | Geometric water depth; white at 3 m |
| 2 | Cross-shore velocity: blue incoming, orange outgoing |
| 3 | Waterborne foam concentration |
| 4 | Stationary sand wetness |
| 6 | Waterborne foam density: green (low) to red (reference or higher), no texture |

### Solver and rendering

`shore_compute.wgsl` advances a rectangular, world-space finite-volume grid (default 256²). It offers the original first-order hydrostatic Rusanov + Euler solver and a second-order MUSCL + SSP-RK2 solver. The latter reconstructs free surface, velocity and bed using a generalized minmod limiter (theta=1.3), with hydrostatic face corrections and a matching centered bed source. Reconstruction falls back to first order near dry cells and patch boundaries. Both retain wet/dry handling and the 12 m/s velocity safeguard; RK2 stages include boundary relaxation and bottom friction. Foam generation/decay and ground memory update once per completed substep and remain split first-order source updates. This is not a port of the reference project's KP07 solver.

By default (dual-population experiment disabled), the stored variables are `(h, hu, hv, surface_foam_density)`. Foam is an effective amount per unit surface area in arbitrary units, not a depth-weighted concentration or physical mass density. A separate conservative upwind flux transports foam using shared face velocities: convergence concentrates it and divergence dilutes it, without creating foam. Both orders use first-order spatial foam transport; second-order SWE uses RK2 for its transport time integration. Foam is not divided by water depth or clipped at 1. The offshore sponge removes foam; cells at or below 3 mm water depth discard their remaining foam without leaving ground residue. Closed boundaries have zero foam flux.

Generation combines the original rising-water source (scaled by the background multiplier, default 0.15) with a directional front source: sample free-surface elevation (bed height plus water depth) ahead of the flow and emit where the surface falls. The front source does not require rising water or nearby dry land. Both retain wet-depth and offshore-sponge gating. This is a visual proxy, not resolved air entrainment; the criterion follows surface slope, so retreat can also emit when its forward water surface is lower; dry samples are excluded. Generation and exponential decay are integrated analytically with a frozen source per completed substep. Density may exceed the color reference, retaining accumulated amount rather than clipping it.

The patch spans at least 64 m across the beach and 384 m along it for a 128 m FFT tile. Its inland coverage expands with a conservative spectral height bound and wave height gain, up to 64 m inland. Gentle slopes can also expand the offshore region. Alongshore spacing is 1.5 m at the default tile size. The bed remains the existing straight sand slope, shared by simulation and rendering. Landward and alongshore boundaries reflect; the offshore edge samples FFT elevation and matching depth-integrated momentum in a relaxation zone occupying the outer 20% of the patch. There is no arbitrary obstacle editor or terrain SDF in this version.

**Shallow Water → SWE and Input → Solving and Sampling → CFL Limit (C)** controls the dimensionless conservative timestep limit, with range 0.05–1.1 and default 0.2. Changes apply live without resetting water, foam, sand, or material-coordinate history; missing `optics.shore.cfl` fields in old configurations retain 0.2. Values above 0.2 are experimental: the default water/foam stability and positivity assumptions are not guaranteed at larger limits. The Performance panel reports actual sampled SWE substeps per frame and simulation advance.

Substeps use a conservative speed/depth estimate. With cell widths `dx` and `dz` in meters, gravity `g = 9.81 m/s²`, and estimated maximum water depth `depth_bound` in meters, the rectangular-grid bound is `dt <= C / ((12 + sqrt(g * depth_bound)) * (1/dx + 1/dz))`, where `dt` is in simulated seconds and 12 m/s is the velocity safeguard. The configured C is not a measured local Courant number. The solver rounds the required substep count up to an integer, so actual steps can be smaller than the bound and different C settings can produce the same timestep. A frame advances at most 0.1 simulated seconds and 128 substeps; excess time from a long stall is discarded. This estimate is intended for the built-in terrain and wave controls, not arbitrary externally injected states. Boundary forcing, thin-film removal and velocity safeguards mean the entire open scene is not a closed conservative system.

The water mesh reads the SWE free surface and fades to the existing FFT displacement between the configured blend depth and 2.5 times that depth. Normals come from the combined surface. The original three FFT render channels remain unchanged; three additional channels drive the SWE inlet. The coupling is **one-way FFT → SWE**: shore reflections and currents do not modify the ocean spectrum. Changing mesh density affects how much of the simulated front is visible. Water thinner than 8 mm is discarded by the water material.

Each substep writes a separate output buffer and copies it to the previous-state buffer; water and ground use a fixed output binding. Parameter copies are ordered in the command encoder so substeps cannot accidentally all read the final uniform. The three state buffers (current, previous and RK predictor) use 15 MiB total at 256², scaling with the square of the selected resolution. There is no per-frame CPU readback. The copied lace image has a CPU-built mip chain in a read-only GPU buffer (about 5.3 MiB); shader sampling performs bilinear/trilinear filtering.

### FFT → SWE boundary coupling

For each wave vector, the existing ocean evolution has two signed branches:

```text
A = h0(k) exp(+i omega t)                travels along -k
B = conjugate(h0(-k)) exp(-i omega t)    travels along +k
H = A + B
Q = (omega / |k|²) k (B - A)
omega = sqrt(g |k|)
```

`Q` is depth-integrated horizontal momentum in m²/s, derived from linear continuity. It is not the FFT choppiness displacement and not the time derivative of that displacement. Both signed branches are retained, so the boundary carries the prescribed external FFT field, including its directional distribution; this is a relaxation boundary, not a characteristic decomposition that injects only incoming waves.

The three boundary spectra `(H, Qx, Qz)` receive the same configurable smooth low-pass. In **Shallow Water Simulation (SWE) → FFT input filter**, **Filter short waves** enables it; **Reject wavelength <= (m)** sets the rejected wavelength limit, and **Transition width (m)** sets the smooth transition above that limit. The default is a fixed 12 m cutoff plus a 12 m transition: wavelengths <= 12 m are rejected and >= 24 m pass fully. Unlike the earlier automatic rule, these values do not change with FFT resolution. A zero transition gives a hard cutoff. Disabling the filter passes all FFT modes. The **3–6 m** preset admits more short waves for comparison, but the first-order SWE grid can still dissipate unresolved waves and does not recover their dispersion.

Filter changes apply immediately and reset SWE water/foam/sand history, while FFT time keeps advancing. The patch is reseeded from the current filtered FFT, rather than restarted as still water. The settings persist with the other controls. No additional short-wave normal detail is added. CLI equivalents (put overrides after `--beach`):

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-filter-cutoff 3 --swe-filter-transition 3
cargo run --release --features ocean --bin ocean -- --beach --no-swe-filter
```

The six spectra share the existing IFFT stages. SWE samples channels 3–5 at `(world / FFT_tile_length + 0.5) * FFT_resolution`, with periodic wrapping and bilinear interpolation. This matches the ocean's undeformed world-coordinate convention. It interpolates the previous and current FFT boundary frames at each SWE substep; no FFT or momentum readback to the CPU is required.

On reset, the entire patch samples the current filtered FFT elevation and momentum. The six-second startup ramp has been removed. Wave height gain multiplied by the SWE-specific input gain applies to elevation and momentum equally. For extreme inputs, a common safety scale limits elevation to half the local still-water depth and momentum magnitude to eight times that depth. The scale preserves their ratio, although those clipped cases no longer reproduce the raw FFT amplitude.

The relaxed target is `(max(eta-Bed, 0), Qx, Qz, 0)`, blended with weight `1-exp(-4*edge_weight*dt)`. Foam is generated inside the SWE region rather than imported from FFT. The existing Jacobian foam remains on the far ocean.

The boundary momentum matches the **linear deep-water FFT**. SWE remains a non-dispersive shallow-water model, so this coupling does not eliminate phase-speed differences or reflection for intermediate/deep-water components. Short-wave filtering, the relaxation zone and the visual transition reduce the mismatch but do not constitute a finite-depth dispersion solver.

Adding three channels doubles the FFT field/scratch storage and butterfly work relative to the original three-channel ocean. At 128² it adds 768 KiB across the two FFT buffers, plus 384 KiB for the previous boundary frame. Normal rendering still performs no CPU readback.

### Ground memory and foam appearance

Sand wetness is stationary and dries with the configurable time constant, default 18 seconds. Waterborne foam moves with the conservative flux and is removed when water depth falls to 3 mm or below. No foam is stored or rendered on exposed ground.

The water material uses two offset flow phases driven by the simulated velocity and coarse/fine Voronoi lace samples. Concentration changes an erosion threshold, opening holes as foam disappears. Mip filtering and derivative-smoothed thresholds reduce distant shimmer. F / Show foam affects waterborne foam without stopping its evolution.

The bundled `assets/shorewaves/voronoi_lace.png` comes from Reboot16 / Enrico K.'s Shorewaves project. Its MIT notice is retained in `assets/shorewaves/LICENSE.txt`. The solver and material implementation here are adapted to Rust/wgpu/WGSL and the existing FFT renderer; no Godot runtime is needed.

### Lifecycle and validation

Pause freezes simulation and texture flow. R, Restart time and **Reset shallow water, foam and sand** reinitialize the local simulation from the current FFT and clear foam/sand history. Terrain, wave height gain, SWE input gain, blend-depth and enable changes also reset it; applying a new FFT spectrum rebuilds the coupled resources; foam-rate, drying-time and view changes preserve history. Settings persist, but simulation state does not. Snapshot mode prewarms up to 60 seconds of shore history (8 seconds offshore); later snapshots approximate a finite history window.

`--verify` exercises the actual GPU shader: lake at rest, local pulse mass conservation away from the inlet, propagation and foam transport in both axes, dry-cell foam removal and sand drying/rewetting, extreme slope/amplitude settings, terrain reset, run-up, finite/nonnegative states, pause, reset and disable. Raster verification prewarms 24 seconds and checks water history at two target sizes. The FFT test compares all six channels to a double-precision direct DFT and checks analytic oblique waves in both directions. An inlet probe checks production world-coordinate wrapping, time interpolation and gain against CPU samples. Zero FFT power must generate no shoreline flow/foam. Existing optics and sky tests remain enabled. `cargo test --all-features` covers configuration compatibility; use `cargo test --workspace --all-features` to also run the CPU suites in `tui-2d-prototype`.

The heightfield cannot overturn or produce airborne spray. First-order numerical diffusion and the coarse alongshore spacing limit small-scale hydrodynamic detail; the lace texture supplies visual detail. Refraction remains the existing optical path implementation. Performance depends on substep count, mesh resolution and GPU; no frame-rate guarantee is implied.


### Dock parameter workspaces

The interactive window uses `egui_dock 0.18` (matching egui 0.33). Five fixed icon entries on the left show their names on hover; the right side switches between independent Dock layouts:

- **Scene**: scene and terrain, models and regions, colliders.
- **Shallow Water**: SWE and FFT input, nearshore foam simulation, spray simulation.
- **Offshore**: spectrum and waveform, offshore foam.
- **Shading**: water and sky, foam appearance, surface and sand, spray appearance.
- **Configurations**: existing named configuration management.

By default parameters are on the left, and Render View occupies the main area on the right; except in Configurations, the parameter side defaults to upper and lower areas. Drag tabs and dividers, close parameter tabs, then restore or add them across categories from the top **Panels** menu; each layout retains one non-closable Render View and does not support separate floating windows. **Panels → Reset current layout** resets only the current category. Render View creates its target at the actual size and scale; drag to rotate the camera and use the scroll wheel to zoom. Scrolling the parameter area does not operate the camera.

Switching categories does not change simulation, time, pause, camera, or unapplied drafts. Unavailable parameters and panels are hidden according to mode, retaining their values and a way to restore them. Layouts and the current category are stored separately beside the session in `<session file stem>.workspaces.json` and restored on normal restart; named configurations do not change layouts. A corrupt layout falls back only for the UI and reports an error; the original file is not overwritten until an explicit reset or confirmed repair. `--no-settings`, read-only `--config`, snapshots, and verification do not implicitly read or write layouts.

The top always shows playback/speed, time/camera, and the panel menu; the right side shows FPS, CPU, and asynchronous real GPU main-frame time, wrapping to another line in narrow windows. When SWE is actually enabled, its GPU sub-interval is shown; unsupported, waiting, failure, and idle states are clearly labeled. The main frame includes simulation, scene, and GUI, but not presentation wait. See the complete constraints in the [workspace UI specification](../specs/workspace-ui.md). Command-line offscreen screenshots still output only the ocean, without the Dock interface.


### Nearshore wavelet normals

The SWE panel's **Wavelet normals → Normal detail strength** controls nearshore visual detail, default 1, range 0–3, with 0 disabling it. Changes apply immediately without resetting SWE; the parameter is saved with other settings. The CLI equivalent is `--shore-normal-strength` (after `--beach`).

The fragment shader reads unfiltered FFT water levels and constructs approximate high-frequency detail from the difference between central-difference slopes at spans of 1 and 4 grid cells; it is independent of SWE input filtering and is not a strict frequency-domain band-pass. Grid-point slopes are reconstructed with periodic Catmull–Rom bicubic interpolation, preserving original grid-point values and keeping slope and its first spatial derivatives continuous across cells, avoiding the square highlight transitions of bilinear interpolation; this is not mip trilinear filtering. Water and foam reuse the same wavelet slope sample but retain their respective base normals; geometric sampling and vertex normals of the main FFT water surface remain unchanged. Detail is added to normals according to the nearshore blend weight, fades out over water depths of 2–25 cm, and is suppressed according to pixel footprint to reduce distant shimmer. It affects water reflection, highlights and surface lighting, as well as normals used by enabled view refraction/solar caustics; it does not modify geometric displacement, water level, flow, foam generation, or beach residue. Reflection and highlights require Water optics' Sky reflection and highlights; physical debug views remain unchanged. This is not actual propagation or refraction of short waves in SWE. `--verify-optics` checks production wavelet normals for cross-cell smoothness, periodic seams, grid-point slopes and strength/unit conversion, fading, and disabled/offshore protection.
<a id="first-and-second-order-swe-and-gpu-timing"></a>
### First- and second-order SWE and GPU timing

At runtime, specialized compute pipelines are cached by water/foam order combination, eliminating disabled higher-order branches; the first use of a combination may incur a short compilation cost. MUSCL water and foam each reuse an axis-aligned five-cell neighborhood within the same compute pass; the water fluxes on both faces and the bed source term share the center reconstruction, while foam reuses the center slope and terrain-blocking queries. Solver cell centers read the uploaded bed directly, while rendering samples at arbitrary world coordinates still use interpolation; normal advancement reads FFT water level and momentum only in the boundary relaxation zone, while reset still initializes the entire domain from the current FFT. These optimizations add no grid buffers or passes and do not change the default order, CFL, two-stage RK2, source terms, or materials. The predictor writes only the conserved quantities and foam momentum needed by the corrector; dual concentrations and texture motion share backtraced positions but retain their respective interpolation rules. Complete substeps alternate output between two complete fields; spray sources read matching old and new fields, with synchronization only once at the end of the frame. A fixed public rendering field is still retained.

**Shallow Water → SWE solver** switches between **First order (Euler)** and **Second order (MUSCL + RK2)**. First order remains the default, and the selection is saved. Switching resets nearshore water, foam, and wet sand, while FFT time continues advancing. Both modes use the same selected grid and CFL substep condition; second order performs two stages per substep.

The panel displays the most recently asynchronously sampled **SWE GPU milliseconds, substep count, advanced simulation milliseconds, and time per substep**. Timestamps cover the start of the first SWE pass through the end of the last pass in the frame, including intermediate parameter copies, foam/wet-sand updates, and spray sources when enabled; they exclude FFT, rendering, particle updates, CPU submission, end-of-frame state synchronization, and timing readback. Only one readback can be in flight; sampling is skipped while it is incomplete, without blocking the frame loop. Pausing/disabling displays idle; GPUs without timestamp query support display unsupported, rather than presenting CPU time as GPU time.

For comparisons, first set **Normal detail strength** to 0 and use the same FFT, filtering parameters, grid, and time multiplier. After switching, wait for waves to reach shore again; click Restart time in Ocean controls when the same phase is needed. When comparing timings, also check the substep count to avoid workload differences caused by changes in frame duration. Second-order SWE is still nondispersive and cannot recover waves that the grid cannot resolve.

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-order 2 --shore-normal-strength 0
```

GPU verification covers still-water preservation, local pulse mass and transport, wet/dry boundaries and foam during recession, extreme slopes, switch resets, and propagation error for a small-amplitude wave with a 4-meter wavelength over a flat bed in both modes. This wave test uses 16 cells per wavelength; it does not imply that the default alongshore 1.5-meter cell spacing can resolve a 4-meter wave.


### Water surface foam and beach residue colors

**Foam color → Water foam** in the SWE panel controls water surface foam, including offshore whitecaps. **Restore default foam color** restores the near-white palette. Color changes apply immediately and are saved manually without resetting the water; lighting and underwater absorption still affect the final displayed color. The CLI uses `--water-foam-color R,G,B`, with a range of 0–1 in linear RGB, placed after `--beach`.


Residue visibility correction: beach residue now uses concentration to adjust opacity, while the texture only modulates mottling, preventing low-concentration residue from being completely erased by the texture threshold. Nearshore water surface coverage now uses sampled SWE water depth, preventing coarser water surface triangles from spanning dry cells and obscuring residue. **Distinguish locations: water green / sand red** marks surface foam in green and actual beach residue in red; **Default distinction colors** restores the normal palette. A wider foam band next to the water usually still belongs to surface foam; residue is the narrow receding-water trace on the landward side.


### Foam surface density debugging

Click **Density debug: low green / high red** in the **Shallow Water** panel, or select **Foam density (low green / high red)** in **Shallow water debug view**. CLI: `--beach --shore-view 6`. Water surface foam and beach residue use the same color scale: green for low density, yellow in the middle, and red at or above the reference value; the background remains visible where there is no foam. Debug colors bypass foam texture, lighting, and offshore whitecap blending; very weak density below 0.01 fades into the background. The normal Shaded view retains the existing material.

- **Background generation rate (per second)**: Controls the background source, scaled by the background generation multiplier; front generation is controlled independently. Set both this rate and the front rate to 0 to observe transport and convergence of existing foam.
- **Water foam decay (per second)**: Exponential decay coefficient per second; 0 disables water surface foam decay.
- **Convergence enhancement threshold (per second)**: Threshold for the compression enhancement term in the generation model; it does not change the convergence effect of conservative transport itself.
- **Density color reference**: Density corresponding to red, default 2; adjusts only the display mapping and does not clip simulation density.

These parameters apply immediately; use **Reset shallow water, foam, and sand** or restart time when a fair comparison is needed. Water foam is cleared after recession and no longer deposits sand residue. No pore texture model is added at this stage. GPU verification covers foam mass conservation, translation, densification under convergence, dilution under divergence, exponential decay, and wet/dry transitions in both solver modes.


### FFT initialization and input gain

**Shallow Water → FFT input → FFT → SWE gain**, range **0–3**, default **1**. This factor multiplies both the FFT water level perturbation and the two horizontal momentum components, and is used for both full-domain initialization and continuous boundary input; offshore FFT display is unaffected. It multiplies the existing **Wave height gain**; 0 resets SWE to still water and disables subsequent input. The parameter is saved manually; changing it reinitializes SWE even when paused, clearing foam and wet-sand history.

Initialization uses the FFT field at the current time with the current filtering parameters, not the previous frame's FFT, and no longer waits for a 6-second ramp-up. Water depth is calculated as “FFT water level minus terrain height” and constrained to be nonnegative; momentum is zeroed in dry areas, while velocity safeguards remain in very shallow water. Consequently, when safety limits constrain a strong gain, the actual amplitude is not guaranteed to triple exactly. Initialization is not a complete shallow-water steady-state solution; propagation phase still adjusts afterward, and shoreward run-up and foam develop.

CLI example: `cargo run --release --features ocean --bin ocean -- --beach --swe-input-gain 1.5`.


Deposition, storage, decay, and rendering of residual foam on sand, along with the corresponding color and deposition-depth controls, have been removed; wet-sand history and drying time remain. Water foam is cleared at water depths of 3 mm or less, leaving no ground residue; water surface geometry clipping and solver wet/dry safeguards still use their respective thresholds. `stranded_foam_color` and `foam_deposit_depth_mm` in old configurations are ignored; old sand foam view 5 loads as normal shading, while water density view retains number 6. The CLI options `--stranded-foam-color` and view 5 have been removed.


Nearshore background foam generation conditions can be adjusted in **Shallow Water**:
- **Background generation start threshold**: Start threshold, default 0.25, range 0–2. The criterion is “water depth rise rate ÷ local shallow-water wave speed,” dimensionless; lowering it makes foam generation easier.
- **Background generation transition range**: Transition width from the start threshold to full generation weight, default 0.40, range 0–2. Thus, by default generation starts at 0.25 and reaches full weight at 0.65. A smaller value increases it faster; 0 means full weight is reached immediately once the threshold is strictly exceeded.

Both parameters apply immediately, are saved manually, and do not clear existing foam. Still water produces no foam even when both are 0. They control only the background generation weight and do not affect the new front source; actual foam quantity also depends on generation rate, background multiplier, compression enhancement, water depth, transport, and decay. Old settings files missing these fields use defaults.


## Fresh white foam N / water surface residual R experiment

Enable under **Shallow Water → Fresh white foam and water surface residual → Dual-concentration experiment**. It is off by default; old configurations missing the field continue to use the original single-concentration conservative/inertial path. Switching modes resets nearshore water, foam, and wet sand without restarting FFT; changing the three parameters below applies live, is saved manually, and does not clear history.

| Parameter | Default | Range / unit |
| --- | --- | --- |
| Fresh half-life | 1.4 | 0.1–60 simulation seconds |
| Residual half-life | 9 | 0.1–120 simulation seconds |
| Exit-to-residual fraction | 0.65 | 0–1, dimensionless |

The exit rate of fresh concentration **N** is `a = ln(2) / fresh half-life`, the decay rate of residual concentration **R** is `b = ln(2) / residual half-life`, and the conversion fraction is denoted by `f`. The local relations are `dN/dt = new generation rate − aN` and `dR/dt = f·aN − bR`. All existing front/background sources inject directly into N only; no independent rock-impact foam generator is added. The half-life already includes exit due to conversion, so fresh decay must not be applied a second time.

Local generation, conversion, and decay are integrated analytically together, including cases where the two half-lives are equal or close; the update occurs once per complete SWE substep and is not repeated in the second-order predictor stage. Without sources or transport, starting from `N=1, R=0`, after 1.4 seconds `N=0.5, R≈0.306191`: the cumulative amount transferred into R is 0.325, but it also decays after conversion.

When enabled, N, R, and both sets of material coordinates are backtraced using the same coarse SWE flow velocity; concentrations use bilinear semi-Lagrangian sampling and are both multiplied by the local compression/expansion response. This mode does not use the old foam momentum transport, drag, following multiplier, newborn velocity multiplier, spatial flux order, or “Water foam decay”; those settings are retained for use when the experiment is disabled. The compression response applies only to this semi-Lagrangian path and is not added to the old conservative flux. First- or second-order SWE remains selectable; the entire foam process is not strictly conservative or frame-rate independent, and still has backtrace interpolation, flow-field approximation, and operator-splitting errors.

- Neither concentration is limited to 0–1; exposed obstacles block backtracing, but passage is allowed after submersion. No foam is imported from outside, and the FFT relaxation zone dissipates both. Both are cleared when water depth is 3 mm or less; R is **not sand residue**.
- When enabled, separate N/R appearances are used, while single-concentration NSH/prototype material settings are retained. By default N displays bright white patches using two-scale triangular-grid gradient noise, with the fine layer rotated and its coordinates offset to reduce grid alignment at sparse fronts; a constant threshold or one of three NSH textures may also be selected, as described below. R always uses a narrow isoband of fine-scale multilayer noise, colored at 72% of the current water foam color; filaments and spots appear as concentration decreases. It does not follow N's texture, micro-normal, or roughness. Shading remains on the same water surface; no new geometry layer, fine-scale turbulence, or individual bubble simulation is added.
- “Opacity density scale” controls the coverage response of N and R respectively; the coordinate reset period remains effective, default 2 seconds. Eight seconds retains longer stretching; 0 means continuous transport without reset. Material debug 1/2 respectively show fresh N's threshold field or texture Soft / residual noise; 3 shows its micro-normal where N texture is present and the original macro normal elsewhere; 4 shows composite opacity, and 5 shows the shared transport grid.
- Switch “Texture flow distortion” under “Shading → Foam appearance → Nearshore foam material” (on by default). When off, all nearshore foam materials use fixed world coordinates, while N and R concentrations continue to transport and update normally; displacement history continues advancing, and re-enabling immediately restores current distortion. Applies live, saved manually, and does not reset the simulation. A coordinate reset period of 0 means continuous distortion, not disabling distortion.
- Under “Foam material → Nearshore foam material → Foam display component,” select “All / Fresh foam N only / Residual foam R only,” default All. Available in dual-concentration mode; applies live, is saved manually, and does not reset or change the simulation. Viewing either separately retains its corresponding material and appearance height and hides offshore whitecaps; density debug views are independent of this option.
- Density debug `--shore-view 6` displays N+R, `9` displays N, and `10` displays R; all bypass noise, lighting, and offshore foam. When the experiment is off, view 9 displays the original single concentration and 10 has no residual.
- Pausing freezes both; reset, disabling, and grid rebuilding clear both. Editing colliders while paused also clears R from newly dry cells while preserving wet-sand history. Offshore FFT foam remains independent.

```powershell
# Finite-frame trial without reading or writing personal settings.
cargo run --release -- --beach --no-settings --foam-dual-phase --frames 600
# Enable the experiment from the saved scene; do not add --beach to override it.
cargo run --release -- --foam-dual-phase
# Read-only comparison with a fixed reef configuration.
cargo run --release -- --config configs/experiments/reef-front-ridge.json --no-foam-ridges --foam-dual-phase --foam-texture-period 8
```

The CLI also supports `--no-foam-dual-phase`, `--foam-fresh-half-life`, `--foam-residual-half-life`, and `--foam-conversion`. Configuration fields are `foam_dual_phase`, `foam_fresh_half_life`, `foam_residual_half_life`, and `foam_conversion`; both sessions and named configurations save them, but running concentrations are not saved. `--verify` checks the GPU analytical update against an independent f64 ODE integration, including timestep splitting, equal rates, 0/100% conversion, and shared transport, compression/expansion, generation into N only, wet/dry boundaries and obstacles, pause, reset, and hydrodynamic independence in both actual SWE orders. A fixed reef snapshot has shown nearshore white patches and fine peripheral streaks; this does not establish reproduction of real foam morphology.

### Fresh N appearance: constant / noise / texture

Use the “Constant | Noise | Texture” segmented selector under “Render mode” in **Shading → Foam appearance → Fresh foam N appearance**; it is shown only in dual-concentration mode. Noise remains the default and matches the old effect. Only parameters valid for the current mode are shown; switching retains all parameters for all three modes, applies live, does not clear concentration or reset water or coordinate history, and requires manual saving. Old configurations missing fields load the defaults in the table below.

| Parameter | Configuration field | Default / range |
| --- | --- | --- |
| Render mode | `foam_fresh_threshold_mode` | 1 Noise; 0 Constant, 2 Texture (original field name retained for compatibility with old configurations) |
| N constant threshold | `foam_fresh_threshold` | 0.5; 0–1, dimensionless |
| N primary noise frequency | `foam_fresh_noise_frequency` | 0.8; 0.05–20/meter |
| N detail noise frequency | `foam_fresh_noise_detail_frequency` | 2.5; 0.05–60/meter |
| N detail noise weight | `foam_fresh_noise_detail_weight` | 0.3; 0–1, dimensionless |
| N texture source | `foam_fresh_texture` | 1 Shorewaves NSH; 2 Crest Foam NSH, 3 Crest Foam2 NSH |
| N texture size | `foam_fresh_texture_size` | 12; 0.25–64 meters, coarse-layer world-space side length |
| N micro-normal strength | `foam_fresh_micro_normal` | 0.65; 0–3, dimensionless, 0 disables micro-normal |
| N roughness | `foam_fresh_roughness` | 0.85; 0.15–1, dimensionless |
| N coverage reference | `foam_fresh_coverage_reference` | 2; 0.01–20, simulated foam quantity/area, not a measured mass unit |
| N density controls opacity | `foam_fresh_density_opacity` | Off, available for all three N render modes |
| N density range | `foam_fresh_opacity_range` | 1; 0–10, simulated foam quantity/area |
| N density exponent p | `foam_fresh_opacity_power` | 1; 0.1–5, dimensionless |
| R density controls opacity | `foam_residual_density_opacity` | Off, in the “Residual foam R appearance” group |
| R density range | `foam_residual_opacity_range` | 1; 0–10, simulated foam quantity/area |
| R density exponent p | `foam_residual_opacity_power` | 1; 0.1–5, dimensionless |

Constant mode replaces N's noise threshold field with a uniform value; a lower threshold makes it easier to display. Both modes compare against `1 − exp(−N / opacity density scale)`, retaining the original soft transition, invisibility at zero concentration, and thin-water fade, rather than hard-clipping raw N directly. Noise mode retains triangular-grid gradient noise, fine-layer rotation/offset, and pixel-footprint filtering; higher frequency makes the pattern finer and is not an exact periodic wavelength. Primary/detail layers are mixed by `1−weight` / `weight`, default 70%/30%; a weight of 0 or 1 isolates one layer.

Texture mode reuses full NSH rendering for N, not merely an image in place of noise: N concentration determines pore coverage and continuous opacity, while texture micro-normals and roughness determine N lighting. The three assets, linear channels, mip levels, and licenses are the same as in [NSH foam textures and materials](#nsh-foam-textures-and-materials). It does not include the “Original grayscale effect”; NSH Height is not geometric height and does not raise the water surface. The coarse-to-fine layer size ratio remains 10 and weights remain 72%/28%, using existing transport coordinates and dual-phase blending; each phase first completes N/R shading and compositing separately, then blends final results, avoiding abrupt hole changes from mixing textures first.

N texture parameters are independent of the single-concentration mode's texture settings. Pore filling uses a separate “N coverage reference,” not “Density color reference”; texture opacity is “pore coverage × `1 − exp(−N / shared opacity density scale)`,” retaining zero-concentration and thin-water safeguards. The shared density scale still affects both N and R and is located in the shared material group, not among N's independent parameters. NSH and constant/noise use different concentration-to-visible-coverage curves, so the same N value may display different areas without changing simulated concentration. Existing `--foam-texture` still selects the single-concentration texture; N's new fields are loaded through session/named configurations or read-only `--config`.

These parameters change only the dual-concentration N material; they do not alter generation, transport, N→R conversion, decay, simulated density, foam geometry height, density debugging, the single-concentration material, or R's own material. Changes in N coverage can still alter how much R is occluded in the composite. Constant mode does not change its threshold with texture motion; shared coordinates continue updating for R. Real GPU verification checks that pure-N appearance for the three N textures matches the original single-concentration NSH, that each independent parameter actually changes rendering, that R-only lighting/normals/opacity and the three density views are unaffected by N appearance, and that the full simulation state remains consistent for first- and second-order SWE.

#### Independent N/R density opacity factors

“Fresh foam N appearance” and “Residual foam R appearance” each have a “Density controls opacity” switch, off by default. When enabled, only that foam category's “Density range” and “Density exponent p” are shown; disabling it retains the parameters and restores the original appearance. Density range is 0–10 and p is 0.1–5, both defaulting to 1. The categories are configured separately, apply only to dual-concentration mode, take effect live, are saved manually, and do not reset history.

In the formula, `n` is the local simulated concentration of the current category, `N` is its configured density range (not the fresh foam category name), and `p` is the exponent. The additional factor is `pow(clamp(n / N, 0, 1), p)`: 0 makes that category completely transparent, while 1 retains its original appearance. It multiplies the existing pore coverage and density response rather than replacing the original curve; it scales both the category's premultiplied color and opacity, then fresh foam is composited over residual foam. It does not merely darken RGB. All three N render modes, constant/noise/texture, support it; R uses its own concentration and parameters rather than N's. Lowering the N factor can reveal more R; this is a change in composite occlusion, not R concentration.

When density range is 0, positive concentration normalizes to 1 and zero or negative concentration to 0, without division by zero; p cannot be 0. Increasing density range or p makes unsaturated portions more transparent, while lowering p makes them more opaque. Zero-concentration and thin-water safeguards remain; pore input patterns, micro-normals, foam geometry height, density debugging, and N/R simulation are unchanged. Both switches are off when old configurations lack the fields. GPU checks use an independent f64 power-function reference, range-zero/clamping/p boundaries, and verify that actual rendered color and opacity approach the underlying water color together according to the factor, rather than merely darkening.

## Front foam: directional compression

Front generation retains only **directional compression**; detection method switching is no longer provided. Compression strength is `max(component of current velocity along sampling direction − component of forward velocity along that direction, 0) ÷ actual sampling distance`, in inverse seconds. Catch-up in the same direction and head-on convergence can trigger it; separation along the flow and purely transverse velocity differences cannot.

Samples are taken along the current flow direction at 1/4, 1/2, 3/4, and 1 times the sampling distance; compression strength is calculated for each and the maximum is used. Water velocity from the same solver stage is used, calculated in world-space distance, and supports non-square grids. Bilinear interpolation counts only wet corners; out-of-bounds and completely dry points are skipped. Detection is disabled when current speed is at most 0.02 m/s and smoothly enabled from 0.02 to 0.1 m/s.

Adjust under **Shallow Water → Front foam**:

| Parameter | Default | Range / meaning |
| --- | --- | --- |
| Front generation rate (per second) | 8 | 0–30, generation rate at full weight; 0 disables it |
| Front sampling distance (meters) | 1 | 0.05–10 |
| Velocity change start threshold (per second) | 0.1 | 0–20, generation starts once compression strength exceeds it |
| Velocity change transition range (per second) | 0.5 | 0–20, smoothly rises to full weight from the threshold to the threshold plus this range; 0 is a hard switch strictly above the threshold |
| Background generation multiplier | 0.15 | 0–1, set to 0 to observe the front source alone |

For example, for flows 0.5 meters apart, with same-direction velocity dropping from 2 m/s at the rear to 0.5 m/s ahead, compression strength is 3/s; reversing them gives 0. Strength maps to a generation weight of 0–1 and does not directly limit accumulated foam density. Background and front sources are added, then thin-water suppression at 3–30 mm and offshore boundary fade are applied; the result is added only once per complete SWE substep.

Parameter changes apply live, are saved manually, do not clear existing foam, and do not change the water equations. `foam_front_mode`, `foam_front_depth_drop`, `foam_front_gain_points`, and `foam_front_power` in old configurations are ignored; other parameters load normally. Set the background multiplier to 0, use density debugging, and fix time, camera, FFT, and SWE order to observe the effect. The compression criterion is an approximate signal for shallow-water foam generation and does not resolve actual air entrainment.

## Foam momentum, catch-up, and convergence

Adjust under **Shallow Water → Foam momentum and water drag**:

| Parameter | Default | Range | Meaning |
| --- | --- | --- | --- |
| Foam spatial transport | First order | First order / second-order MUSCL | Selection is enabled only with second-order SWE (RK2); first-order SWE always uses the original flux |
| Enable water drag | On | On/off | Disabling retains independent inertia for isolated verification; generation, decay, and dry-cell clearing still operate |
| Flow following multiplier | 0.8 | 0–1 | Foam target velocity is water velocity multiplied by this factor |
| Velocity response time (seconds) | 0.35 | 0–20 | Time constant for velocity approaching the target; 0 means immediate following |
| New foam velocity multiplier | 1 | 0–1 | New foam velocity is local water velocity multiplied by this factor; affects only newly generated foam |

The following applies to the default single-concentration mode; the dual-concentration experiment uses SWE flow transport as described above. By default each cell stores “foam quantity/area” and “foam quantity/area × horizontal velocity.” Both are transported across cells using the same upwind weights; cell velocity equals momentum divided by foam quantity. Thus fast foam catching slow foam accumulates and densifies, with velocities mixed weighted by foam quantity. For example, if the front patch has foam quantity 0.2 and velocity 0.2 m/s, and the rear patch has quantity 0.8 and velocity 1 m/s, after complete mixing the total quantity is 1, total momentum is 0.84, and average velocity is 0.84 m/s. Local density may exceed 1; convergence does not create additional foam.

New foam injects momentum equal to “new quantity × newborn velocity,” while decay removes foam quantity and momentum in the same proportion. Drag acceleration is “difference between target velocity and foam velocity / response time.” For fixed flow and generation rate within each substep, generation, decay, and drag are integrated analytically together; with drag alone, velocity approaches the target exponentially. With drag off, following multiplier and response time do not affect inertial transport. Foam gradually stops only when the following multiplier is 0 and drag is enabled. This model does not feed back on the water.

The fourth SWE history block stores foam surface momentum and the first set of texture displacement; the fifth stores the second set of texture displacement and the update phase. The fifth block's w stores surface R in dual-concentration mode, the original density slot stores N+R, and R is zero in default single-concentration mode. The buffer still has five blocks and is unchanged in size. Foam quantity and both momentum components share conservative fluxes and advance together in RK2 prediction and correction; generation and drag are integrated only once when the substep completes. Transport velocity retains SWE's 12 m/s limit and CFL timestep constraint. Both sets of texture displacement are backtraced and accumulated using the same foam velocity field; material coordinates are “world position − displacement.”

To avoid streaking from unlimited accumulation, each texture retains at most **2 seconds of simulation time** of deformation, and the two update **1 second** apart. The display weight of a texture is zero when it updates, and the two coverage weights alternate smoothly; coordinates are not mixed, nor are two noise fields directly averaged before thresholding. Each texture still moves and stretches with the flow while displayed, but individual pores gradually refresh and cannot be tracked forever. Foam density and velocity are not cleared when textures update.

Texture tracking and rendering interpolation use displacement only from wet cells and renormalize weights, preventing zero displacement in dry cells from pulling wet-area textures. Initial coordinates are used when there are no wet samples. Update phase advances with actual SWE substeps, does not update while paused, and updates only once after a completed RK2 substep; dry cells still maintain a synchronized phase.

Parameters apply live, require manual saving, and do not reset history. Pausing preserves momentum and texture history; reset and disabling clear history, while dry cells clear foam and its momentum without leaving sand residue. The convergence criterion for background generation continues to use water velocity; front generation uses only directional compression. The thin-water material's 8 mm cutoff is unchanged.

Foam velocity uses depth-averaged water flow as its target; semi-Lagrangian transport of texture displacement still has numerical diffusion, and strong shear during a single 2-second interval can still cause stretching. Foam spatial transport remains first order by default; under second-order SWE (RK2), “Foam spatial transport” can be switched to second-order MUSCL. It performs limited linear reconstruction jointly on foam quantity and both momentum components, with uniform scaling to keep face density nonnegative and avoid significant neighborhood overshoot in face velocity. It locally falls back to first order at wet/dry interfaces (reconstruction neighborhood depth at or below 20 mm or the wet threshold), obstacle blockages, and domain edges. It retains the shared Rusanov flux and existing 0.2 CFL; it does not preserve shape by clipping to 0–1, nor change generation, decay, drag, two-second texture updates, or materials. It reduces numerical diffusion but does not create new turbulence or permanently preserve sub-cell filaments.

When `foam_transport_order` is missing, it defaults to 1, so old configurations do not change behavior. Selection applies live without clearing history; a saved second-order selection has no effect while SWE is first order. The following command enables it for a fixed scene without reading or overwriting personal settings:

```powershell
cargo run --release -- --beach --no-settings --swe-order 2 --foam-transport-order 2
```

`--verify` first uses the production GPU pipeline to verify two foam patches, slow in front and fast behind: each 2 m long, separated by 1 m, rear density 0.8 and velocity 1 m/s, front density 0.2 and velocity 0.2 m/s, in still water with generation, drag, and decay disabled, advancing 5 seconds. It checks conservation of total quantity and total momentum in both directions, center-of-mass displacement, convergence density growth, and relative kinetic energy dissipation, covering first-order SWE, first-order foam + RK2, and MUSCL foam + RK2; it also scales the MUSCL catch-up scenario to an initial density of 1e-7 to test near-vacuum behavior. Second-order foam also covers obstacles, wet/dry clearing, and full coastal advancement. An independent f64 RK4 reference additionally checks WGSL newborn momentum, drag, decay, and timestep consistency, followed by verification of coastal propagation, wet/dry clearing, pause, parameter changes, and rendering.

In RTX 3070 / Vulkan verification of the original first-order spatial flux, Euler/RK2 peak densities were 1.0483/1.0441 respectively, and total momentum divided by total quantity was approximately 0.84 m/s in both; relative total quantity error was about one part per million, and momentum error about two parts per ten million. On a 0.25 m grid, the center of mass moved about 4.10 m, compared with 4.20 m for the continuous conservation equation, an approximately 0.10 m first-order spatial error; conservation of total quantity and momentum does not mean trajectories have no numerical error.

### Physical basis and recommended starting point

“Physical reference starting point → Apply recommended values” sets the following five items, applies live, and is saved manually; old configurations retain their existing following multiplier, response time, and decay values, with defaults supplied for missing new fields.

| Parameter | Recommended starting point | Explanation |
| --- | --- | --- |
| Enable water drag | On | Water transfers momentum to foam |
| Flow following multiplier | 1 | With water drag alone, both approach the same velocity; a value below 1 adds visual retardation |
| Velocity response time | 2 seconds | Eliminates approximately 63% of initial relative velocity in 2 seconds; compare 2–5 seconds for different degrees of retained inertia |
| New foam velocity multiplier | 1 | Inherits local water velocity at generation, then changes through drag and transport |
| Water foam decay | 0.3 /second | With no source, half-life is about 2.31 seconds and 10% remains after about 7.68 seconds; this differs from the time of visible disappearance |

[Wretborn et al., SIGGRAPH 2022, Guided Bubbles and Wet Foam](https://alexey.stomakhin.com/research/siggraph2022_whitewater.pdf) uses velocity relative to water to produce linear surface drag; Table 3 gives simulated drag coefficients of 0.05–0.5 /second. Taking their reciprocals gives response times of 2–20 seconds in this project; the recommended 2 seconds selects the stronger-drag end. This is a tuning reference for a production simulation, not a universal constant measured on a coast. The complete model also includes foam pressure, viscosity, and cohesion; this implementation borrows only the drag form and cannot be equated directly with the paper's full physical model.

[Callaghan et al., 2024, comparison of laboratory and field whitecap foam evolution](https://agupubs.onlinelibrary.wiley.com/doi/full/10.1029/2023JC020193) indicates that foam decay is influenced both by escape of underwater bubbles and stabilization by surfactants, so lifetime cannot be determined from water velocity alone. This project chooses 0.3 /second as a calibration starting point for decay on a timescale of seconds, not as a direct conversion of the paper's visible-area lifetime into this project's surface-density decay coefficient.

This implementation assumes foam quantity is proportional to effective mass and uses a pressureless single-velocity layer; mixing within a cell after collision conserves total momentum and dissipates relative kinetic energy. It does not yet simulate liquid fraction changes, drainage, bubble-size distribution, film rupture, pressure, viscosity, cohesion, wind, or foam feedback on water. It can therefore represent “catch-up–convergence–decay,” but is not a complete quantitative seawater foam model. Front generation thresholds and rates remain model- and resolution-dependent parameters and are not called physical material constants without local measurements.

## Foam morphology transport comparison

The independent offline `foam-lab` tool directly calls production GPU foam transport with fixed translation, shear, and differential rotation. By default it compares first- and second-order spatial transport on 128/256/512 grids under the same RK2 and CFL=0.2, outputting density snapshots, analytical references, error metrics, and actual GPU timestamp timings where supported at 0, 2, 4, and 8 simulation seconds. The default shape-preservation experiment does not read personal settings, run full SWE, or use foam materials. A separate `--benchmark` mode calls the full production SWE kernel, comparing three order combinations on fixed sparse/dense/synthetic model terrain/8-obstacle cases and a model with dual concentration + nonzero FFT input; after warm-up it reports repeated GPU substep timings and exports single-step state. This kernel benchmark excludes normal-frame parameter/state copies and rendering and does not represent UI FPS. There is also `ocean-profile --config PATH --output NEW_DIR`, which reads the configuration without writing to it, runs the full production path offscreen, and separately records FFT, offshore foam, SWE (including copies and spray sources), particle, and drawing times after a fixed time. See [Foam diagnostics and benchmarking](foam-lab.md) for usage, parameters, and limitations of results.

```powershell
cargo run --release --features ocean --bin foam-lab -- --output target/foam-lab
# Open target/foam-lab/index.html when complete; the output directory must not exist.
```

## Foam height: independent appearance offset

Set under **Shallow Water → Foam height (appearance only)**:

| Parameter | Default | Range | Effect |
| --- | --- | --- | --- |
| Maximum height (meters) | 0.05 | 0–1 | Maximum upward foam lift; 0 disables it |
| Start density | 0.1 | 0–20 | Height is 0 at or below this density |
| Density transition range | 1.9 | 0.001–20 | Gradually reaches maximum height from start density to “start density + transition range” |

The simulated water foam quantity/area is used as density, without first clipping to 0–1 or using the density color reference or pore texture. The calculation where water is sufficiently deep is:

```text
height weight = smoothstep(start density, start density + density transition range, foam density)
appearance height offset = maximum height × height weight
rendered vertex height = original water surface height + appearance height offset
```

For example, with defaults: density 0.1 produces no lift, density 1.05 lifts by 0.025 meters, and density 2.0 or above lifts by 0.05 meters. Density here is the existing simulated quantity, not foam material density in kg/m³; this mapping represents thickness without introducing liquid fraction or volume solving.

The offset occurs only in rendered vertices and is not written back to FFT/SWE; it does not change water level, depth, momentum, foam transport, or generation. Foam shading uses the raised surface normal, while water optics and water normals continue to use original water surface data. Parameters are independent of water surface amplitude gain, apply live, are saved manually, can be adjusted while paused, and do not clear history; old configurations missing these fields receive the defaults above.

Currently only nearshore water foam is raised; offshore FFT foam is unaffected. The effect smoothly enables over simulated water depths of 8–20 mm and is zero at 8 mm or less to prevent lifting dry land; disabling “Show foam” or SWE also disables the offset. The red/green density debug view can show height changes. The existing water mesh is reused, with no separate foam shell at pores; geometric detail of narrow foam bands is limited by render mesh resolution.

`--verify` calls the actual vertex function to check smoothstep numerical points, saturated height, wet/dry boundaries, display switches, independence from color reference and amplitude, and separation of raised foam normals from unraised water surface data. It also verifies that all simulation state remains identical when only height parameters change in first- and second-order modes, and checks actual rendered pixel changes with fixed coastal history, camera, and time.

## Static colliders

A **Colliders** tab is added to the right Dock, allowing up to 8 cylinders or cuboids. There are no obstacles by default; first enable “Sloped beach” and “Enable shallow water simulation” in the SWE panel, then add them. The panel uses the shared two-column table, with detailed explanations in name hover tooltips.

| Control | Meaning |
| --- | --- |
| Enable / delete | Toggle or remove an individual collider |
| Shape | Cylinder or cuboid |
| Position X/Z (meters) | World coordinates; increasing X points offshore, Z runs alongshore |
| Radius (meters) | Cylinder radius, 0.25–30 |
| Width / length (meters) | Full cuboid dimensions, 0.5–60 |
| Rotation (degrees) | Cuboid rotation around the vertical axis, ±180 |
| Height (meters) | Height from the original seabed at the center to the horizontal top, 0.1–30 |
| Top elevation (meters) | Automatically calculated top height; still sea level is 0 |
| Simulation status | Whether enabled and whether inside the valid SWE region |

The first added collider is placed 8 meters seaward of the shoreline; subsequent slots are offset by 8 meters along Z. Default height is 4 meters; with the default 0.18 beach slope, the top is 2.56 meters above sea level, suitable for initially observing water blockage and flow around it. Lower the height to bring the top near or below sea level to observe overtopping or submerged obstacle effects. The panel displays actual grid spacing and valid bounds; covering at least 3 cells in both directions is recommended, and circular and rotated boundaries remain grid approximations in the simulation.

Colliders participate in SWE as fixed seabed terrain: each cell's effective bed is the maximum of the original beach and the tops of all colliders covering that cell. First- and second-order solvers use the same bed, producing blockage, reflection, and flow around obstacles through existing still-water reconstruction, gravity, and wet/dry boundaries; flow can pass over the top when the water surface exceeds it. Overlapping shapes use the highest top, without adding heights. Water foam retains independent momentum transport; cross-face flux is disabled at unsubmerged obstacle boundaries to prevent passage through side walls. No extra collision foam is created from nothing; existing generation modes still determine foam from flow changes.

Cylinders and cuboids write both opaque color and depth; submerged portions participate in water absorption and scattering, while above-water portions occlude the water surface. Water surface interpolation ignores dry obstacle tops to avoid pulling false water mounds around objects. Offshore FFT remains a one-way input; SWE is displayed within 2 meters of colliders, original FFT blending is restored over 2–8 meters, and this override fades out at the computational domain boundary.

Interaction is limited to local SWE. The entire shape must be inside the simulation area and, on the offshore side, must also avoid the final 20% FFT input relaxation zone; the implementation reserves an additional boundary margin. When out of bounds, parameters are retained, but the collider is neither displayed nor used for collision, and the panel reports its status. Collision response is not written back to offshore FFT.

Adding, moving, editing the dimensions/height/rotation, enabling, or deleting valid colliders takes effect immediately, including while paused, without reinitializing SWE, restarting nearshore input, or clearing domain-wide foam and sand history. State is adjusted only in cells whose effective bed actually changes: raising the bed clips newly occupied water and scales water momentum by the remaining depth, avoiding directly raising the original water surface to the object's top; lowering the bed retains the existing water quantity, allowing surrounding flow to refill it after execution resumes without creating water. Water foam, foam momentum, and texture displacement are cleared in newly dry cells, without creating false deposits on object tops; history in other cells remains unchanged. Editing changes local water quantity and does not perform conservative drainage or water displacement by moving rigid bodies. Parameters enter the manually saved working state and named configurations; old configurations missing collider fields load as an empty scene. This is static scene editing, not moving rigid bodies: buoyancy, rigid-body motion, fluid feedback on objects, flow underneath suspended objects, and three-dimensional spray are not simulated.

`--verify` checks still-water balance for cylinders/rotated cuboids, reflection from tall walls exposed above water, water and foam not passing through walls and total quantity conservation, flow around both sides of a cylinder, overtopping of low obstacles, and local state adjustment during paused editing, preservation of history in unchanged areas, and resumption under both solver orders. Rendering verification covers above-water occlusion and underwater scene color reads for cylinders/rotated cuboids. The `OCEAN_COLLIDER_PREVIEW` environment variable can specify the output path for a verification preview PNG; normal verification writes no additional image.


### SWE grid resolution

Select 64, 128, 256, 512, or 1024 under **Shallow Water → Grid cells per axis**, default 256. This parameter is independent of FFT resolution and the render mesh, is saved manually, and is also included in named configurations; old configurations default to 256.

The simulation coverage remains unchanged; X/Z cell sizes are respectively region width/cell count and region length/cell count, and both SWE and collider panels display actual grid spacing. Switching immediately rebuilds nearshore buffers and reinitializes water with the current FFT, clearing nearshore foam and wet-sand history; FFT time and offshore foam continue running. The one-cell margin reserved at collider edges also changes with resolution.

Doubling the cell count per axis roughly quadruples cell count and primary state GPU memory; the CFL stable timestep is usually halved, so computation for the same simulation time may increase roughly eightfold. Three state buffers occupy approximately 15/60/240 MiB at 256/512/1024 respectively. Compare SWE GPU time with actual advanced time; the default limit of at most 128 substeps per frame remains in effect.

## NSH foam textures and materials

Select Original grayscale effect, Shorewaves NSH, Crest Foam NSH, or Crest Foam2 NSH under **Shallow Water → Foam textures and materials**. Shorewaves NSH is the default; old configurations missing the field receive this default, while other existing parameters load normally. Switching texture, size, micro-normal, opacity density scale, and roughness applies live, is saved manually, and can be compared while paused, without resetting the simulation or texture displacement history. `foam_soft_opacity` and `foam_thick_start` in old configurations are ignored; when the new `foam_opacity_scale` field is missing, it defaults to 2, and other parameters load normally.

All three NSH textures are baked offline from source images in the repository and standardized to 1024×1024: RG is tangent normal XY, B is Soft, and A is relative Height. They are uploaded as a linear `Rgba8Unorm` texture array, using Repeat and a complete mip chain; normals are averaged as three-dimensional directions and normalized, while Soft/Height are linearly filtered separately. Crest source images are decoded from sRGB according to their original import settings before baking; see [assets/foam/SOURCES.md](../../assets/foam/SOURCES.md) for source images, pinned versions, SHA-256, and MIT license. Height is morphology designed for shading, not measured foam thickness.

The material retains the existing two displacement fields and alternating 2-second resets. Each displacement samples coarse and fine scales separately, with the fine scale at 1/10 of the coarse scale and weights 0.72/0.28. Each phase retains the original pore coverage determined by density and Soft, but no longer separates thin/thick shading; a continuous density response is used within coverage:

```text
opacity = pore coverage × (1 − exp(−raw foam density / opacity density scale))
```

Raw density is simulated foam quantity/area, not normalized and clipped first, and not a measured mass density. At full coverage and the default scale of 2, densities 2, 4, and 6 give approximately 63%, 86%, and 95% opacity; increasing the scale makes the same density more transparent without changing the pore threshold. Density color reference still controls the debug color scale and pore filling range, but is no longer the saturation limit for NSH opacity; high density can still fill existing holes. This is an appearance mapping and adds no new morphology or distortion.

Each phase completes lighting separately before premultiplied color and opacity are blended, avoiding abrupt hole changes caused by mixing raw patterns before thresholding. Micro-normal direction adjusts to local rotation/shear of flow coordinates and is layered over the existing macro foam thickness normal. Height can still be inspected in debugging and is the source of offline micro-normals, but is no longer used for runtime thin/thick gating.

| Parameter | Default | Meaning |
| --- | --- | --- |
| Texture size | 12 meters | World-space length of each coarse-layer texture tile, range 0.25–64 meters |
| Foam micro-normal strength | 0.65 | Dimensionless, range 0–3; 0 retains the macro normal and disables the texture micro-normal |
| Opacity density scale | 2 | Simulated foam quantity/area, range 0.01–20; at full coverage this density corresponds to about 63% opacity, with larger values more transparent |
| Foam roughness | 0.85 | Range 0.15–1, controls the width of simplified foam highlights |

Material debugging provides four views: Soft, Height, world normal, and final opacity. View 4 was formerly thin green/thick red and now displays opacity including pores and dual-phase blending: black is 0 and white is 1. It applies only under normal shallow-water shading with foam display enabled; existing red/green density debugging still directly displays simulated density. Continuous opacity applies only to the nearshore NSH path; the original grayscale effect and offshore FFT foam remain unchanged. Material parameters themselves do not change density, momentum, generation, decay, wet sand, or foam appearance height.

```powershell
# Rebuild the three NSH textures and their recipe; previews are written to the ignored target directory.
cargo run --features ocean --bin foam-bake
# View custom source image, color space, curve, blur, and normal parameters.
cargo run --features ocean --bin foam-bake -- --help

# Fix the default seed, scene, and time to compare sources 0–3 (the output path must not exist).
cargo run --release --bin ocean -- --beach --no-settings --foam-texture 2 --time 6 --snapshot target/crest-foam.png
# Debug NSH Height.
cargo run --release --bin ocean -- --beach --no-settings --foam-texture 2 --foam-material-view 2 --frames 600
# Expand the density response range and inspect final opacity; put the scene preset before the parameters.
cargo run --release --bin ocean -- --beach --no-settings --foam-opacity-scale 4 --foam-material-view 4 --frames 600
```

See [assets/foam/README.md](../../assets/foam/README.md) for the bake process and preview panel order. Output images are embedded at compile time, so ocean must be rebuilt after rebaking. The current tool processes existing grayscale images into NSH; it does not yet implement a generator for bubble-size/network morphology from random seeds, nor has it been changed to three phases.

`--verify` checks production WGSL zero-density/dry-water boundaries, exponential opacity beyond the old reference value, density and scale monotonicity, pore preservation, normal direction and normalization, and actual GPU three-layer texture sampling, Repeat, and mip scalar averaging. Existing first-/second-order comparisons also check that material parameters do not affect simulation state.

Nearshore foam retains background and front generation; their rates are added before thin-water suppression and inlet fade. Flow-field-blend generation and its parameter panel, along with the `--foam-source-mode` option, have been removed; related fields in old configurations are ignored, while other parameters load normally. Water foam transport, convergence, and drag remain unchanged.


## Underwater caustics and refraction

In the “Shading” category, the caustics Tab offers “Off | Physical,” and the refraction Tab offers “Off | Fast | Physical.” The two are selected independently and default to off. Old configurations retain their enabled states; former fast caustics use physical caustics, and a missing refraction method defaults to physical. Parameters are saved with the current session and named configurations; switching applies live without resetting FFT, SWE, or foam, and each mode retains its own parameters. Water refractive index is shared between the two Tabs. Flat beds, sand slopes, and imported reef models are supported. Model caustics use the prebaked heightfield as the receiving surface; refraction uses actual opaque model depth to determine intersections.

```powershell
# Flat bed: enable both effects with fixed parameters, without overwriting personal settings.
cargo run --release -- --no-settings --caustics --refraction
# Sand slope: retain FFT → SWE coupling.
cargo run --release -- --beach --no-settings --caustics --refraction
# Reef: share the existing heightfield and FFT/SWE.
cargo run --release -- --model --no-settings --caustics --refraction
# Physical caustics with fast refraction.
cargo run --release -- --model --no-settings --caustics --refraction-mode fast
# Run only the real GPU caustics/refraction checks.
target\release\ocean.exe --verify-optics
```

| Parameter | Default / range | Meaning |
| --- | --- | --- |
| Enable underwater caustics | Off | A refracted sunlight mesh generates underwater illumination |
| Caustics strength | 1 / 0–4 | Dimensionless; 1 uses computed transmitted direct light, 0 restores original direct light |
| Caustics coverage width | 128 / 8–512 meters | Side length of the world-space receiving region |
| Caustics softness | 1 / 0–4 texels | Five-point filter radius of the 512×512 illumination map |
| Caustics beam grid | 256 / 128, 256, 512 | Beam cells per axis, independent of water simulation resolution |
| Enable underwater refraction | Off | Air→water refraction of viewing rays |
| Water refractive index | 1.333 / 1–1.6 | Shared with caustics; 1 produces no bending |
| Maximum refraction path length | 100 / 1–500 meters | Falls back to straight sampling beyond this range |

Corresponding CLI: `--caustics`/`--no-caustics`, `--caustics-strength`, `--caustics-extent`, `--caustics-softness`, `--caustics-resolution`, `--refraction`/`--no-refraction`, `--water-ior`, `--refraction-distance`. Place scene presets before these override parameters.


Fast refraction uses the current normal, viewing direction, and refractive index to estimate a refracted hit on the local horizontal plane at the original background receiver's height, then projects it into a screen UV offset; it is not ray marching. Thin water and grazing angles reduce the offset, and the maximum offset is limited relative to screen height. Candidate depth must be behind the water surface and the receiver must be underwater; color uses four-point bilinear reconstruction with a depth mask, with smooth fallback at screen edges. Path length and integration direction come from the estimated receiver position. This can reduce stretching near shorelines and reef outlines, but cannot recover occluded/offscreen seabed or guarantee a geometrically exact intersection.

| Fast parameter | Default / range | Meaning |
| --- | --- | --- |
| Refraction distortion strength | 1 / 0–2 | Scales the estimated UV offset |
| Refraction depth for full distortion | 2 / 0.01–20 meters | Water thickness at which full distortion is reached |
| Refraction offset limit | 4 / 0–20% of screen height | Aspect-ratio-independent maximum screen offset |

Fast refraction skips the screen depth pyramid and hierarchical tracing. Physical mode is described below; “Physical” refers to the existing approximation based on wave-surface and receiver intersections, not full three-dimensional path tracing.

Caustics share water surface position, FFT horizontal displacement, SWE blending, and nearshore FFT wavelet normals; wavelet filtering is determined by beam sampling and does not use camera screen derivatives. Visual foam lift does not participate in refraction; nearshore foam uses density and material opacity density scale for a continuous light-blocking approximation without changing foam state. Each beam's seabed hit is calculated, irradiance is computed from incident projected area / receiving area, and overlapping beams are accumulated additively in a floating-point map; entry Fresnel loss and RGB extinction from sun to seabed are included. Sand replaces only direct sunlight, ambient light retains its existing semantics, and extinction from seabed to viewer remains the responsibility of water integration.

The receiver map is fixed in world space: centered on the SWE or model region when enabled, otherwise on the origin; it does not move with the camera. The emission region has a guard band and is offset toward the incoming sun direction. The receiving region edge fades to transmitted direct light for a flat water surface, so distant areas have no dynamic caustics. Strong convergence uses a minimum receiving area to limit singular brightness, combined with spatial filtering; this is not a strictly conservative finite-light-source solution, and undersampled folds may still flicker. Higher beam resolution or a smaller coverage region can improve detail.

Flat-bed/sand-slope refraction intersects the seabed plane from the current wave surface along the Snell direction and projects the intersection back into scene color. Sampling occurs only when the scene-depth-reconstructed position matches the receiving surface; offscreen positions, foreground occlusion, no intersection, or exceeding maximum path length all fall back to original straight sampling to prevent stretching land/obstacle colors. On success, underwater path length and scattering integration direction are updated together; within four pixels of the screen edge the fallback result is smoothly blended in. This transition is an appearance approximation to remove screen-boundary seams. This is screen-space color reuse and does not include offscreen scenes, model triangle tracing, a submerged camera, or total internal reflection.

Reef caustics reuse the uploaded heightfield relative to sea level, building a min/max pyramid with successive 2×2 merges (logically a regular quadtree). The leaf level contains conservative height ranges for bilinear sampling neighborhoods; rays skip nonintersecting blocks from near to far and find the nearest intersection with the bilinear surface inside candidate cells. The pyramid updates only after model transforms, water level, region, supplemental seabed, or grid rebuilding changes; normal and paused frames reuse it. Detail remains limited by uploaded SWE bed resolution and cannot recover rock details filtered out by it. Each beam visits at most 256 nodes and is discarded if the budget is exhausted; receiver triangles crossing steep steps or deviating internally from the terrain are rejected.

Scanned models retain their original unlit color and baked shadows; water surface compositing adds only focused brightening above the flat-water reference irradiance. Actual visible intersections must be close to the heightfield and underwater, avoiding painting caustics beneath overhangs; this is an appearance approximation, not physical relighting of scanned materials. Caustics diagnostics display underwater receiver irradiance, while above-water scanned models retain their original materials.

Model refraction builds a screen-depth min/max mip chain after the opaque prepass and searches visible surfaces from coarse to fine along the projected refracted ray. Intersection tolerance varies with pixel world-space size to avoid stripes from repeatedly switching between hit and fallback across discrete depths; path length is obtained by projecting reconstructed world coordinates along the ray, reducing precision loss from distant nonlinear depth. It is built every frame only when model refraction is enabled; resources are rebuilt when the window size changes. At most 128 nodes are visited; offscreen surfaces, surfaces hidden behind occluders, paths beyond the limit, or exhausted query budgets fall back to original sampling. On a hit, underwater path length, scattering direction, and caustics receiver position are updated together; depth differences are first reconstructed in a consistent coordinate system, rather than subtracting hardware depth directly. Screen space contains only the foremost opaque layer per pixel, so it still cannot recover seabed hidden by foreground objects.

Each Tab has a diagnostic entry sharing the same debug view as “Water and sky → Optics diagnostics”:

- 12: Incident caustic light, linear RGB, without multiplying by sand color or viewing-path extinction; requires caustics enabled and is black when disabled. Display values may exceed 1.
- 13: Underwater ray direction actually used for integration, with components mapped from -1–1 to 0–1.
- 14: Refraction hits in green, fallback in red, and gray when refraction is disabled.

The nearest heightfield intersection can handle underwater rays blocked by reef ridges; rock shadows before water entry, wave-surface self-occlusion, independent collider occlusion, offshore foam light blocking, volumetric caustics, and dispersion are still not calculated. A single-layer heightfield does not support caves, overhangs, and complete vertical rock walls. Density, wet sand, and the fluid solver are unaffected by the two switches. Fix camera, FFT seed, time, gain, SWE order, and wavelet normal parameters when comparing. `--verify-optics` checks production-path flat-bed/sand-slope irradiance, beam resolution, sun direction, Snell path length, fallback, size rebuilding, FFT animation/pause, first-/second-order SWE rebinding, and dry-sand protection, as well as nearest intersections with narrow heightfield ridges/complex beds, empty-space skipping, real reef screen refraction, water level changes, and paused reuse; it also checks zero-distortion fast refraction, valid reef sampling, and mode switching.
