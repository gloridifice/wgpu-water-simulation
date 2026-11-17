<!-- doco:lifecycle v=1 created-at=2026-10-29T04:22:36Z completed-at=2026-10-29T06:25:42Z archived-at=2026-10-29T06:25:50Z -->
# Reduce Repeated Computation in Second-Order SWE

## Purpose

The water MUSCL reconstruction currently runs separately for four faces, with the center reconstructed again for the bed source; model cell-center bed queries repeatedly transform world coordinates and sample bilinearly; FFT incident waves are also read outside the input band. In a fixed-input RTX 3070 / Vulkan benchmark, 256² second-order water with first-order foam took approximately 0.594, 0.603–0.613 and 1.505–1.510 ms per full substep for the beach, synthetic model and eight-obstacle scenes respectively, substantially more than the cost of merely adding a second stage.

Reduce actual GPU cost at the same grid and time step without sacrificing precision, reducing simulation advancement or disabling existing foam/spray effects. The user requested avoiding excessive defensive design: reuse data directly rather than building a generic optimization framework or duplicating boundary protection.

## Scope and acceptance

### Scope

The first three kernel optimizations were delivered: reuse five-cell neighborhoods and reconstructions by axis for water, directly index model cell-center bed heights, and sample incident waves only in the FFT relaxation band during normal advancement. Fixed-input benchmarks cover dual-concentration model foam, emission and nonzero FFT input; production GPU verification checks numerical behavior.

The user requested committing the current changes before trying subsequent optimizations individually. The first batch was committed as 055172c (including the user's personal configuration changes). The second round independently experiments with reuse of dual-concentration backtracing and removal of predictor outputs with no consumers; retain them only with repeatable gains and passing behavior checks. Full state retains its original five-block layout, without expanding into state rotation or adaptive grids.

Retain MUSCL + SSP-RK2, the original first-order path, limiter and wet/dry order fallback, bed source and pressure correction, CFL=0.2, the 12 m/s velocity constraint and FFT → SWE direction. Configuration, defaults, state layout and stage count remain unchanged; spray is still measured once per completed substep. The third round may change only internal bindings/copies and parameter passing under those numerical constraints.

The user subsequently requested continuing until every candidate had been evaluated. The third round adds offscreen production-path profiling with read-only configuration and evaluates local water surface index reuse, alternating substep buffer roles to reduce copies, uniform scheduling and dry-region culling in turn; retain or withdraw each after A/B comparison. Buffer roles may alternate within a frame, but public field and previous remain synchronized at frame end so rendering, collider editing and existing library consumers are unchanged; spray sources bind according to current/initial roles.

Do not change numerical precision, CFL, default effects or materials. Do not implement shared-memory tiles, intermediate flux buffers, a generic cache framework, UI or adaptive grids. Evaluate quadtree applicability only; do not force a solver rewrite unless the current scale and sparsity gains justify it.

### Acceptance

1. Use fixed inputs at 256²/512² with consistent time steps and device; repeatedly sample GPU timestamps after warmup and report medians and cost per simulated second. Achieve repeatable gains in at least the second-order model or obstacle path; do not claim noise-level differences or different substep counts as speedups.
2. Compare outputs across versions for water depth, momentum, single/dual-concentration foam, wetness and texture state. Use existing production GPU verification to protect still water, conservation, nonnegative depth, wet/dry behavior, wave propagation, FFT initialization/input, collider editing and the absence of spray feedback; do not relax existing thresholds to accept an optimization.
3. Integer bed reads apply only at solver grid centers; rendering samples at arbitrary world coordinates retain interpolation. Noninteger model coordinates, rectangular domains and open boundaries work.
4. Reset still initializes the entire domain from the current FFT; normal stages only omit incident-wave calculations at zero weight, without changing water level/matching momentum, gain, safety limits or time interpolation.
5. Add no runtime GPU readback, synchronous waits, new configuration or writes to personal settings; preserve the independent CPU package boundary. Full GPU verification, library tests, formatting and static checks pass; clearly list anything not verified.
6. Documentation distinguishes fixed-input kernel benchmarks from production SWE time including state copies and spray sources; do not equate these benchmarks with personal model scene FPS.

## Result

All three rounds of candidate evaluation and verification and all execution tasks were completed; the user explicitly authorized completion and archiving. The first batch was committed, while the second and third rounds remained uncommitted; archiving does not commit or push code. These are existing verification results, not a new runtime run.

In repeated fixed-input RTX 3070 / Vulkan measurements, 256² second-order model single-concentration time fell from about 0.596 to 0.096 ms/substep, and dual-concentration model plus incident waves from about 0.565 to 0.121 ms/substep. Maximum scaled difference across the five single-step state blocks was 2.384e-7; full GPU verification, 64 library tests, formatting and Clippy passed. Numerical thresholds, time step, ABI and scheduling were unchanged; these results are not personal scene FPS and other devices were not verified.

The second round re-established the 055172c baseline on the same device. With both changes combined, 256² second-order dual-concentration model substeps fell from 0.1215–0.1222 to 0.1015–0.1019 ms, and 512² from 0.4109–0.4185 to 0.3257–0.3333 ms. All 30 final states were bit-identical; full GPU verification, 64 library tests, the offline diagnostic entry point and static checks passed. Production scheduling, CFL and state allocation size were unchanged.

The third round retained local water surface index reuse and alternating full-substep roles, and added the read-only ocean-profile tool. In the current model configuration, full SWE including copies and spray sources fell at 256² from about 2.46–2.53 to 2.01–2.02 ms per fixed-time frame, and at 512² from 15.57 to 11.60–11.62 ms; drawing fell respectively from about 3.70–3.72 to 3.53 and from 6.26 to 5.75 ms. Continuously evolved final states were bit-identical, and final pixels with spray disabled were byte-identical. Full GPU verification, 64 ocean library tests, 33 CPU tests and static checks passed.

Parameter-slot caching gains did not justify the added management cost, and the dry-neighborhood shortcut did not demonstrate a net gain; both were withdrawn. Fixed active blocks/quadtree were not implemented because the currently cullable area is limited and extra scheduling/numerical boundary handling would be required. Defaults, precision, CFL, sources and materials were retained; performance was confirmed only for RTX 3070 / Vulkan offscreen fixed-time runs, not window FPS, and does not imply there is no room for further optimization.
