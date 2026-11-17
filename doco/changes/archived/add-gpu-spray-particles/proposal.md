<!-- doco:lifecycle v=1 created-at=2026-10-27T03:56:21Z completed-at=2026-10-29T06:25:42Z archived-at=2026-10-29T06:25:50Z -->
# GPU-Driven Nearshore Spray Texture Particles

## Purpose

The existing FFT and nearshore SWE have surface foam but no spray that leaves the water and falls back. This change proposes textured quad particles for that visual effect, without a three-dimensional fluid solver.

The user explicitly requires emission signals to reside in a GPU buffer, which the particle system reads directly for emission without CPU readback. The CPU only uploads configuration and schedules compute and drawing; it must not read back emission signals or active counts to determine runtime behavior.

## Scope and acceptance

### Scope

- Initially generate emission signals from nearshore SWE, stored separately in a fixed-grid-indexed storage buffer; keep signals separate from particle state.
- Integrate substep emission on the GPU, retain fractional remainders, update a fixed-capacity particle pool and generate indirect draw arguments.
- Use textured particles with alpha for breaking spray and droplets, including initial velocity, gravity, drag, lifetime, disappearance on hitting water or ground, and soft intersections with scene depth.
- Provide independent spray parameters and an emission intensity debug view, integrated with existing configuration saving and parameter panels. Supply defaults for old configurations without discarding existing parameters; the initial design disables spray by default until explicitly enabled for preview.
- Preserve FFT → SWE, surface foam transport and beach wetness history; do not use foam display opacity or accumulated density as an emission signal.

### Non-goals

This change does not implement offshore FFT spray, a separate mist layer, a curling wave tongue mesh, SPH/FLIP, particle collisions, exact collision with arbitrary 3D model surfaces, feedback from landing particles into SWE or additional foam, or a replacement for the existing foam material. Offshore may later use the same GPU signal consumption boundary, but this change does not establish a generic effects framework in advance.

### Acceptance

1. Signal generation, particle allocation and indirect drawing all run on the GPU; no CPU readback, synchronous wait or CPU emission list is added for signals, particle state or active counts. A separate verification program may read results back after submission.
2. Specified still water, dry ground and flat uniform backflow inputs emit nothing; a specified steep rising wave front produces finite, nonzero emission. Existing foam residue alone does not trigger continuous spray.
3. Integrate emission by cell area and actual simulated time; do not emit extra particles during the second-order predictor. For different time partitions of the same given source strength, cumulative unsaturated emission differs from the integral by less than one particle per emitting cell, allowing for floating-point error.
4. When capacity is exhausted, discard new requests in a bounded manner without out-of-bounds access, overwriting live particles or accumulating integer emission debt; capacity is reusable after particles expire. Pausing does not consume old signals again; reset, disable and rebuild clear temporary state correctly.
5. With fixed camera, spectrum, random seed and solver order, particles originate near valid water surfaces and fall back without penetrating the beach or forming obvious hard texture edges at the water. Depth sampling remains correct after window resizing. Bit-identical particle order across hardware is not required.
6. Both first- and second-order SWE work; enabling spray does not change water or existing foam state. Old configurations load, particle update/draw is skipped when spray is disabled, and the existing CPU program builds without `ocean`.

### Research basis

- [Real-time Breaking Waves for Shallow Water Simulations](https://cgl.ethz.ch/Downloads/Publications/Papers/2007/Thue07b/Thue07b.pdf): a height field cannot express a curling surface alone and benefits from additional visual elements.
- [Real-time Animation and Rendering of Ocean Whitecaps](https://liris.cnrs.fr/Documents/Liris-5812.pdf): FFT surface deformation whitecap criterion, a reference for future offshore emission rather than this SWE criterion.
- [NVIDIA Soft Particles](https://developer.download.nvidia.com/whitepapers/2007/SDK10/SoftParticles_hi.pdf): depth fading at particle/scene intersections.
- [GPU Gems 3, Chapter 23](https://developer.nvidia.com/gpugems/gpugems3/part-iv-image-effects/chapter-23-high-speed-screen-particles): transparent particle coverage cost; reduced-resolution offscreen rendering is left for future optimization.

These sources inform the design and do not imply that performance or visuals have been verified in this project; the SWE emission rule is a proposed engineering approximation. Check provenance and licensing before introducing any external texture.

## Result

Completed. The user explicitly authorized completion and archiving; the following records existing delivery and verification, not a new runtime verification.

GPU signal integration, the fixed-capacity particle pool, indirect textured drawing, soft intersections, parameter panel and configuration saving were connected. Library tests, the CPU feature boundary, full GPU verification on RTX 3070/Vulkan and a bounded window preview passed. The initial version remains limited to pure SWE regions, unsorted transparency and height-field collision; other devices and actual VRAM exhaustion were not verified.
