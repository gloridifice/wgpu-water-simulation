---
name: ocean-scene-mcp
description: Use this repository's Ocean scene MCP for automated debugging: query and batch-edit ocean, nearshore, and lake parameters, execute registered actions, step at a fixed rate, and capture the current GPU scene. Use when asked to tune scene parameters, execute scene buttons, take automated screenshots, or compare physics and rendering results; not for ordinary static code analysis.
---

# Ocean Scene MCP

Start Pi from the repository root and confirm that you trust this project. First build from the repository root:

```powershell
cargo build --features ocean --bin ocean
```

`.pi/mcp.json` directly starts `target/debug/ocean.exe --headless --no-settings` (`.exe` on Windows): an Ocean scene **without a window or UI**, using defaults and neither reading nor writing personal settings. The configuration does not build automatically; after changing source, close this MCP subprocess, rebuild manually, and reconnect. A working local GPU is required. Pi's project configuration exposes tools using `deferred`: search `tool_search` for `ocean-scene` or tools under `mcp__ocean_scene`, then call `mcp__ocean_scene__<tool name>`. If disconnected, use `pi mcp list` to diagnose; `/mcp` and `/reload` are available interactively. Reconnect after changing the configuration file. Do not send MCP protocol JSON directly to an ordinary terminal.

## Workflow

1. Call `scene_describe` for currently executable actions and `parameters_get` for configuration, time, and version. Do not guess field names on first use: paths come from the returned `settings`.
2. To modify values, call `parameters_patch`, for example `{"changes":{"optics.shore.foam_emission_enabled":false,"camera.gain":1.5}}`. Only existing scalar fields are allowed; index arrays numerically, e.g. `camera.target.0`. Multiple fields are validated together as one candidate. Do not treat `isError: true` as success or automatically retry unclear non-idempotent actions. The returned `structuredContent` contains actual state and configuration.
3. Apply spectrum drafts `draft.*` by calling `action_call` with `{"id":"ocean.apply_draft"}`; writing `ocean.*` applies immediately. Lake terrain parameters `optics.lake.*` apply directly in no-UI mode; changes to the propagation field reset history. There is no lake draft. Only use action IDs currently listed by `scene_describe` (such as `simulation.reset`, `camera.reset`, or, for lakes, `lake.emit` / `lake.clear`); do not simulate egui clicks.
4. Advance explicitly with `simulation_step`: `{"frames":60}` requests 60 × 1/60 simulation seconds, up to 600 frames. This mode does not autoplay; `paused` and `speed` do not affect explicit stepping. Each step returns when submitted to the GPU queue, not when GPU readback finishes; capturing a frame waits for the GPU.
5. Use `frame_capture` to render the current instance to a PNG that **does not yet exist**, e.g. `{"path":"target/mcp-scene.png"}`. By default write only task-specific files under `target/`, not the root or versioned files; screenshots do not include the parameter panel. Fix configuration, camera, seed, and step count when comparing images. Read the generated file when visual inspection is needed; do not infer visual results from successful PNG creation alone.

## Safety and boundaries

- The project MCP defaults to a read-only session; changes exist only in memory. For manual persistence, the user must explicitly specify a separate `--settings PATH` startup argument before calling `configuration.save`. Do not substitute a personal configuration path, modify `.pi/mcp.json`, or overwrite user snapshots for debugging.
- To start a non-default scene (e.g. `--beach` or `--lake`), first confirm the target scene with the user. Use a separate one-off MCP startup configuration or a task-specific read-only `--config PATH`; do not overwrite the default scene in `.pi/mcp.json` that others depend on.
- These tools control only **one Ocean scene in that MCP subprocess**; they do not connect to a Dock window started by the user with ordinary `cargo run`. To show a scene without a parameter UI, explicitly start another MCP process with `--no-ui` instead of `--headless`. HTTP attachment, arbitrary panel buttons, and per-field unit/range metadata are not yet supported. See `doco/specs/scene-control.md` for the contract and `doco/guides/ocean.md` for startup instructions.
