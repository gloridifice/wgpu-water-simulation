# Rectangular Slider Appearance Lab

This independent window adjusts slider appearance; **the interactive preview shares its control with Ocean's parameter panels**. Lab settings affect only this window. It does not run FFT/SWE or read or save personal settings, named configurations, or Dock layouts; lab settings are discarded on exit.

```powershell
cargo run --bin slider-lab
# Limited-frame window smoke test
cargo run --bin slider-lab -- --frames 120
```

## Usage

- Adjust background and bar colors/radii, control height, value box width, end padding, normal bar width/inset, hover bar width/extension, and transition duration on the left. Changes apply immediately.
- The upper right contains real interactive examples with different ranges and both endpoints; below are fixed normal/hover illustrations and a native egui slider comparison. The native comparison shares its value with the first custom slider.
- Click the background to position or drag to adjust; drag or click the value box to enter a value. Once focused with Tab, use arrow keys for fine adjustment. Input and editing use the native egui Slider; no separate Home/End shortcuts are defined.
- Hovering anywhere over the background triggers the hover style, which persists when dragging off the track and returns to normal after release and exit. Hovering over the value box does not trigger track hover.
- “Reset appearance” resets appearance only, not example values. Esc (when not consumed by an editing control) or closing the window exits.

## Current Baseline

The background is a uniform, unoutlined rounded rectangle, not filled according to value, with the same height as the value box. The normal thin bar is inset vertically; on hover/drag it extends vertically, widens, and brightens, without an outline.

| Item | Baseline |
| --- | --- |
| Control height / value box width | Lab initial values 24 / 64 pt; production panels retain the theme minimum height of 22 pt and native padding |
| Left/right internal padding | 2 pt each, measured from the widest bar's outer edge |
| Background / bar radius | 2 pt; production panels use the theme widget radius; thin bars are limited to half their width |
| Background color | `#151515` |
| Normal bar width / vertical inset | 2 / 4.1 pt per side |
| Normal bar color | `#8D8D8D` |
| Hover bar width / vertical extension | 4.8 / 0.5 pt per side |
| Hover bar color | `#DFDFDF` |
| Transition duration | 57 ms |

Colors were sampled from solid areas of tuning screenshots. Points are logical pixels and scale with system DPI. The ends still represent the full minimum and maximum; rows reserve space for the extension, and animation does not change value mapping or row layout. The lab explicitly adjusts height through `.height()`; `.appearance()` overrides only appearance, not production panels or the theme. Production sliders' value boxes and standalone value boxes share theme height and padding. Lab corner radii are independently adjustable (0 is square); colors are freely selectable without automatic hover brightness correction.

The window lives in `src/ocean/labs/slider_lab/`; the shared control is `ParameterSlider` from crates.io `yummy_egui`. The window uses the library's default theme, and this repository does not save theme configuration. The control retains native Slider input, numeric editing, integer/step, clamping, and nonnegative logarithmic scales; it overrides only track and bar drawing, not simulation parameters or configuration format.
