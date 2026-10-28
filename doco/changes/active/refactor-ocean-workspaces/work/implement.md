# Implementation design

## 1. Baseline and goals

基线定位：Git `6a31b4e`。创建前工作区只有用户的未跟踪原型目录 prototypes，本变更不读取或修改该目录。以下均是计划，除本次图标资源与提案外，不表示功能已经交付。

实际入口：

- [src/ocean/gui.rs](../../../../../src/ocean/gui.rs)：`Panel`、`default_dock()`、`Gui::frame()`；当前只持有一份 `DockState<Panel>`，默认左上远海／配置、左下远海泡沫／浪花／性能、右侧 SWE／模型／碰撞体／光学，中间 Viewport。`ocean_view()` 已隔离相机输入。
- `parameter_table()`、`parameter()` 已提供 42%／58% 两列表格；继续复用，不为每行创建独立 Grid。
- `model_panel.rs` 维护独立模型草稿及显式应用；`configuration_panel.rs` 维护命名配置选择和操作；`spray_panel.rs`、`collider_panel.rs`、`color_editor.rs`、`performance.rs` 提供现有子面板。
- `settings.rs::SavedSettings` 只包含模拟、相机、播放参数。`SettingsDocument` 另存命名配置库，目前没有布局持久化。
- `viewer.rs::saved_settings()`、`persist()`、`load_configuration()` 管理模拟保存／加载。命名配置加载会重置模拟，这与工作区切换必须严格区分。
- `Cargo.toml` 已给 `egui` 和 `egui_dock 0.18` 开启 serde；仍不能把版本相关的 Dock 内部结构直接当成长期稳定文件格式。
- `OpticsConfig::swe_enabled()` 的实际条件为 `(beach || model.enabled) && shore.enabled`；显示门控不能只看一个布尔值。浪花仍遵守现有 [GPU 浪花合约](../../../../specs/gpu-spray.md)。

目标契约与验收场景见 [workspace-ui.md](specs/workspace-ui.md)。

## 2. Overall approach

### 界面所有权

`Gui` 继续唯一持有模拟编辑值、错误状态、颜色编辑器、模型草稿、配置库和一个 Viewport 纹理。新增工作区布局状态，不复制整份 `Gui` 或 GPU 资源：

```text
Gui
├── shared editor state / model draft / configuration panel
├── WorkspaceLayouts
│   ├── active: Scene | Shallow | Offshore | Shading | Configurations
│   └── five independent WorkspaceLayout values
└── one viewport texture and camera input path
```

外壳顺序：常驻工具栏 → 固定左侧工作区导航 → 占剩余区域的 DockArea。导航不作为 Tab，点击后只选择布局。默认参数区在左，主要场景在右；参数区宽度约占 Dock 区的 31%。场景上区为场景与地形、下区为模型／碰撞体；浅水上区为 SWE 与输入、下区为近岸泡沫／浪花；远海上区为波谱与波形、下区为远海泡沫；着色上区为水体与天空、下区为其他材质；配置不拆分。已有布局不会因默认调整被覆盖。允许改变 Dock 分割与标签位置；不增加浮动窗口。当前工作区的 Viewport 页签不可关闭且不可重复添加，参数页签可以关闭和重新添加。

### 模块职责

建议新增 `workspace.rs` 管理工作区／面板标识、默认布局、添加／关闭／切换、可用性协调和布局校验；新增 `workspace_settings.rs` 管理独立布局文件。`gui.rs` 保留 UI 外壳、统一控件和渲染合成，按职责提取远海／浅水／着色面板，复用现有模型、配置、性能实现。具体小文件拆分可调整，但不在渲染器／求解器内引入 UI 分类。

每个参数只有一个主要归属，跨工作区添加的是同一个面板类型。模拟参数与草稿只维护一份；可折叠／滚动等 UI ID 以 `(workspace_id, panel_id, group_id)` 区分，防止切换工作区串状态。草稿不能按工作区重新初始化。

## 3. APIs and data model

以下是拟新增的内部边界，不是当前已存在 API；实现者可调整名称，但不改变所有权和契约：

```rust
// UI-only identities with stable serialized string names.
enum WorkspaceId { Scene, Shallow, Offshore, Shading, Configurations }
struct WorkspaceLayouts { active: WorkspaceId, /* five layouts */ }

fn default_layout(workspace: WorkspaceId) -> WorkspaceLayout;
fn panel_availability(panel: PanelId, state: &UiAvailability) -> PanelAvailability;
// Availability includes an accessible enable/recovery entry where needed.
fn reconcile_availability(layout: &mut WorkspaceLayout, state: &UiAvailability);
fn reset_current(layouts: &mut WorkspaceLayouts);
```

`UiAvailability` 每帧从当前场景、SWE、泡沫模式、浪花、光学与编辑状态派生，不引入第二份功能开关。返回可用／仅有启用或错误入口／隐藏三种 UI 结果；不修改业务值。

### 布局文件

- `WorkspaceDocument` 有独立 `version`、当前工作区及五套布局。每套以自有递归树保存分割轴、有限的分割比例、叶子面板 ID 顺序与激活项；另保存因不可用临时停放的面板及恢复锚点。不要保存像素矩形、GPU 对象或 egui 内部临时状态。
- 文件从实际可写 `SettingsStore.path` 派生：`<会话文件 stem>.workspaces.json`，与会话文件同目录。例如 `ocean-settings.json` 对应 `ocean-settings.workspaces.json`。不硬编码用户绝对路径，不修改已有 `SavedSettings`／命名配置的数据结构。
- 只有存在可写会话存储时才读取／保存关联布局；`--no-settings`、只读 `--config`、快照、验证均不隐式读写布局。普通无文件启动使用默认布局，默认选择“场景”。
- 未知版本或无法解析的布局：显示布局错误并回退默认 UI，不丢弃模拟参数。原损坏布局保持不动；用户点击“重置当前布局”或明确确认保存修复后，才允许替换损坏文件。个别工作区无效只回退该工作区。
- 树深度不超过 16、叶子不超过 64；比例为有限数且在 0～1 的开区间，展示时遵守可用空间下限。未知面板 ID 被跳过；重复面板去重；缺少 Viewport 时补回，保留已知可恢复面板。不信任载入的活动页签索引或隐藏锚点。
- 保存使用同目录临时文件原子替换；错误仅影响布局保存状态，保留最后成功文件和当前内存布局。最多每 500 ms 比较一次规范化布局快照，稳定后保存，退出时刷新；不每帧写盘或回读 GPU。
- 命名配置加载／保存不触碰工作区文件。运行期“添加面板”、用户关闭、当前分类和重置属于 UI 变更；个人侧车文件不加入版本管理。

### SVG 资源

[assets/ui/workspaces/](../../../../../assets/ui/workspaces/) 保存五个原创、可编辑、无嵌入位图的 24×24 SVG：`scene.svg`、`shallow.svg`、`offshore.svg`、`shading.svg`、`configurations.svg`。统一 1.75 单位圆端描边，主体留边，不依赖网络、字体或脚本，使用 `currentColor`。预览包含实际尺寸和选中态；HTML 与生成参考图仅在系统 Temp。

执行接入时通过编译期嵌入加载；为 `currentColor` 指定主题颜色后按缩放缓存解码结果，不在每帧或每次切分类时解码。SVG 解码依赖必须受 `ocean` feature 限制，不改变 CPU 构建边界。具体缓存／解码工具属于局部实现选择，不能以 Unicode emoji 代替交付 SVG。

## 4. Algorithms and rules

### 工作区切换与 Dock 编辑

1. 每帧读取共享模拟状态，派生可用性。
2. 对状态变化涉及的工作区协调可见面板，不因普通切换调用 `default_layout()`。
3. 绘制固定导航，处理当前工作区选择；只有被选工作区参与本帧 Dock UI。
4. 绘制当前 Dock；面板编辑继续走现有应用、校验和重置链路。单纯导航不得设置 `apply`、`clear_foam`、`load_configuration` 或修改相机。
5. “添加面板”菜单列出可用且未添加的面板；当前已有项定位并激活，不创建重复实例。用户主动关闭的面板不因功能再启用自动打开。
6. 规范化并记录 UI 布局；Viewport 尺寸变化只沿现有 `resize_view()` 和渲染目标更新路径处理，不重建 Ocean／Shore。

### 临时隐藏与位置恢复

`egui_dock` 不提供业务可用性语义，必须在应用层协调，而不是每帧过滤副本后丢失用户拖动结果。

- 只有可用性发生变化时才移除／恢复面板。移除前记录同叶子的相邻面板 ID、页签顺序、分割祖先方向与比例，以及最近存活分支中的面板锚点；不只存会因树压缩失效的 `NodeIndex`。
- 仍有同组面板时按相邻面板恢复原顺序。原叶子因隐藏被删且其他结构未变时，按记录的分割链恢复原区域。
- 隐藏期间用户改变了相关结构、原锚点已不存在时，优先邻近存活锚点，再回落到该工作区默认参数侧；不重置整个布局、不覆盖其他拖动。
- 暂时隐藏和用户关闭分开记录；隐藏位置也持久化。清空当前布局只清空当前工作区的停放记录。
- 启用入口必须放在仍可见的主面板；浪花失败等错误同时在常驻状态入口可达，不能随不可用内容消失。

### 参数归属与条件

具体矩阵见目标规范。重要限制：

- FFT 耦合增益、短波过滤与 FFT 混合水深放同一浅水输入面板；不改变水位与动量的现有共同增益。
- 背景泡沫速率、倍率、起始阈值、过渡范围、汇聚阈值归一组；前沿和脊线实验归另一组。
- 单浓度输送／拖曳／衰减与双浓度半衰期／转换分别成组，只显示当前路径；密度不等同于显示透明度。
- 材质选择在“着色”，生成和输送在“浅水”；泡沫共用颜色及显示开关不能被 SWE 关闭条件遮蔽。
- 模型计算域和边界保留在“场景”的同一草稿应用范围，SWE 分辨率在“浅水”仍实时生效。水位／位置修改不能因拆 UI 变成实时应用。
- `OpticsConfig::depth` 移到场景海床；颜色色带／棋盘标定与沙地纹理归着色，按实际海底材质路径显示。
- 切换调试视图不使整个面板不停消失：只隐藏严格无效的依赖项，保留返回正常视图的入口。优先忠实呈现现有模式与优先级，不新增跨模块全局调试状态。

### 小枚举与无动作按钮

两到三项枚举采用主题化 selectable button 的共享水平控件。使用“一阶／二阶”“正常／雅可比／覆盖率”等短标签，算法全称留 hover。维持选中项和原数值映射，不自动替用户选择模式；宽度均分右列，设合理面板最小宽度，字体可正常换行但按钮排列保持横向。

“应用更改”无草稿变化、“保存配置”遇只读默认配置等属于操作状态，不机械套用参数隐藏规则；保持稳定位置与禁用原因。只读状态、错误、越界碰撞体的修复控件必须保留。

## 5. Fixed decisions and discretion

已确认：五个分类及顺序；FFT → SWE 归浅水；固定导航与可自由停靠的右侧区域；独立工作区布局及重启恢复；允许跨工作区添加面板；布局与模拟／命名配置分离；不改默认值及算法。

本方案明确的可执行默认：导航按钮为 38×38 pt 正方形、居中 24 pt icon、不显示文字，hover 和可访问名称保留分类名称；未选中按钮透明无边框，选中态保持主题高亮；侧栏四周 8 pt 留白、按钮间隔 8 pt、圆角 8 pt，配置分隔线上下间距一致；工具栏置于窗口顶部并横跨全宽，左侧导航从工具栏下方开始；工具栏操作左对齐、性能摘要右对齐，窄窗口时另起一行右对齐；两到三项枚举横向按钮；四项及以上维持下拉／列表；五个导航入口常驻；无动作按钮保留禁用提示；默认场景工作区；仅当前布局重置；保留单窗口约束。视觉留白、精确默认分隔比例和文件拆分可在不改变契约下微调。

用户已确认图标外观并授权执行。新增工具栏 FPS、CPU／GPU／SWE 指标：FPS 与 CPU 沿用 Performance 的 1 秒窗口；SWE 沿用已有异步时间戳。增加独立 GPU 主帧计时器，以支持 TIMESTAMP_QUERY 的空 compute pass 在本帧主 encoder 的 FFT 之前及 GUI 编码之后写入两个时间戳，覆盖模拟、场景和 GUI 主命令缓冲，不含呈现等待和额外回调命令缓冲；异步读回，一次最多一个在途样本，不阻塞帧循环。无支持／失败／尚无样本显示状态，SWE 条目在 swe_enabled() 为 false 时隐藏。暂停仍计量 GPU 主帧，SWE 显示空闲。没有待实现者自行决定的核心保存／切换语义。

## 6. Verification and documentation impact

- 创建阶段：`doco check refactor-ocean-workspaces`；五个 SVG 结构检查；浏览器查看 HTML 的实际尺寸、深浅背景、选中与 hover。HTML 不模拟真实 Dock 拖动，明确标为图标预览。
- 执行阶段：对切换不改变共享参数、布局存取隔离／损坏恢复、隐藏后位置恢复做有意义的状态／配置测试。编写测试前加载 `test-audit` skill，不添加仅断言标题文字和函数存在的测试。
- 实际 GUI 验证目标规范 A1～A8，包括切换与重启、草稿跨工作区、跨组添加、关闭／恢复、窄窗口与缩放；用专用配置或 `--no-settings`，不覆盖个人配置。用窗口截图验证 UI，而不是仅用 `--snapshot`。
- 根据改动执行 fmt、clippy、ocean 库测试；若新增依赖触及 feature 边界，验证 `--no-default-features` 的 CPU 构建和测试。纯 UI 工作不要求重复完整 GPU 物理验证；任何意外 shader／绑定变更应移出本变更或补相关 GPU 验证。
- 实现交付后更新 [docs/ocean.md](../../../../../docs/ocean.md)、[当前架构](../../../../architecture.md)，在当前 specs 目录新增 workspace-ui.md 界面合约，修正 [AGENTS.md](../../../../../AGENTS.md) 的两侧布局约定。创建阶段不更新这些当前事实文档。
