# 单场景 MCP 控制（当前实现）

## 范围

- `ocean --headless` 创建一个持久 GPU Ocean 实例，不创建窗口、Surface、egui 或 Dock。默认未提供 `--config`／`--settings` 时从默认配置启动，不读取个人会话；`--config PATH` 严格只读。`--settings PATH` 显式打开可写会话，只有 `configuration.save` 才写盘。`--headless` 与截图、验证或窗口帧数模式互斥。
- `ocean --no-ui` 创建只显示场景的窗口及 MCP stdio，不创建 egui、Dock 或布局状态；可用 `--frames N` 有限呈现帧数预览。窗口按墙钟和 `speed` 自动推进，暂停时不前进；外部命令即使无法呈现也从事件循环处理。`--headless` 与 `--no-ui` 互斥。当前仅 MCP stdio；每个客户端启动一个独立进程及场景，stdout 仅传 JSON-RPC；无 HTTP 或普通参数面板窗口附加。common 只定义命令及传输；Ocean 独占场景并顺序执行命令。
- `tools/list` 返回 `scene_describe`、`parameters_get`、`parameters_patch`、`action_call`、`simulation_step`、`frame_capture`。执行错误返回工具级 `isError`；成功结果返回 `structuredContent`。

## Ocean 配置与动作

- `parameters_get` 返回已应用 `SavedSettings` 与运行时场景／时间／版本。`parameters_patch` 的 `changes` 是点分路径到标量值的映射；数组元素使用数字索引。不允许整体覆盖对象／数组，不允许新增字段。先反序列化、执行完整业务校验，再提交；无效批量请求不修改场景或配置。资源重建失败返回错误，不提交配置；对已有渲染资源的场景准备失败尝试恢复旧场景。
- 写 `draft.*` 只改远海草稿，`ocean.apply_draft` 才将草稿应用到 `ocean`；写 `ocean.*` 直接应用。湖泊参数 `optics.lake.*` 直接应用，地形／传播模式／波种改变会在下一次推进时重置波场；不存在独立湖泊草稿。单独修改远海网格密度／拓扑只重建绘制网格。海洋与湖泊共用 `ocean` FFT 参数；湖泊 `optics.lake.fft_strength` 实时调整叠加强度，0 关闭。发射／清空仅作用于沿岸程序波，不清空共享 FFT；参数补丁后可用零帧步进发布当前时刻的波场和诊断选择，再截帧。
- `action_call` 支持远海草稿应用、模拟重置、相机重置、湖泊显式发射／清空及显式保存。湖泊动作仅在湖泊场景可用，发射／清空在本次动作内提交零步 GPU 命令。未实现的 UI 按钮不能通过模拟点击执行。`configuration.save` 不覆盖命名快照、不保存 Dock 布局；只读模式拒绝。
- 无窗口实例不自动按墙钟推进；`--no-ui` 窗口按墙钟推进。`simulation_step` 接受 0～600 帧，每个非零帧以 1/60 模拟秒推进当前实例；初始化／重置零步不消耗请求帧数；显式步进不受 `paused`／`speed` 控制。返回请求时间，不把 SWE 内部实际接受步长伪称为真实耗时。步进返回表示命令已提交至 GPU 队列，非 GPU 完成。`frame_capture` 从当前实例绘制并阻塞回读 PNG（无窗口固定 1280×800，窗口使用当前尺寸），目标文件必须不存在；不额外暖机或重播历史，输出无 UI。

项目 `.pi/mcp.json` 默认启动 Ocean 场景；调试技能位于 `.agents/skills/ocean-scene-mcp/`。

## 限制

参数发现当前返回参数根名和完整配置值，没有逐字段单位／范围／重置元数据；具体值仍以应用校验为准。操作串行执行，长时间步进会阻塞后续命令；不提供 HTTP 并发控制或会话外的场景切换。更详细的启动示例见 [Ocean 指南](../guides/ocean.md#无窗口-mcp-调试)。
