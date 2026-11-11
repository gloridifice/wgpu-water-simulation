---
name: fog-scene-mcp
description: 用本项目 SWE Fog 场景 MCP 自动化调试独立雾场：读取或批量修改 Fog Session 参数、重置、固定 30 Hz 步进和截取当前 GPU 画面。用户要求调 Fog/Grandma/Playground 场景、检查薄层雾状态或自动截图时使用；不适用于 Ocean 场景和普通静态代码分析。
---

# SWE Fog MCP

项目 `.pi/mcp.json` 中的 `fog-scene` **默认禁用**，避免 Pi 启动时同时创建两个 GPU 场景；需用户明确要调 Fog 时，用 Pi 的 `/mcp` 选择该服务并在本项目启用、重新连接。默认选择 playground，无窗口无 UI，不读取或写入个人配置。首次运行前执行 `cargo build -p swe-fog --bin swe-fog`；配置直接启动现有 `target/debug/swe-fog.exe`，不在重连时重建。源码改动后先关闭该 MCP 子进程，再手动重建、重连。GPU 必须可用。从仓库根目录运行 Pi，信任项目。工具采用 `deferred`，先用 `tool_search` 查找 `fog-scene` 再调用 `mcp__fog_scene__<工具名>`。

1. `scene_describe` 查看当前已注册动作和步长；`parameters_get` 返回实际 `Session` 和 GPU 模拟时间／故障状态。不猜字段；以当前返回的 `settings` 为准。
2. `parameters_patch` 接受 `{"changes":{"fog.wind.0":0.1,"fog.half_life":120}}`。字段路径必须存在；只批量写标量，数组用数字下标；整份 `Session::validate` 后应用，拒绝无效批量。Fog 配置变化保守地重置雾；场景切换或区域变更重建 GPU 场，原相机及配置尽量保留。`isError` 为真时不得视为成功。
3. `action_call` 的 `{"id":"simulation.reset"}` 清空并重新初始化雾；如确有用户指定的独立可写 `--settings PATH` 才能显式调用 `configuration.save`，绝不覆盖个人会话或默认项目 MCP 配置。
4. `simulation_step` 的 `{"frames":30}` 请求 30 × 1/30 秒物理演化，返回 GPU 实际接受时间、丢弃时间及故障。此离屏模式没有墙钟自动播放，显式步进不受暂停控制。参数改动重置后初始化零步不计入请求步数。
5. `frame_capture` 将**当前实例**保存到不存在的 PNG，如 `{"path":"target/fog-check.png"}`，不重播历史，不含 UI；默认只放 `target/` 并按需读取图片进行视觉检查。严格固定场景、参数和步数进行对比。

`fog-scene` 只控制它启动的 Fog 子进程，不是 Ocean 的 MCP，也不能附加已运行桌面窗口。模型导入／外部静态资产须使用 CLI 显式指定且先验证；不要让 MCP 路径调用任意文件导入。更详细的合约见 `doco/specs/scene-control.md`。
