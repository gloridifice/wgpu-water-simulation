<!-- doco:lifecycle v=1 created-at=2026-10-28T08:08:07Z completed-at=2026-10-29T06:25:42Z archived-at=2026-10-29T06:25:50Z -->
# GPU Ocean Five-Category Dock Workspaces

## Purpose

GPU Ocean currently uses one Dock layout with parameters scattered on both sides of the scene. The SWE tab mixes solver, foam generation and transport, material and terrain responsibilities; closely related parameters are scattered, and many disabled or editable but ineffective controls remain visible when a mode does not apply.

This change separates navigation from Dock: fixed category buttons on the far left switch independent category-specific Dock workspaces on the right. Users can still redock panels, while parameters are regrouped by their actual effects.

## Scope and acceptance

### Workspaces and parameter ownership

Five top-level categories are fixed in the following order. Buttons are 38×38 pt squares showing only consistently styled SVG icons with Chinese names on hover, outside Dock; unselected backgrounds are transparent and borderless, while selection retains the theme highlight:

| Workspace | Main contents |
| --- | --- |
| Scene | Scene entry, beach terrain, model transform and water level, computation domain, boundaries, colliders |
| Shallow Water | SWE grid and solver, FFT → SWE gain/short-wave filtering/blend depth, nearshore foam generation/transport/decay, wet sand history, spray generation and motion |
| Offshore | FFT spectrum, extent and sampling, wave height and horizontal choppiness, offshore foam generation and decay |
| Shading | Water and sky optics, reflection, foam display/color/material/height, normal detail, sand material, spray size and soft intersections |
| Configurations | Existing configuration selection, creation, saving, renaming and deletion |

Combining top-level entries does not mean combining all parameters into one long list. Each workspace retains multiple Dock panels by responsibility; FFT → SWE coupling belongs explicitly to Shallow Water, not Offshore. Debug entries belong to their feature panels. Playback, time scale, timing and camera operations remain in the permanent toolbar; performance is a general Dock panel available in any workspace, not a new top-level category. The permanent toolbar spans the window, with navigation below it; action buttons align left and FPS, CPU frame processing time and GPU main-frame time align right. In narrow windows the summary moves to a separate right-aligned row; SWE GPU time appears only when SWE is enabled. GPU metrics use asynchronous real timestamps and explicitly show unsupported, failure or pending status rather than presenting CPU time as GPU time.

### Layout and shared state

- Each category has an independent Dock layout. By default parameters are on the left and a large Render View on the right; the parameter side of Scene, Shallow Water, Offshore and Shading has upper and lower areas, while Configurations has one. Keep related tabs in each area rather than stacking all panels in one column. New defaults apply only without a saved layout or on explicit reset, never over existing personal layouts.
- Allow dragging tabs, resizing dividers and adding available panels from other workspaces via “Add Panel”. Retain a single window without floating system windows.
- Each layout retains exactly one non-closable Render View tab; all workspaces share the scene, camera and simulation state.
- Switching categories restores that workspace's last layout without loading a scene preset, changing parameters, clearing history or resetting timing or camera; render targets may be rebuilt normally when display size changes.
- Layouts and the active category can be restored across launches; “Reset Current Layout” resets only the current workspace.
- Layout saving is independent of simulation parameters and named configurations; loading or overwriting named configurations does not change workspace layouts. Respect the no-implicit-write constraints of `--no-settings` and read-only `--config`.

### Parameter interaction

- Preserve all existing parameters, valid ranges, defaults, units, hover descriptions and application/reset semantics; spectrum and model drafts still require explicit application.
- Each logical group shares a two-column grid: after spacing, names occupy 42% and controls 58%; long names wrap and controls must not widen columns. Group headings are bold labels at normal text size.
- Hide parameter rows, empty groups and entirely unavailable panels when the current scene/mode does not support them, rather than graying out whole areas. Retain re-enable and invalid-configuration correction entries, necessary status and error messages; hiding does not clear saved values.
- Hidden panels retain their restore positions and do not appear in “Add Panel” while unavailable. Keep the five top-level entries fixed to avoid navigation movement and make enable controls accessible.
- Do not treat read-only performance status as unavailable, hide still-running foam generation/transport parameters when foam display is disabled, or hide sky colors/intensity still affecting background and reflections when sky scattering is disabled.
- Use horizontal mutually exclusive buttons for fixed two- or three-option enums, such as solver order, foam transport order, collider shape, sky integration steps and offshore foam view. Four or more options or dynamic lists normally retain dropdowns/lists; enable switches remain checkboxes. Use short labels and hover help to fit narrow columns without forcing them wider.

### Observable acceptance

1. Five top-level entries always remain outside Dock; switching affects only the right-hand layout, with default parameter ownership as above.
2. Move and resize a panel in Shallow Water, switch to Offshore and back, and recover the Shallow Water layout; restarting restores the last workspace and each layout. Resetting one layout does not affect the other four.
3. Switching, docking and hiding panels do not change simulation parameters, drafts, time, pause state, camera or physical history. The same parameters remain consistent in panels added across workspaces.
4. When filtering, drag or SWE is disabled, or dual-concentration/ridge/material paths are switched, show only actually available parameters; re-enabling restores original values, with error and enable entries still accessible.
5. Small enums have clear mutually exclusive selection states; controls do not overflow or shift column boundaries with small windows, scaling or long Chinese names.
6. Old parameter configurations remain valid without layout data; corrupt layouts do not discard simulation parameters or named configurations. No-save/read-only modes do not create or overwrite personal layout files.
7. Five editable SVG sources remain recognizable at 20/24/32 pixels, with distinct Shallow Water and Offshore semantics; temporary HTML shows actual sizes, selection and sidebar composition. Icons still require user visual confirmation; a preview is not proof of egui integration.

### Non-goals and delivery boundary

Do not change solver, WGSL, simulation or shading algorithms, default parameters, configuration preset semantics or CLI entry points, and do not affect the CPU program without the ocean feature. Do not extend the independent foam-weight-lab tool or add an icon generation service or multi-window management.

The user confirmed the icon preview and authorized the egui refactor and layout persistence, additionally requesting toolbar performance metrics. The preview is not a requirements source; do not add temporary screenshots or generated reference images to version control.

After delivery, update the current architecture, UI specification, usage guide and outdated two-sided layout conventions in [AGENTS.md](../../../../AGENTS.md); do not present the target as implemented during creation.

## Result

Delivered fixed five-category SVG navigation, independent Dock workspaces, parameter regrouping and conditional hiding, a separate layout sidecar and a permanent toolbar performance summary. Workspaces share simulation, drafts and camera; layouts are separate from simulation sessions and named configurations. See the [workspace UI specification](../../../specs/workspace-ui.md) for the current contract and the [Ocean guide](../../../guides/ocean.md#dock-parameter-workspaces) for usage.

The user explicitly confirmed completion and authorized marking all tasks complete and archiving. This review checked existing navigation, workspaces, layout saving and asynchronous GPU timing code and synchronized current documentation; an existing record of the subsequent module reorganization includes a passing default workspace window smoke test and regression checks. Builds, tests and individual GUI interaction acceptance were not rerun for this review; user confirmation and mechanical checks are not presented as new runtime verification.
