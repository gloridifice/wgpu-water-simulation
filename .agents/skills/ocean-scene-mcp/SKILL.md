---
name: ocean-scene-mcp
description: 使用本仓库 Ocean 场景 MCP 做自动化调试：查询和批量修改海洋、近岸、湖泊参数，执行已注册动作，固定步进并截取当前 GPU 场景。用户要求调场景参数、执行场景按钮、自动截图或对比物理与渲染结果时使用；不适用于普通代码静态分析或 Fog 场景。
---

# Ocean 场景 MCP

从仓库根目录启动 Pi，并确认信任本项目。`启动前先在仓库根目录构建：

```powershell
cargo build --features ocean --bin ocean
```

`.pi/mcp.json` 直接启动 `target/debug/ocean.exe --headless --no-settings`（Windows 为 `.exe`）：一个**无窗口、无 UI** 的 Ocean 场景，默认参数，不读取或写入个人配置；配置不自动编译，源码变更后需先关闭该 MCP 子进程并手动重建、重连；需要本机可用 GPU。Pi 项目配置采用 `deferred` 暴露，通过 `tool_search` 搜索 `ocean-scene` 或 `mcp__ocean_scene` 下的工具，再调用 `mcp__ocean_scene__<工具名>`。如未连接，运行 `pi mcp list` 排查，交互中可用 `/mcp`、`/reload`；配置文件改动后须重新连接。不要将 MCP 协议 JSON 直接发给普通终端。

## 操作顺序

1. 调 `scene_describe` 获取当前可执行的动作；调 `parameters_get` 读取配置、时间与版本。首次操作不要猜字段名：路径以返回的 `settings` 为准。
2. 需要修改时，调 `parameters_patch`，参数示例：`{"changes":{"optics.shore.foam_emission_enabled":false,"camera.gain":1.5}}`。只允许现有标量字段；数组用数字下标，例如 `camera.target.0`。多字段作为一个候选整体校验；`isError: true` 时不要当成成功，也不要自行重试不明确的非幂等动作。返回 `structuredContent` 中有实际状态和配置。
3. 波谱草稿 `draft.*` 要调用 `action_call`，参数 `{"id":"ocean.apply_draft"}` 才应用；直接写 `ocean.*` 立即应用。湖泊地形参数 `optics.lake.*` 在无 UI 模式直接应用，变更传播场会重置历史；它没有湖泊草稿。动作只使用 `scene_describe` 当前列出的 ID（如 `simulation.reset`、`camera.reset`、湖泊下 `lake.emit`／`lake.clear`）；不模拟 egui 点击。
4. 用 `simulation_step` 显式推进，参数 `{"frames":60}` 即请求 60 × 1/60 模拟秒，最多 600 帧；当前模式无自动播放，`paused` 与 `speed` 不影响显式步进。每步返回表示命令已提交 GPU 队列，不是完成 GPU 回读；截帧则会等待 GPU。
5. 用 `frame_capture` 将当前实例渲染到**尚不存在**的 PNG，例如 `{"path":"target/mcp-scene.png"}`。默认只写入 `target/` 下任务专用文件，不写根目录或版本库；截图不含参数面板。比较图像时固定配置、相机、种子和步数。需要看图时读取生成的文件；不要仅凭 PNG 成功返回断言视觉效果。

## 安全与边界

- 项目 MCP 默认为只读会话；修改仅在内存中。确需手动持久化，必须用户明确指定独立的 `--settings PATH` 启动参数，再调用 `configuration.save`；不要改成个人配置路径，不改写 `.pi/mcp.json` 或覆盖用户快照以便调试。
- 若要从非默认场景（如 `--beach`、`--lake`）启动，先与用户确认目标场景；可改用一次性的独立 MCP 启动配置，或任务专用只读 `--config PATH`，不覆盖当前 `.pi/mcp.json` 里其他人依赖的默认场景。
- 当前工具只控制**该 MCP 子进程中的一个 Ocean 场景**，不会连接用户以普通 `cargo run` 启动的 Dock 窗口。需要无参数界面但能显示场景时，可显式用 `--no-ui` 代替 `--headless` 启动另一个 MCP 进程；Fog 使用独立的 `fog-scene` MCP 服务和 `fog-scene-mcp` 技能；此 Ocean 入口不控制 Fog。HTTP 附加、任意面板按钮和逐字段单位／范围元数据尚不支持。详细合约见 `doco/specs/scene-control.md`，启动说明见 `doco/guides/ocean.md`。
