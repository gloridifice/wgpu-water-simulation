# Implementation Tasks

Implementation, automated verification and native desktop acceptance have been performed; the current workspace has separate Fog formatting/compilation issues, detailed in the 4.2 verification record. See [implement.md](implement.md) for design; acceptance numbers refer to [proposal.md](../proposal.md).

- [x] 1.1 Define lake configuration, mode parameters, defaults, finite-number and range validation, and old configuration compatibility.
  Acceptance: proposal 6; old configurations retain the original ocean mode, lake parameters and independent mode settings round-trip, and active waves are not serialized.

- [x] 1.2 Implement deterministic irregular shoreline, lake bed, procedural rocks and shared wet/dry geometry data.
  Dependencies: 1.1
  Acceptance: proposal 1; fixed seeds rebuild consistently, without shoreline self-intersection, infinite placement retries or obvious rock/mask misalignment.

- [x] 1.3 Generate depth and two-dimensional Euclidean SDF with exposed-rock holes, and provide shared coordinate sampling and field diagnostic resources.
  Dependencies: 1.2
  Acceptance: proposal 1, 2; error against regular references stays within grid scale, submerged rocks affect only depth, SDF signs and distance units are correct, and plateau/medial-axis handling remains finite.

- [x] 2.1 Implement a bounded wave event pool, frozen emission parameters, analytic advancement, single reflection and capacity reservation.
  Dependencies: 1.1
  Acceptance: proposal 3, 4; exact shore-arrival time is independent of frame partitioning, sectors do not flip, reflection is weaker and happens once, clear/expiry do not reflect, and pause/full-pool behavior follows the design.

- [x] 2.2 Implement GPU wave records, angular envelope, dual-mode wave-field composition and consistent height/slope publication.
  Dependencies: 1.3, 2.1
  Acceptance: proposal 2, 3, 5; CPU/WGSL layouts agree, angular wraparound and handoff are continuous, with no dry-area water, division by zero, nonfinite normals or normal-frame state readback.

- [x] 2.3 Integrate Renderer, procedural opaque scene, window and snapshot scheduling, isolating ocean and lake resource paths.
  Dependencies: 2.2
  Acceptance: proposal 1, 5; lake does not advance FFT/SWE/foam/spray, has no surrounding infinite sea, and supported optics use lake wave height and normals; physical caustics/refraction are explicitly unavailable in lake mode, while the original ocean remains available.

- [x] 3.1 Integrate scene entry, lake Dock panel, draft application and configuration saving rules.
  Dependencies: 2.3
  Acceptance: proposal 6; retain five categories, depth/SDF require explicit application and reset and retain the old scene on failure, parameter units and application semantics are clear, rocks are configurable, existing Dock layouts are not overwritten and read-only mode does not write.

- [x] 3.2 Add behaviorally valuable CPU and real GPU verification entry points and perform visual comparisons under fixed conditions.
  Dependencies: 3.1
  Acceptance: proposal 1–6; read the test-audit skill first; verify field generation, distance references, single reflection, time partitioning, angle wraparound, configuration compatibility and production shaders. Do not substitute compilation for GPU checks or offscreen images for UI acceptance.

- [x] 4.1 Synchronize implemented architecture, Ocean usage guide and workspace/lake behavior specifications.
  Dependencies: 3.2
  Acceptance: proposal 7; document units and limitations of both propagation fields, rock behavior, single reflection, disabled optics and parameter reset/save boundaries without presenting plans or unverified visuals as delivered facts.

- [x] 4.2 Perform final formatting, static checks, library tests, relevant GPU verification, real UI and performance acceptance; record limitations.
  Dependencies: 4.1
  Acceptance: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --features ocean --lib` and relevant actual GPU verification pass; compare both modes and reflection with fixed seed/camera/time and record default and full-pool GPU times. Limit window frames and use no-settings, do not overwrite personal configuration or terminate user programs. Explicitly list checks that cannot run; do not claim everything passed.

## Verification

- 1.1–1.3: Library tests passed for lake configuration compatibility/round trips/invalid-value rejection, fixed-seed field generation, independent circular-ring plus island distance references (grid tolerance), and submerged rocks affecting depth only.
- 2.1: Behavior tests passed for frame-time partitioning, fixed sectors, weaker descendants, single reflection, pause, full-pool reservation and cancellation on clear.
- 2.2–2.3: Actual GPU compute and raster checks passed on RTX 3070 / Vulkan, including wet/dry and trough constraints, height/slope consistency, mode differences, ±180° continuity, 512-second rebasing, reflection handoff, still water/clear, physical optics isolation, fast refraction/SSR and switching back to the original ocean. Maximum height difference 0.0002 seconds on either side of handoff was approximately 4.8e-5 meters.
- 3.1: Production egui control tests confirmed selecting SDF changes only the draft; explicit application updates the mode and reset request. Existing Dock and configuration saving/read-only tests passed. A dedicated read-only 512² SDF configuration completed snapshots and a 120-frame window run; its SHA-256 was unchanged and no layout sidecar was produced.
- 3.2: `cargo test --features ocean --lib`: 74 passed. Tests follow test-audit's behavior/independent-expectation requirements, with no new test-only production interfaces. Depth/SDF snapshots with scene seed 73, wave seed 42, lake camera and t=7 were read and compared; rocks and shore were visible, ripples were weak, and static screenshots are not claimed as manual confirmation of motion direction.
- 4.1: Synchronized [architecture.md](../../../../architecture.md), [Ocean guide](../../../../guides/ocean.md) and [UI specification](../../../../specs/workspace-ui.md), and added the [lake specification](../../../../specs/animated-lake.md), documenting animation/SDF limitations and unsupported optics.
- 4.2 passed at delivery: `cargo fmt --all --check`; `cargo clippy --workspace --all-targets -- -D warnings`; 74 library tests; release build; targeted `--verify-lake`, `--verify-optics` and full `--verify` (including original FFT/SWE/foam/spray/optics regression); read-only 512² window smoke and snapshots of both modes. The final full GPU log was temporarily retained in the ignored repository-root path target/lake-full-verification-final.log (temporary evidence, not a durable delivery document).
- Performance: 256² wave-field composition (excluding drawing/UI), real GPU timestamps, nine runs after warmup; medians approximately 0.018 ms for 4 active waves and 0.093 ms for 128. Results represent only the local RTX 3070 and promise no frame rate on other hardware.
- Confirmed native UI: a frame-limited, no-settings window run and actual inspection of cutty screenshots confirmed five categories, lake scene entry, shared two-column parameters, depth/SDF selector and disabled save state. Screenshots and temporary configuration were not versioned; the program exited after its frame limit and no user process was terminated.
- 4.2 additional native desktop acceptance: used task-specific `target/lake-ui-acceptance.json` without touching personal configuration. Real window clicks confirmed applying SDF and manually saving; “Emit One” while paused changed active waves from 0 to 1. After dragging “Lake Animation” into the “Scene and Terrain” group, closing this test process and restarting, the sidecar and cutty screenshots confirmed restoration of both tabs and the active tab. Windows were frame-limited; only the test process explicitly started for this task was closed.
- 4.2 additional MCP acceptance: Pi's tool catalog did not expose project tools at the time, so a local MCP SDK established a separate `target/debug/ocean.exe --headless --no-settings` stdio session without modifying `.pi/mcp.json`. After discovering fields and actions using `scene_describe`/`parameters_get`, actually invoked parameter patches, `lake.emit`/`lake.clear`, fixed stepping and captures; explicit stepping remained available while paused. Fixed-seed and camera wave-height diagnostic images were read, confirming depth incidence, weaker reflection in the same sector, SDF arcs and clearing to zero; visual results were not asserted from PNG creation alone.
- 4.2 current recheck: `cargo clippy --features ocean --lib -- -D warnings` passed; `cargo test --features ocean --lib` reported 75 passed (the extra test came from later MCP integration); `target/debug/ocean.exe --verify-lake` passed again, with median times of approximately 0.018/0.102 ms for 4/128 waves. `git diff --check` passed.
- Current workspace limitation (outside this lake change): `cargo fmt --all --check` is now blocked by formatting differences in `crates/swe-fog/src/parameters.rs` and `volumes.rs`; `cargo clippy --workspace --all-targets -- -D warnings` is now blocked by `String`/`Box<dyn Error>` type errors in those two Fog files. These unrelated files were not changed merely to pass checks. Repository-wide formatting, clippy, 74 library tests and full GPU regression actually passed at lake delivery; the current results do not claim a repository-wide pass.
