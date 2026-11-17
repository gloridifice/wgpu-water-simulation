<!-- doco:lifecycle v=1 created-at=2026-11-11T03:41:22Z completed-at=2026-11-11T06:48:47Z archived-at=- -->
# Procedural Lake Wave Animation and Single Reflection

## Purpose

Provide calm lake animation in the existing GPU Ocean program independently of SWE: ripples advance toward the boundary along propagation-field contours, with thicker centers and thinner ends, and produce a weaker reverse wave that gradually dissipates on reaching the reflection line. The existing surface combines FFT and nearshore SWE and does not yet have this animation mode.

The lake uses fixed-seed noise for an irregular shoreline and places procedural rocks; ripple direction and angular range remain expressed in circular angular coordinates about the lake center. Add switchable depth and SDF propagation modes to compare depth-contour animation with distance-field animation accounting for exposed rock boundaries.

## Scope and acceptance

### Scope and fixed decisions

- Add a “Lake Scene” preset to the existing program, not a separate experiment window. Preserve the original beach and rocky shore scenes and FFT → SWE behavior; lake mode does not advance FFT, SWE, their foam or spray.
- Procedurally generate a simply connected outer shoreline around the lake center, a bowl-shaped bed and several rocks. Use bounded, periodically continuous radial noise for the outer shoreline without self-intersection; rocks may form holes in the water. Seed, size and rock settings can be saved and rebuilt.
- Depth mode reads still-water depth including rock heights; SDF mode reads the two-dimensional signed distance of water from the outer shore and exposed rock footprints, positive in water. Submerged rocks affect depth but are not dry SDF obstacles.
- Both modes share wave instances and the circular angular envelope. Direction angle marks the ripple sector; propagation sign is stored separately. Reflection does not move the sector to the opposite shore. Mode changes are explicitly applied and clear animation history.
- Each wave stores its birth parameters and advances analytically using shared simulation time; reaching the propagation-field reflection threshold generates one weaker reflected wave. A short crossfade prevents a discontinuity; the reflected wave moves toward increasing propagation coordinate and decays without further reflection.
- Reflection is reverse playback of contour animation, not a promise of specular reflection about local normals. SDF is distance to the nearest boundary, not a travel-time field around obstacles; diffraction, energy transport behind occlusion and a single connected wavefront are not promised.
- The CPU manages a bounded wave event pool, and the GPU evaluates waveforms and the surface; normal frames do not synchronously read back GPU state. Pause, time scale, manual emission, clearing and deterministic seeds have explicit semantics.
- Retain the five-category Dock and add a “Lake Animation” panel under Scene; depth/SDF use a mutually exclusive selector, and shore-wave reflection is not mixed with SSR. Reuse applicable existing shading controls and explicitly disable or hide unsupported features.
- Store lake parameters in existing sessions and named configurations, retaining manual-save, read-only and no-save rules; old configurations missing new fields enter the original mode. Active waves are not persisted.

### Non-goals

No SWE or other conservative solver, dynamic water level, fluid feedback, multiple reflections, true refractive/diffractive wave propagation, dynamic obstacles, cave/overhang collision or interconnected lakes; do not change the independent CPU package. Do not present the initial static distance field as a complete rock collision simulation.

### Acceptance

1. The same seed generates the same irregular shoreline, bed and rocks; water does not cover land or exposed rock interiors, and depth and SDF have debug views.
2. Switching modes in a fixed scene reveals different contour shapes; depth includes submerged rocks and SDF includes exposed rock boundaries, with finite field values and correct signs.
3. Ripples retain their world-angle sector, with a thick center and ends smoothly tapering to zero, continuous across 0°/360°. Incident and reflected waves meet at the same position; reflection is weaker, triggers once and does not jump to the opposite shore.
4. Pause does not advance or repeatedly generate waves; different frame-time partitions produce consistent events and wave positions at the same simulation time within numerical tolerance. Clear, expiry and a full pool do not spuriously trigger reflection.
5. Real GPU surface displacement, normals and supported optics read consistently in both modes; unsupported optics paths must not silently read FFT/SWE. Original ocean mode passes relevant regressions.
6. Confirm scene entry, mode application, parameter application semantics, panel hide/restore and five-category layout in a real window; snapshots do not replace UI acceptance. Verify old configuration loading, lake configuration round trips and read-only mode.
7. Update current architecture, Ocean guide and workspace UI contract with only delivered capabilities and verified boundaries.

## Result

Delivered an independent procedural lake mode: deterministic irregular shores, bed and rocks, depth/two-dimensional Euclidean SDF propagation, single incident waves in fixed sectors and one weaker reflection, GPU height/slope composition and local surface rendering. Integrated with the five-category Dock, scene entry, parameter validation and configuration compatibility; the lake does not advance FFT/SWE/foam/spray, physical caustics and physical refraction are explicitly unavailable, and active waves are not persisted.

Acceptance included configuration and event CPU tests, 75 Ocean library tests, real GPU tests of both modes, reflection handoff, angular wraparound, 512-second rebasing, rasterization and original ocean regressions, plus native window mode application, paused wave emission and Dock drag/restart restoration. An independent MCP stdio session additionally confirmed parameter patches, explicit actions, paused fixed stepping and wave-height diagnostic captures. On an RTX 3070, median 256² wave-field composition for 4/128 waves was approximately 0.018/0.102 ms; this does not represent other hardware. Repository-wide formatting, clippy and full GPU verification passed at lake delivery; unrelated Fog formatting/compilation issues in the later workspace are separately listed in the current acceptance record.
