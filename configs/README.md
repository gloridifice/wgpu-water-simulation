# Repository configuration and reef front experiment

## Configuration files

- `ocean-settings.json`: the current session and named configuration library copied in full from the original personal file, including “Common” and “Reef”, tracked in Git. The original remains in the user directory and was not rewritten by the experiment. When starting from the repository root, this file is the default manual save target; parameter adjustments become Git changes only after toolbar Save or Ctrl+S. Stopping editing, losing focus, and exiting no longer save the session. It is not a permanently frozen backup and does not automatically synchronize with the old user-directory file.
- `experiments/reef-front-ridge.json`: derived from the named “Reef” configuration, not the top-level current camera state. Retains the original FFT, terrain, water level, camera, second-order water/foam transport, drag, decay, colors, and material; enables the compression ridge with 1 m bandwidth and relative prominence 0.02, and sets background multiplier to zero. Spray particles and region wireframe are disabled and the simulation unpaused to isolate waterborne foam.

The default path depends on the **startup working directory**, not an absolute path on the developer's machine. Without `configs/ocean-settings.json`, it falls back to the original user directory; `--settings PATH` explicitly selects a writable target. Corrupt-configuration backups and atomic-write temporary files are not tracked by Git.

The built-in read-only configuration is displayed as “Default”. Existing user-defined names, including Unicode names or a previously saved profile named “Default”, are preserved on load; new or renamed profiles cannot use the built-in name. The bundled profiles use English display names; parameter values, profile indices, and the configuration schema are unchanged.

`--config PATH` loads the top-level current session read-only, retaining camera and offshore foam parameters. It does not select named library entries or save automatically; missing or invalid files produce errors. Parameters can still be adjusted temporarily in the window but are not written back on exit. Do not add `--model` or `--beach`, which override model placement and camera in this configuration.

```powershell
# Restore and continue saving your current working state.
cargo run --release

# Preview the experiment read-only; exit automatically without changing settings.
cargo run --release --bin ocean -- --config configs/experiments/reef-front-ridge.json --frames 600

# To save manual experiment adjustments to the experiment file, explicitly select a writable target.
cargo run --release --bin ocean -- --settings configs/experiments/reef-front-ridge.json
```

## Fixed-condition comparison

This comparison switches only between the old and new front sources, both with zero background multiplier; it does not directly compare all old background sources against the new front. Fixed conditions: seed 42, FFT 128², 128 m domain, wave-height gain 0.3, SWE 256², second-order SWE/foam, original “Reef” camera. Snapshots use simulation time, unaffected by saved pause/playback speed; output files must not already exist.

```powershell
cargo build --release --bin ocean
$exe = "target/release/ocean.exe"
$cfg = "configs/experiments/reef-front-ridge.json"

# Actual emission rate in the latest substep: black is zero, white is full front generation rate.
& $exe --config $cfg --time 12 --shore-view 7 --snapshot target/reef-ridge-source-12.png
& $exe --config $cfg --no-foam-ridges --time 12 --shore-view 7 --snapshot target/reef-broad-source-12.png

# Final shading at the same simulation time. Use --shore-view 6 to exclude material effects.
& $exe --config $cfg --time 12 --snapshot target/reef-ridge-shaded-12.png
& $exe --config $cfg --no-foam-ridges --time 12 --snapshot target/reef-broad-shaded-12.png

# Emit only in (8,9] seconds to observe transport and decay after emission stops.
& $exe --config $cfg --foam-emission-window 8,9 --time 9 --snapshot target/reef-pulse-9.png
& $exe --config $cfg --foam-emission-window 8,9 --time 12 --snapshot target/reef-pulse-12.png
& $exe --config $cfg --foam-emission-window 8,9 --time 16 --snapshot target/reef-pulse-16.png
```

The time window toggles emission on warmup frames of approximately 1/60 s; it does not automatically track the same wavefront. The window must lie within the snapshot's most recent 60 seconds of warmup; existing foam is not cleared when emission stops. The production substeps still integrate emission rate over simulation time. `--shore-view 8` shows peak weight, not transported density; weight can be nonzero while emission is stopped, but the actual source is zero. Pausing retains the most recent substep's diagnostics.

## Observed results and limitations

On RTX 3070 / Vulkan, under the fixed configuration and times above, the complete 65,536-cell field was read, not white pixels counted from screenshots. Emitting cells use a generation-rate threshold above 0.001/s; foam cells use an area-density threshold above 0.02.

| Case | Time | Emitting cells | Foam cells | Maximum foam density |
| --- | ---: | ---: | ---: | ---: |
| Old front source, continuous emission | 12 s | 5167 | 10368 | 17.650 |
| Compression ridge, continuous emission | 12 s | 1186 | 6316 | 7.147 |
| Compression ridge, emission only at 8–9 s | 9 s | 1027 | 2824 | 5.633 |
| Same, after emission stops | 12 s | 0 | 2233 | 1.192 |
| Same, after emission stops | 16 s | 0 | 823 | 0.230 |

The actual generation region narrowed, and the large white band against the rocks became more scattered patches and short streaks. Foam state persists after emission stops; a single pulse fades quickly with the original decay rate of 0.45/s and material. **These results do not yet show the connected net-like structure of the reference image and do not establish that the morphology problem is solved.** Opacity and decay were not changed to increase visibility.

Current limitations:

- Local flow direction only approximates the wavefront normal and may be inaccurate in flow around reefs or crossing waves; near-stationary cells do not emit.
- The five-point local fit supports an effective narrow band only one or two cells wide and depends on the grid; higher resolution does not retain the same physical bandwidth.
- There is no wavefront tracking, temporal hysteresis, or automatic single emission. A stationary strong compression peak can continuously produce foam.
- No viscosity, cohesion, or fine-scale turbulence was added, nor was a prefabricated net texture substituted for dynamics. The independent FFT/J diagnostic tool was unchanged.

Full `ocean --verify` passed, including added real-GPU checks: analytic compression-peak tangential continuity, uniform compression not treated as a peak, rectangular grids and rotation, expansion/dry-region protection, one injection per substep at both orders, source terms not changing water, and conservative downstream transport of density above 1 after emission stops. CPU library tests, Clippy, and a CPU build without ocean also passed. A 900-frame read-only window was also run, and window screenshots were used to check reef rendering and visible new controls; not every control was operated individually. After the window exited, SHA-256 hashes of both repository configurations and the original user file were unchanged. Generated images and logs remain in `target/` and are not tracked by Git.
