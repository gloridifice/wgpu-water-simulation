# Implementation Design: Procedural Lake Animation

This document is the implementation basis; see [tasks.md](tasks.md) for implementation and verification status. Unchecked acceptance items are not complete. See the [proposal](../proposal.md) for requirements and acceptance.

## Baseline and goals

At creation `git status --short` was empty, with no existing workspace changes to merge. Current facts come from the [architecture](../../../../architecture.md), [UI contract](../../../../specs/workspace-ui.md) and the following source (links relative to this file; new files explicitly marked as planned paths):

- [scene.rs](../../../../../src/ocean/ui/panels/scene.rs) provides only beach/rocky shore presets; the local simulation entry depends on `OpticsConfig::swe_enabled()`.
- [optics.rs](../../../../../src/ocean/parameters/optics.rs) aggregates model, SWE and optical parameters and uses serde defaults for missing fields; it has no LakeConfig yet.
- `surface()` in [surface.wgsl](../../../../../src/ocean/render/shaders/surface.wgsl) combines FFT and Shore heights, with horizontal displacement from FFT; optics also reuse surface sampling.
- HeightField in [terrain.rs](../../../../../src/ocean/scene/terrain.rs) supplies single-layer heights for model baking/resampling, not an existing lake SDF system.
- [workspace/mod.rs](../../../../../src/ocean/ui/workspace/mod.rs) manages ocean panel registration and dependencies; `Panel::Reflection` is SSR, not wave-motion reflection.
- SavedSettings in [session.rs](../../../../../src/ocean/settings/session.rs) already stores optics, camera, time scale and pause; named configurations reuse parameter snapshots.
- Window scheduling lives in [viewer.rs](../../../../../src/ocean/app/viewer.rs), and offscreen advancement in [snapshot.rs](../../../../../src/ocean/app/snapshot.rs); both paths must be integrated, not just the interactive window.

Do not change common's business boundaries or introduce GPU dependencies into the CPU package.

## Overall approach

Proposed additions (all planned APIs, not existing interfaces):

- [parameters/lake.rs](../../../../../src/ocean/parameters/lake.rs): LakeConfig, PropagationMode::{Depth,Sdf} and validation. Aggregate via a default-disabled lake field in OpticsConfig; when lake is enabled, prefer the lake path and retain but do not use the original beach/model/shore values. Original scene presets explicitly disable lake.
- [scene/lake.rs](../../../../../src/ocean/scene/lake.rs): LakeScene, owning deterministic shoreline, bed, rock mesh, still-water depth, wet/dry mask and CPU SDF data; generate once and upload. Do not borrow SWE buffers as animation dependencies.
- [simulation/animated_lake/](../../../../../src/ocean/simulation/animated_lake/): AnimatedLake, owning an f64 simulation clock, fixed-seed PRNG, event pool and GPU publication resources; place WGSL in its shaders directory.
- [ui/panels/lake.rs](../../../../../src/ocean/ui/panels/lake.rs): draft/application controls and emit/clear requests; do not advance simulation directly while drawing the panel.
- Renderer owns lake drawing resources and AnimatedLake; window and snapshot invoke the same business update entry point. Keep the ocean renderer path's layout and behavior; a separate lake pipeline avoids unrelated fields in every SWE shader.

Proposed responsibilities: `LakeScene::build(&LakeConfig) -> Result<Self, Error>`; `AnimatedLake::advance(dt: f64, commands: &[LakeCommand])`; `AnimatedLake::encode(...)` publishes the wave field; `reset(seed)` resets the clock and sequence. Rust arguments and resource wrappers may be adjusted to the existing Renderer borrowing structure during implementation, but responsibilities must not move into common.

## Algorithms and rules

### Procedural scene

1. Generate a periodically continuous low-frequency noise radius R(angle) in polar coordinates about the lake center. Multiply a positive base radius by bounded perturbation and limit amplitude to keep R positive. Each ray from the center has only one outer shore point, avoiding self-intersections.
2. Generate a smooth bowl-shaped bed from normalized radial distance; add bounded low-amplitude noise, keeping the outer shore predictable and avoiding unintended external puddles. Static water elevation is zero.
3. Place deformed ellipsoid/low-poly rocks using an independent random sequence derived from the scene seed. Bound rock count and size and retain sufficient open water in the default scene; use a fixed number of attempts on sampling failure, report actual placement count and never retry indefinitely.
4. Combine rock top heights and lake bed into a single-layer bed. Rock render geometry, waterline footprints and height samples come from the same generated data, allowing grid-resolution-scale error but no unrelated disk collision proxies. No overhangs initially.
5. Still-water depth is d=max(0,-bed_height). Exposed rocks and the exterior are dry; fully submerged rocks change only d.
6. Compute two-dimensional Euclidean signed distance D to the wet/dry boundary on a shared world XZ grid, positive in water and negative in dry areas, including outer shores and exposed-rock holes. Use a deterministic distance transform and convert grid spacing to meters; boundary error is grid-scale. This is not Manhattan distance, and radial distance must not be called SDF.

Default grid 256², initial choices 128²/256²/512², at most 32 rocks. Rebuild on scene application; do not recompute the distance field during normal frames. On build failure retain the old scene, parameters and active waves and display an error; on success swap resources atomically and reset waves. Size, seed, resolution and rock layout all require explicit application.

## Two propagation coordinates

Use a common “positive distance remaining from the boundary”, decreasing for incidence and increasing for reflection:

- Depth mode: C=d/d_max, where d_max is the scene's effective maximum depth.
- SDF mode: C=max(D,0)/D_max, where D_max is the maximum positive distance in the scene's water.

C is dimensionless and 0 is the respective boundary. Label UI speed “propagation progress/second” and wave width “propagation coordinate width”; do not claim horizontal meters/second or constant world-space width. Give both modes independent starting coordinate, speed, width and reflection coordinate settings; switching must not reuse raw depth values with different unit meanings. Optional depth/shore-distance readouts aid understanding.

Validate 0 < reflection < start < 1, positive finite speed and width, and bounded maxima; defaults avoid C=0 and C=1. Depth plateaus and the SDF medial axis change contour topology, so a single arc is not guaranteed. Smoothly attenuate display in low-gradient regions and use bounded gradient normals; do not divide by a near-zero gradient to claim constant world speed. Debug views retain the original C rather than showing only a smoothed idealization.

Local SDF contours can surround rocks, producing multiple wave bands within a fixed lake-center sector and merging at the medial axis. Accept this procedural appearance; do not add implicit geodesic distance, per-point ray tracing or connected-wavefront tracking. Depth mode can also include rocks, but represents them only in bed depth. These are two appearance choices, not a binary distinction of rock support.

## Waveform and direction

Each wave fixes its lake-center reference, world direction angle and angular half-width. Use atan2 or a dot product to construct the periodic shortest angular difference, not direct angle subtraction; smoothly fade near the lake center to avoid undefined direction.

The angular envelope is 1 at the center and smoothly reaches 0 at the ends, modulating both amplitude and propagation-coordinate width; the tip width ratio is strictly positive. A locally compactly supported crest/trough curve along C-front produces height, with continuous edge function and first derivative. Reflection inherits the sector without reversing its angular direction. Following the user's latest request, this design uses angular range and no longer promises a fixed world-space lateral arc length.

The mask always suppresses dry land and exposed-rock interiors; smoothly limit amplitude in the boundary depth band to prevent troughs penetrating the bed. This is display clipping, not volume conservation. Water remains visible above submerged rocks.

## Time, events and reflection

- The CPU f64 clock advances with simulation dt driven by toolbar pause/time scale, consistently for window and snapshots; GPU f32 time is relative to a local origin, periodically rebasing current time and all birth times together.
- Each emission freezes direction, angular width, waveform, speed, reflection ratio and decay parameters. Edits affect only subsequent incident waves, not existing waves or their reflected descendants.
- Incident front=start-speed*age. Exact shore arrival is birth+(start-reflection)/speed; event traversal handles `(previous_time,current_time]` and records a triggered flag.
- Generate the reflected record at that exact time, front=reflection+speed*reflected_age; hold the incident crest at the threshold and fade it out over the transition while fading in reflection at the same position, multiplied by an amplitude ratio below 1 and exponentially decayed. Continuous weights avoid a height jump; perfectly matched derivatives are not required.
- Reflected records cannot reflect again and disappear smoothly upon reaching the effective upper coordinate bound or end of life. Natural expiry, clearing, scene replacement and full-pool rejection are not shore-arrival events.
- Fixed total capacity is 128; process incident and reflected waves by event time and stable ID. Reserve one reflection slot when emitting a parent (only if reflection is enabled), ensuring its descendant cannot be lost to capacity contention on arrival. Reject a new incident wave and increment a visible dropped count if insufficient slots remain; never overwrite visible waves.
- Paused users may manually emit and display the birth state, but automatic advancement/emission does not repeat. Clearing removes pending reflections and reservations and resets the next automatic emission time; do not make up old backlog next frame.
- Process all normal dt; minimum emission interval and pool capacity bound event cost. Automatic births derive time and random values from integer sequence numbers so dt partitioning does not change the sequence. A per-frame event budget must not silently change normal playback results.

## APIs and data model

Each published record uses five vec4<f32>, 80 bytes total:

1. direction.xy, birth, lifetime.
2. start_coordinate, signed_speed, coordinate_width, amplitude_m.
3. center.xz, angular_half_width_rad, tip_width_ratio.
4. fade_in_s, fade_out_s, decay_half_life_s, phase.
5. front_min, front_max, hold_at_end_s, reserved zero.

The CPU additionally stores stable ID, reflection reservation/trigger state and frozen parameters; do not upload the CPU management structure directly. Rust uses matching five [f32;4] values; check length, bindings, alignment and WGSL on a real GPU. If the initial single waveform does not need phase, fix it to zero rather than adding a useless UI control.

Initial compute composes height on the lake grid, then computes consistent slopes from the final masked height and outputs height/gradient textures shared by rendering and optics; do not traverse 128 waves per screen pixel. Separate two-stage reads and writes avoid neighborhood access to a texture being written. Publish wetness/bed/propagation-coordinate textures for clipping and diagnostics.

Lake drawing covers only the local grid, disabling infinite ocean tiling. Reuse water color, sky, basic Fresnel and HDR; SSR and fast screen refraction use actual lake normals and the procedural opaque scene. Initially disable existing physical caustics and physical refraction that depend on ocean bed tracing in lake mode (show the reason and retain parameters); do not silently read FFT/SWE or automatically change “physical” to “fast”. Later support is outside this change. Procedural bed and rocks enter opaque scene depth and color.

Any new timing category uses asynchronous GPU timestamps only; show unavailable when unsupported. Record resolution, active wave count and GPU time, measuring both default scene and full pool without promising FPS in advance.

## UI, configuration and compatibility

Add a “Lake Scene” button and closable “Lake Animation” Dock panel under Scene, with that tab in the default layout. Do not forcibly overwrite existing layouts; the top panel menu can add it, and the lake scene entry provides an action to open it. Continue the five-category and temporary-hide restoration contract.

Panel groups: terrain/rock drafts; depth | SDF selection and application/reset; emission direction angle/angular half-width/interval/random angular deviation; waveform; reflection; emit one/clear; field diagnostics and active/dropped counts. Each group shares a two-column grid using yummy_egui, with tooltips for units, ranges and application timing. Separate scene generation seed from wave random seed.

Mode switching changes only a draft; reset history only after successful application, and roll back resources and current mode on failure. Retain draft values for existing modes. Applying while paused remains paused. Switching workspaces resets no waves. Store parameters, seeds and selected mode in OpticsConfig::lake, defaulting missing fields to disabled; do not save active waves, time or counts. Loading lake configuration rebuilds from initial state; UI must make clear it is not a playback-progress save. Named configuration, Ctrl+S, no-settings and read-only rules remain unchanged.

## Verification and documentation impact

Verify independent behavioral expectations rather than padding tests with field getters or source-string assertions. Read the test-audit skill before implementing tests.

- CPU: deterministic scene; analytic distance reference and discrete tolerance for a regular circular shore/single rock; submerged rocks in depth; angle wraparound; event time partition consistency, single reflection, capacity reservation, pause/clear; old/new configuration round trips and invalid-value rejection.
- Real GPU: field sign and mask, before/after wavefront handoff, direction not jumping to the opposite shore, differences between modes, finite normals consistent with height differences; still water with zero waves; high-wave-count boundaries; original ocean rendering regression. Window/snapshot yield consistent state at the same time and seed.
- Real window: entry, depth/SDF application, rocks, paused emission, read-only saving, Dock restoration and unsupported optical status. Fix camera/time/seed for visual comparison.
- At delivery update architecture.md, guides/ocean.md and specs/workspace-ui.md, and add a lake behavior specification reflecting actual delivery. Do not modify these current facts during creation.

## Fixed decisions and discretion

No blocking open questions. Implementers may adjust grid/mesh details, noise implementation and default artistic values within constraints; they must not unilaterally change the SDF definition, angular envelope, reflection count, time or save semantics, or expand this into physical propagation. If appearance at the medial axis is unacceptable, report the boundary and discuss separately rather than quietly adding a new propagation algorithm to this change.
