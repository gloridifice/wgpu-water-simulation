# Baked foam NSH assets

This directory provides three comparable foam-detail datasets with the same layout. Simulation density, momentum, and dual-phase displacement remain products of the existing nearshore simulation; these images only affect appearance.

| Runtime image | Original input | Input interpretation |
| --- | --- | --- |
| `baked/shorewaves_nsh.png` | `../shorewaves/voronoi_lace.png` | Linear grayscale, matching the original project's loading convention |
| `baked/crest_foam_nsh.png` | `crest/foam.png` | First channel after sRGB decoding; original 512×512 RGBA |
| `baked/crest_foam2_nsh.png` | `crest/Foam2.png` | First channel after sRGB decoding; original 450×450 grayscale |

All three default outputs are 1024×1024, 8-bit RGBA linear data without sRGB/gAMA tags or premultiplied alpha. Smaller originals are enlarged with periodic bilinear interpolation, standardizing sample size without adding source pattern detail; downsampling uses area filtering so fine patterns do not lose coverage through point sampling.

## Channel conventions

- R, G: tangent normal X/Y, each encoded from -1–1 to 0–1. Local Y points in the direction of increasing UV V; local Z points outward from the surface. At render time transform these into the world XZ water-surface basis using actual flow coordinates.
- B: Soft, a smooth distribution derived from the input mask with an adjustable curve, not final opacity.
- A: Height, relative relief designed from the same shape, not measured geometric height or water depth in meters.

Height is obtained from a power curve of Soft, then blurred with a separable periodic box filter. The normal uses periodic central differences of that height:

```text
normal = normalize(-height derivative along U × relative height,
                   -height derivative along V × relative height,
                   1)
```

Relative height is the ratio of height to the full texture side length, default 0.004. It defines micro-normal slope without directly raising render vertices. This is an artistic approximation, not a way to recover real foam thickness from grayscale. Normal, Soft, and Height share an input, but this is not a new bubble-size or lace morphology generator.

## Rebaking

Run from the repository root:

```powershell
cargo run --features ocean --bin foam-bake
cargo test --features ocean --bin foam-bake
```

By default the tool updates the three runtime assets above and their `.recipe.json` files. Each recipe records input path, original dimensions, color space, algorithm version, FNV-1a content fingerprint, and all parameters; third-party original SHA-256 hashes are in [SOURCES.md](SOURCES.md). By default it downloads nothing and does not change the simulation or personal settings.

The baked PNGs are versioned runtime assets embedded with `include_bytes!`; overwrite them directly when rebaking. Do not delete them before building the tool, or the main library will fail to compile due to missing embedded files. Fresh clones contain them and can run the commands directly.

Custom input example:

```powershell
cargo run --features ocean --bin foam-bake -- --input assets/shorewaves/voronoi_lace.png --output target/foam-test/nsh.png --input-space linear --height-gamma 1.35 --blur-radius 2 --normal-height 0.004
cargo run --features ocean --bin foam-bake -- --help
```

`--input-space` must match the source convention. The tool interprets the first channel according to explicit parameters; it does not automatically treat display brightness as physical height or use source alpha as the mask. Retain sRGB decoding when comparing the original Crest effect; if deliberately interpreting it as linear, save a separate recipe and retune the coverage curve.

`--size` supports powers of two from 32–4096; `--soft-gamma` and `--height-gamma` support 0.1–8; `--blur-radius` is the blur radius in output pixels, range 0–32; `--normal-height` supports 0–0.1. All parameters are checked for finiteness. Default batch processing preserves input intensity ranges rather than automatically normalizing each image, avoiding implicit coverage changes in comparisons.

## Preview and comparison

Previews default to `target/foam-bake-preview/` and are not versioned; use `--preview-dir` to change the directory or `--no-preview` to disable them. Each `_preview.png` has two rows of four:

1. Top: Soft, Height, decoded normal RGB, packed RGB (preview does not use alpha transparency).
2. Bottom: illustrative coverage at display densities 0.2 / 0.5 / 0.8, followed by a 3×3 Soft tile.

Preview coverage uses an explicit smooth threshold curve for comparing input shapes only; it does not claim to match all runtime lighting. It does not modify simulated foam density. Periodic resampling, blur, and differentiation do not clamp image edges; obvious seams in the input itself still require reworking the shape. The tool does not promise to remove all seams automatically.

When generating runtime mips, decode RG to a 3D normal, average, normalize, and re-encode; filter B/A separately as linear scalars. Do not treat the full RGB as an ordinary normal or overwrite A with 255.

See [SOURCES.md](SOURCES.md) for sources and licenses; Crest-derived PNGs continue to include `crest/LICENSE.txt`, and Shorewaves-derived PNGs continue to include `../shorewaves/LICENSE.txt`.
