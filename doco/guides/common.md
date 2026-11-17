# 公共桌面框架

## 所有权

`crates/common/` 是 workspace 内的库，供 GPU 桌面程序复用；不依赖 `water-simulation`，不持有 FFT、SWE、泡沫或场景业务参数。根包在 `ocean` feature 下使用它，CPU 包不依赖它，`tall-cell-fluid` 继续保持独立 workspace。

- `app::run` 接受窗口属性、可选呈现帧数及初始化闭包；应用实现 `WindowScene`。应用拥有输入消费策略和计算／绘制顺序，公共循环不强制模拟节拍，不用墙钟代替实际模拟时间。可选 `poll_commands` 在事件循环等待阶段运行，窗口不可呈现时仍能处理外部命令。
- `app::Desktop` 建立窗口 Surface 与 GPU；`app::acquire` 统一处理超时、遮挡、丢失和失效。零窗口尺寸不绘制，不计入有限呈现帧数。
- `gpu::Gpu` 提供窗口／离屏共用设备上下文；显式阻塞回读只供验证和离屏导出。`gpu::timer` 是至多一次在途的真实 timestamp 异步采样，不支持时返回 None；应用自己附加阶段、阶数或模拟步长元数据。
- `ui::Egui` 管输入平台状态、默认主题与 GPU 绘制；宿主安装字体和图片加载器。`ui::Viewport` 管可调整尺寸的原生 egui 纹理及颜色视图，不掌管相机输入。
- `ui::layout::{Layout,Workspace}` 以应用的 `PanelId` 参数化，保留规范布局中的临时隐藏面板，区别于主动关闭。应用提供面板、可用条件、默认布局和旧版本迁移；新程序不必复制海洋的分类注册表。
- `settings::SettingsStore<T>` 使用应用自己的 serde 类型与 `Settings::validate`。保存工作状态与命名库，保留只读默认快照；编辑不写盘，只有显式保存才持久化。`load_config` 严格只读，不替换坏文件。
- `settings::sidecar::Sidecar<T>` 与会话保存独立，负责布局读取上限、节流与失败反馈；损坏文件只有显式修复才可替换。应用仅在可写模式下创建该对象。
- `camera::Orbit` 只计算三维目标、眼点、投影及平移。角色运动、相机碰撞、海浪显示开关等由应用负责。
- 可选 `mcp` feature 提供 `control` 的 MCP stdio 适配与有界命令队列；应用自行持有场景、校验字段、实现操作和确定执行时序。当前 Ocean 的 `--headless` 和 `--no-ui` 纯场景窗口接入该适配；原有 Dock 桌面窗口尚未接入。参见 [Ocean 无窗口调试](ocean.md#无窗口-mcp-调试)。

## Ocean 兼容性

原 `water_simulation::ocean` 公开入口保留；GPU 设备为重导出，计时和相机有海洋适配。海洋 JSON 顶层字段、默认路径、命名配置、手动保存／自动布局边界及布局 v1/v2/v3 兼容保持原语义。海洋模拟、渲染 WGSL、参数默认值、FFT→SWE 顺序和离屏暖机未改变。独立实验窗口目前仍保留自己的应用适配，不宣称所有窗口都已共用公共循环。

## 验证

在仓库根目录执行：

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo check --no-default-features
cargo build --release --bin ocean
.\target\release\ocean.exe --verify
.\target\release\ocean.exe --beach --no-settings --frames 600
```

框架提取阶段实际运行了以上检查；Ocean 68 项库测试和 CPU 包 33 项测试通过，完整 GPU 检查和 600 帧窗口运行返回成功。截图检查显示海滩、水面、五分类导航、Dock 与常驻工具栏正常显示。固定个人配置／相机／1 秒模拟时间的离屏前后图像仅 411 像素有单色阶差异（最大 1/255），并非逐字节一致。

日志和图片位于 Git 忽略的 `target/migration-check/`，不作为源码资产。验证使用只读配置或 `--no-settings`，未写个人会话。截图只证明该视角的显示，不能替代其他平台的 Surface 故障检查或所有真实 Dock 操作。
