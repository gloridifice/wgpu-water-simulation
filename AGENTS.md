# 项目协作说明

本文件适用于整个仓库。与用户使用中文交流；以当前代码和用户最新要求为准，不将讨论中的设想描述为已实现功能。

## 项目与入口

- 项目使用 Rust 2024 edition。
- 根包 `cargo run` 默认启动 GPU Ocean；CPU 二维 PIC 终端程序与一维浅水模式独立位于 workspace 成员 `crates/tui-2d-prototype/`。
- 默认启用的 `ocean` feature 提供 wgpu 三维海洋：远海 FFT、近岸局部二维 SWE、泡沫、沙滩与水体光学，以及 egui_dock 参数面板。
- 修改 GPU 海洋时，保留独立 CPU 包的可用性，不向其引入 GPU 依赖；`crates/tall-cell-fluid/` 保持独立 workspace。
- 使用说明集中在 `doco/guides/`，入口见 `README.md`；架构与精确合约分别在 `doco/architecture.md`、`doco/specs/`。修改用户可见行为、参数或启动方式时，同步相关说明。

## 代码位置

| 位置 | 职责 |
| --- | --- |
| `crates/tui-2d-prototype/src/{main.rs,pic/,tui/}` | CPU PIC 入口、求解与终端交互 |
| `crates/tui-2d-prototype/src/{solver.rs,shallow_tui/}` | 一维浅水求解与终端交互 |
| `src/bin/ocean.rs` | GPU 海洋入口及命令行参数 |
| `src/ocean/viewer.rs` | 窗口、相机、帧调度与运行状态 |
| `src/ocean/spectrum.rs`、`gpu.rs`、`shaders/{evolve,ifft}.wgsl` | 波谱、GPU FFT 与演化 |
| `src/ocean/shore.rs`、`shaders/shore*.wgsl` | SWE、FFT 输入、近岸泡沫及岸边状态读取 |
| `src/ocean/foam.rs`、`shaders/foam.wgsl` | FFT 海洋泡沫 |
| `src/ocean/shaders/render.wgsl`、`optics*`、`seabed*`、`skylight*` | 水面、海底与天空光学 |
| `src/ocean/gui.rs`、`color_editor.rs`、`theme.rs` | Dock 面板、参数控件、颜色编辑与主题 |
| `src/ocean/settings.rs`、`configuration_panel.rs` | 会话保存、命名配置及配置管理界面 |
| `src/ocean/verify.rs`、`*_verify.rs`、`gpu_timer.rs` | GPU 验证与计时 |

## 常用命令与验证

在仓库根目录执行：

```powershell
# 启动 GPU 海洋；恢复个人参数时不要额外指定场景预设。
cargo run
# 优化构建用于观察性能。
cargo run --release

# 启动原有 CPU 程序。
cargo run --release -p tui-2d-prototype

# 格式、静态检查与包含海洋模块的单元测试。
cargo fmt --all --check
cargo clippy --features ocean --all-targets -- -D warnings
cargo test --features ocean --lib
# CPU 包测试；--workspace 可同时覆盖根包与 CPU 包。
cargo test -p tui-2d-prototype

# 构建并验证实际 GPU 路径。
cargo build --release --features ocean --bin ocean
target\release\ocean.exe --verify

# 有限帧数的界面冒烟运行，不覆盖个人配置。
target\release\ocean.exe --beach --no-settings --frames 600
```

- 根据修改范围选择验证。纯文档修改不需要构建；纯布局修改不必重复完整物理验证。
- `cargo test --features ocean --bin ocean` 不能替代库测试；海洋模块的测试应使用 `--lib`。
- Rust 编译通过不代表 WGSL 正确。修改着色器、绑定或缓冲区布局时，应运行相关 GPU 验证；无法运行时明确说明。
- 修改 CPU 包或 workspace 边界时，用 `cargo test -p tui-2d-prototype` 和 `cargo clippy -p tui-2d-prototype --all-targets -- -D warnings` 验证 CPU 路径；启动指定 `-p tui-2d-prototype`。目录名和 Cargo 包名均为 `tui-2d-prototype`。根包 `--no-default-features` 不再选择 CPU 程序。
- 比较视觉结果时固定相机、时间、随机种子、波谱与求解阶数。`--snapshot` 输出渲染画面，不能用于验证参数面板布局。
- 不为可逆的小型界面修改添加仅复述实现的测试。数值边界、配置兼容、质量输送等行为需要有意义的验证。

## 模拟与 GPU 约定

- WGSL 文件放在所属模块的 `shaders/` 子目录，使用相对源文件的 `include_str!` / `include_bytes!` 编译期嵌入；跨模块复用时引用实际所属模块，不复制文件。
- 保留远海 FFT 与近岸 SWE 的分工；当前耦合方向为 FFT → SWE。不要无意中重新引入独立近岸造波器。
- FFT 输入涉及水位和匹配动量；调整过滤、增益或初始化时，同时检查两者，避免重复应用增益。
- 保留一阶、二阶 SWE 切换。修改求解器时检查静水平衡、干湿边界、稳定步长及非负水深。
- 水上泡沫密度、沙地残留泡沫、沙滩湿润历史和远海 FFT 泡沫是不同状态，不要混用其阈值、颜色或衰减参数。
- 近岸水上泡沫考虑生成、输送、汇聚和衰减。密度不等同于显示透明度；不要为显示方便把模拟密度直接截断到 0～1。
- 保守输送已能表达汇聚和稀释；新增压缩增强项前，检查是否重复计算同一效果。
- 保留密度调试视图，便于独立验证密度与着色。孔隙纹理和小波改进按用户明确要求推进，不自行扩大任务范围。
- Rust uniform/storage 数据结构、WGSL 定义、对齐、绑定、复制大小和子步偏移必须一致。新增字段需检查全部相关着色器。
- SWE GPU 耗时使用真实 GPU timestamp，兼容不支持该功能的设备；不能把 CPU 提交耗时标为 GPU 耗时，也不要为计时每帧阻塞等待 GPU。
- 参数注明单位。区分毫米与米、模拟时间与现实时间、无量纲阈值与变化率。解释公式时先说明中文名称和含义，避免未定义字母。

## 参数面板约定

- 使用 egui_dock，渲染视图置于中央，参数面板安排在两侧。
- 每个逻辑参数组使用一个共享的两列网格，让同组多行实际对齐；不要每行创建独立网格。
- 左列为名称，右列为参数控件；复用统一布局辅助方法。扣除列间距后，名称列占 42%，参数列占 58%。
- 长名称允许换行；控件不得撑大列宽，导致不同参数组的列边界错位。
- 分组标题使用普通字号的加粗 `Label`，不使用标题组件。
- 参数解释放在名称的 hover 提示中，不常驻显示介绍小字。保留必要的操作按钮、当前状态及错误提示。
- 参数提示说明作用、单位、范围，以及修改后实时生效还是重置模拟。
- 只调整面板时，不顺带修改模拟、着色或参数默认值。

## 配置与参数接入

- 新增参数检查完整链路：默认值、合法范围、有限数校验、序列化兼容、面板、CPU → GPU 传递、WGSL 使用点，以及必要的重置或重建条件。
- 加字段时让旧配置能够使用默认值加载；不要仅因旧文件缺少新字段就丢弃整份配置。
- 分清当前工作状态自动保存、命名配置手动保存和 Dock 布局保存。它们不是同一功能；不能仅保存模拟参数就声称保存了面板布局。
- 当前参数保存入口在 `settings.rs`，默认路径由 `default_path()` 决定。不要硬编码开发者机器上的绝对路径。
- 命名配置通过配置库管理。保持默认配置快照、另存、覆盖与自动保存的既有语义，避免自动保存意外改写命名预设。
- 排查“保存后恢复不一致”时，检查读取路径、保存错误、CLI 覆盖和 `--no-settings`。尤其注意当前 `--beach` 会应用场景预设并覆盖部分已保存状态。
- 自动验证和预览尽量使用 `--no-settings` 或专用测试文件，不覆盖用户正在调试的个人配置。

## 工作区与交付

- 修改前查看工作区状态和相关代码。保留用户已有的未提交修改，不还原、不覆盖、不混入无关重构。
- 不擅自提交或推送；用户要求提交时，检查差异并只提交约定范围。
- Windows 上运行中的程序可能锁定可执行文件。不要为了构建终止用户正在运行的程序；可使用其他构建产物或说明阻碍。
- 预览程序优先限制帧数；如需清理，只操作本次任务明确启动的进程。
- 不把构建产物、截图、临时配置或调试日志加入版本管理，除非用户明确要求。
- 参考外部项目时核查资源许可与来源，按需保留署名；复制资源后使用仓库内路径，不依赖个人下载目录。
- 完成后简述改动、验证结果及未验证的限制。不要把代码检查、编译通过或截图推断说成实际运行确认。

<!-- DOCO:START -->
<!-- doco:entry template=v2 -->
## Doco

For current project documentation and explicitly tracked changes, use the
`doco` skill. Read `.agents/skills/doco/SKILL.md` before creating, executing, completing,
or archiving a change. Perform only the requested phase.
A doco change package is optional for implementation and is not an
implementation-history mechanism: archiving retains only proposal.md. Routine
behavior fixes and implementation-detail changes may proceed without creating
a doco change unless the user explicitly requests tracking. Use Git, pull
requests, or release notes for implementation history. Regardless of tracking,
update current architecture and specs when their documented facts or contracts
change.
Use the doco library nearest to the working directory, or select one with
`doco --root <library-root> ...`. Child libraries keep their own documents and
changes but reuse this installed skill; do not generate child skill or Agent
entry files. Resolve the skill path above from this entry's directory and
`doco/` paths from the selected library. Parent documents describe shared
constraints; child documents describe local facts without automatic overrides.
`doco list` includes all project libraries, current first; write commands and
change IDs remain local to the selected library.
Start from its `doco/architecture.md` and relevant current specs and decisions.
When implementing a selected tracked change, use its proposal and any present
design and task files. Treat completed changes, archived changes, and
`doco/tmp/` as non-current material; consult them only when explicitly needed.
<!-- DOCO:END -->
