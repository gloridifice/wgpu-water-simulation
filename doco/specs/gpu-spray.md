# Nearshore GPU Spray

## Scope

A textured splash effect driven by nearshore SWE, separate from surface foam and beach wetness. It does not solve three-dimensional fluid, overturning sheets, offshore spray, or particle feedback. Emission occurs only in the pure SWE region; the FFT blend zone is not an emission region in the initial version.

## GPU signals and consumption

- Each SWE cell has a 32-byte signal: two four-component vectors containing world position weighted by expected particle count and total count, followed by weighted initial velocity and a reserved zero.
- Expected count is emission density (particles per square meter per simulated second) multiplied by cell area, actual substep duration, and independent slope, positive rise, and effective water depth gates. Accumulated foam density is not used as an emission signal.
- Each completed SWE substep produces one signal; the second-order prediction stage is not counted separately. Signals are cleared and reintegrated only on valid simulation frames. Pausing preserves the debug signal, but the consumption timestep is zero.
- The particle system reads the storage buffer and maintains fractional remainders and birth sequence numbers on the GPU; it does not multiply by area or time again. When unsaturated and birth positions remain valid, the difference between cumulative integer emissions and the integral per cell is less than one particle, subject to floating-point error.
- Emission signals, particle state, and active counts must not be read back to the CPU to decide runtime emission, updates, or drawing. The CPU only uploads parameters, manages resource lifetimes, and schedules passes in order. Standalone verification may read back at the end.

## Lifecycle and rendering

- Capacity is fixed at 65,536 particles. Each frame first updates and compacts survivors, then fills vacant slots. At capacity, new integer requests are discarded; living particles are not overwritten and no backlog is carried forward.
- Particles are driven only by initial velocity, gravity, and stationary air resistance. Age advances by actual simulation time; particles are removed on water or ground contact, leaving the domain, expiry, or non-finite position. The visual elevation of surface foam does not affect particle motion.
- Pausing stops emission, motion, and aging; disabling releases source and particle resources. Time reset, domain/grid/spectrum rebuilds, and collider edits clear old state.
- An external Splash texture with documented provenance and license is used. Each particle randomly selects one of four Alpha layers for its lifetime; each layer has its own transparent border and mip chain. Texture selection does not affect emission or motion; debug markers do not depend on art textures.
- The GPU uniformly samples a configurable multiplier range for each new particle and multiplies it by the base diameter (0.01–2 meters). Both multiplier bounds are 0–2; the lower bound must not exceed the upper, and equal bounds give a fixed size. The default is 0.65–1.35, compatible with old configurations. Edits affect only subsequent births and do not reset existing particles; final diameter may exceed 2 meters.
- The GPU writes indirect draw arguments; quads use premultiplied alpha. Sampling saved scene depth occludes particles and softens intersections; the particle pass does not bind that depth as a writable attachment. Resize rebuilds the relevant bindings.
- The initial version does not sort transparent overlaps or use OIT. Particle collision with real 3D models is limited to the existing heightfield proxy.

## Configuration and failure behavior

`OpticsConfig::spray` is disabled by default. Old files missing the field use defaults while retaining other parameters. Sessions and named configurations save parameters, not particle lists; configuration validation checks finite values and ranges. `--spray` overrides only enabled state; `--spray-debug` overrides enabled and debug state without resetting other saved spray parameters.

If device storage limits or resource creation fail, spray is disabled and an error is reported; CPU emission is not a fallback. The interactive application retains the original ocean mode; a snapshot explicitly requesting spray returns an error on resource failure.

## Acceptance scenarios

- Given still water, dry ground, or a flat return flow, the source is zero. Given a rising steep slope, source amount and weighted world position and initial velocity meet their definitions.
- With the same source intensity split across different timesteps or area inputs, the integral and fractional remainder remain consistent, without duplicate emission during second-order prediction.
- With configured size and random multipliers, actual GPU births fall within the specified range; a nondegenerate interval produces size variation, and equal bounds produce a fixed size.
- With a full pool and continued source input, new particles do not replace survivors. Capacity is reused after expiry without an old integer backlog.
- Pausing, resetting, and disabling/re-enabling do not consume old signals twice. Water and foam states agree with and without spray, for both first and second order.
- Given known particles and a known depth plane, an object in front fully occludes them; coverage fades near the plane and returns away from it. These relationships persist after resizing.

`src/ocean/simulation/spray/verification.rs` performs behavioral verification through the full `ocean --verify`; library tests cover configuration and CLI overrides. Compilation does not substitute for visual inspection.
