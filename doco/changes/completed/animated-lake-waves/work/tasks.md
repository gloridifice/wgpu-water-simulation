# 实施任务

实现、自动验证与原生桌面验收已执行；当前工作区另有 Fog 模块的格式／编译问题，详见 4.2 验证记录。设计见 [implement.md](implement.md)，验收编号对应 [proposal.md](../proposal.md)。

- [x] 1.1 定义湖泊配置、模式参数、默认值、有限数与范围校验以及旧配置兼容。
  Acceptance: proposal 验收 6；旧配置仍使用原海洋模式，湖泊参数及两模式独立设置可往返，活动波不序列化。

- [x] 1.2 实现确定性不规则湖岸、湖底、程序化岩石及共享干湿几何数据。
  Dependencies: 1.1
  Acceptance: proposal 验收 1；固定种子可重建，无岸线自交、无限放置重试或岩石与遮罩明显错位。

- [x] 1.3 生成深度与含露水岩石孔洞的二维欧氏 SDF，提供统一坐标采样和场诊断资源。
  Dependencies: 1.2
  Acceptance: proposal 验收 1、2；规则参考误差在网格尺度内，水下岩石只影响深度，SDF 符号和距离单位正确；平台与中轴处理有限。

- [x] 2.1 实现有界波事件池、冻结发射参数、解析推进、一次反射和容量预留。
  Dependencies: 1.1
  Acceptance: proposal 验收 3、4；精确抵岸时刻不依赖帧分割，扇区不翻转，反射较弱且仅一次，清空／过期不反射，暂停与池满行为符合设计。

- [x] 2.2 实现 GPU 波记录、角包络、双模式波场合成及一致高度／坡度发布。
  Dependencies: 1.3, 2.1
  Acceptance: proposal 验收 2、3、5；CPU/WGSL 布局一致，角度跨界和交接连续，无干域水面、除零或非有限法线，无正常帧状态回读。

- [x] 2.3 接入 Renderer、程序化不透明场景、窗口与快照调度，隔离海洋与湖泊资源路径。
  Dependencies: 2.2
  Acceptance: proposal 验收 1、5；湖泊不推进 FFT/SWE/泡沫/浪花，无外围无限海面，支持的光学用湖波高度法线；物理焦散／物理折射在湖泊显式不可用，原海洋保持可用。

- [x] 3.1 接入场景入口、湖泊 Dock 面板、草稿应用和配置保存规则。
  Dependencies: 2.3
  Acceptance: proposal 验收 6；五分类不变，深度／SDF 显式应用重置且失败保留旧场景，参数单位与生效语义清楚，岩石可配置，已有 Dock 布局不被覆盖，只读不写盘。

- [x] 3.2 增加有行为价值的 CPU 与真实 GPU 验证入口并完成固定条件视觉对比。
  Dependencies: 3.1
  Acceptance: proposal 验收 1–6；先读取 test-audit skill；验证场生成、距离参考、单次反射、时序分割、角度跨界、配置兼容与生产 shader，不以编译代替 GPU 检查、不以离屏图代替 UI。

- [x] 4.1 同步已实现架构、Ocean 使用说明与工作区／湖泊行为规范。
  Dependencies: 3.2
  Acceptance: proposal 验收 7；记录两种传播场单位与局限、岩石行为、单次反射、禁用光学、参数重置和保存边界，不将计划或未验证视觉描述成既成事实。

- [x] 4.2 执行最终格式、静态检查、库测试、相关 GPU 验证、真实 UI 与性能验收，记录限制。
  Dependencies: 4.1
  Acceptance: `cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --features ocean --lib` 及相关实际 GPU 验证通过；固定种子／相机／时间对照两模式与反射，记录默认及满池 GPU 耗时；窗口限帧且 no-settings，不覆盖个人配置，不终止用户程序。无法运行的检查明确列出，不声称全部通过。

## Verification

- 1.1–1.3：湖泊配置兼容／往返／非法值拒绝、固定种子场生成、独立圆环＋岛屿距离参考（网格容差）及水下岩石只改变深度的库测试通过。
- 2.1：帧时间分割、固定扇区、较弱后代、单次反射、暂停、满池预留与清空取消行为测试通过。
- 2.2–2.3：RTX 3070 / Vulkan，实际 GPU compute 和 raster 检查通过；包括干湿／波谷约束、高度坡度一致、两模式差异、±180° 连续、512 秒重基准、反射交接、静水／清空、物理光学隔离、快速折射／SSR 与切回原海洋。交接两侧 0.0002 秒的最大高度差约 4.8e-5 米。
- 3.1：生产 egui 控件测试确认选 SDF 只改草稿，显式应用才更新模式与重置请求；既有 Dock、配置保存／只读测试通过。专用只读 512² SDF 配置完成快照和 120 帧窗口运行，文件 SHA-256 前后一致，未产生布局 sidecar。
- 3.2：`cargo test --features ocean --lib`：74 passed。已按 test-audit 的行为／独立预期要求编写测试，没有新增仅供测试的生产接口。固定场景种子 73、波种子 42、湖泊相机、t=7 的深度／SDF 快照已读取对比；岩石与岸域可见，波纹较弱，不据静态截图声称运动方向已做人工确认。
- 4.1：已同步 [architecture.md](../../../../architecture.md)、[Ocean 指南](../../../../guides/ocean.md)、[UI 规范](../../../../specs/workspace-ui.md)，新增 [湖泊规范](../../../../specs/animated-lake.md)，明确动画／SDF 局限和不支持光学。
- 4.2 已通过部分：`cargo fmt --all --check`；`cargo clippy --workspace --all-targets -- -D warnings`；74 个库测试；release 构建；专项 `--verify-lake`、`--verify-optics` 和完整 `--verify`（含原 FFT/SWE/泡沫/浪花／光学回归）；只读 512² 窗口冒烟及两模式快照。最终完整 GPU 日志临时保留在忽略目录 仓库根目录下的 target/lake-full-verification-final.log（临时证据，不是持久交付文档）。
- 性能：256² 波场合成（不含绘制／UI），真实 GPU timestamp，预热后 9 次；4 条活动波中位数约 0.018 ms，128 条约 0.093 ms。结果仅代表本机 RTX 3070，不承诺其他硬件帧率。
- 原生 UI 已确认部分：限帧、no-settings 窗口运行，cutty 截图实际读取确认五分类、湖泊场景入口、共享两列参数、深度／SDF 选择器及禁用保存状态。截图与临时配置均不纳入版本管理，程序按有限帧自行退出，没有终止用户进程。
- 4.2 原生桌面补充验收：使用任务专用 `target/lake-ui-acceptance.json`，未触碰个人配置。真实窗口点击已确认应用 SDF 并手动保存，暂停时“发射一道”使活动波由 0 变为 1；将“湖泊动画”拖到“场景与地形”同组后关闭本次测试进程并重启，sidecar 与 cutty 截图确认双标签及活动标签恢复。窗口均限帧，只关闭本次明确启动的测试进程。
- 4.2 MCP 补充验收：Pi 工具目录当时未暴露项目工具，因此用本地 MCP SDK 建立独立 `target/debug/ocean.exe --headless --no-settings` stdio 会话，未改 `.pi/mcp.json`。`scene_describe`／`parameters_get` 后发现字段和动作，实际调用参数补丁、`lake.emit`／`lake.clear`、固定步进与截帧；暂停状态仍接受显式步进。固定种子和相机的波高诊断图已读取，确认深度入射、同扇区较弱反射、SDF 弧段及清空归零；未仅凭 PNG 成功断言视觉结果。
- 4.2 当前复检：`cargo clippy --features ocean --lib -- -D warnings` 通过；`cargo test --features ocean --lib` 为 75 passed（新增的一项来自后续 MCP 接入）；`target/debug/ocean.exe --verify-lake` 再次通过，4／128 波中位耗时约 0.018／0.102 ms。`git diff --check` 通过。
- 当前工作区限制（不属于本湖泊变更）：`cargo fmt --all --check` 现被 `crates/swe-fog/src/parameters.rs`、`volumes.rs` 的格式差异挡住；`cargo clippy --workspace --all-targets -- -D warnings` 现被这两个 Fog 文件中的 `String`／`Box<dyn Error>` 类型错误挡住。未为通过检查擅改这些无关文件。湖泊交付时上述全仓格式、clippy、74 个库测试及完整 GPU 回归均已实际通过；当前结果不声称为全仓通过。
