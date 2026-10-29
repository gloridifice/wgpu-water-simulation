# charming_egui 组件与主题

`crates/charming_egui/` 提供后端无关的 egui 控件、参数列表、选择列表和 TOML 主题加载器。基于 egui 0.33.3；可选 `dock` feature 使用 egui_dock 0.18。库不依赖 wgpu、winit、Ocean 参数或配置保存，也不包含 MiSans。当前仅作仓库内 path 依赖，未发布到 crates.io。

## 接入

```toml
[dependencies]
egui = "0.33.3"
charming_egui = { path = "path/to/crates/charming_egui" }
# 使用 Dock 时：
# charming_egui = { path = "path/to/crates/charming_egui", features = ["dock"] }
```

```rust
let theme = charming_egui::Theme::default();
theme.apply(&context);
// 应用按需调用 context.set_fonts(...)，主题不会覆盖它。
// 启用 dock 时，在创建 DockArea 后使用：
// .style(theme.dock_style().clone())
```

库默认 feature 为空，不主动启用 egui 的内置字体；由宿主选择默认字体或安装自己的字体。Ocean、slider-lab、foam-weight-lab 显式应用默认主题，再调用主程序 `src/ocean/ui/fonts.rs` 安装 MiSans。CPU 包不依赖此库。独立 `tall-cell-fluid` workspace 的 `demo` feature 仅为标题组件增加可选依赖，不应用本库的整体主题，保留自己的字体与界面配色；求解器和非 demo feature 不依赖此库。

## 三份主题

默认文件位于 `crates/charming_egui/src/theme/default/`，用 `include_str!` 编译期内嵌，运行不依赖工作目录：

| 文件 | 对应内容 |
| --- | --- |
| `egui.toml` | egui 原生样式、字体字号、间距和颜色 |
| `egui_dock.toml` | Dock 标签栏、分隔线与内容区；仅 `dock` 启用时读取 |
| `charming_egui.toml` | 自定义滑块、面板按钮、参数列表、选择列表、互斥选项 |

外部目录加载：

```rust
let theme = charming_egui::Theme::load_dir("themes/my-theme")?;
theme.apply(&context);
```

也可以由应用自行读取、下载或嵌入文件，再通过 `Theme::from_sources(ThemeSources { ... })` 解析。`ThemeSources` 的 `egui`、`charming_egui` 是 `Option<&str>`；启用 `dock` 时另有 `egui_dock` 字段。`None` 使用对应内嵌文件，不改变另两份主题。

### 加载约定

- 指定目录必须存在。目录内缺少某个文件时，使用对应内嵌文件；关闭 `dock` 时完全不打开 `egui_dock.toml`，即使该文件损坏。
- **外部文件替换整份对应文档，不与内嵌主题逐字段合并。** 建议先复制内嵌文件再修改。egui 部分字段缺省会使用上游 egui 默认值，而非本库默认值；Dock 文档需要完整字段；自定义文档需要各组件段，只有明确的可选字段可以省略。
- 解析返回 `Result<Theme, ThemeError>`；IO 错误带路径，TOML 错误带文件名和解析位置，自定义布局／尺寸错误带字段说明。非有限数、自定义负间距、非法列比例、非正宽度、透明滑块背景等被拒绝。
- 三份文件全部成功解析后才返回主题，解析本身不改变 Context。失败时由应用显示错误并继续保留当前主题，不自动覆盖或修复文件。
- `apply` 设置 egui 深／浅色模式、原生样式和当前 Context 的自定义样式，不修改字体、不修改其他 Context。Dock 样式需另行交给 DockArea。
- 不提供自动文件监听、不每帧读盘、不写入文件。Ocean 当前没有新增主题 CLI，也不隐式加载外部主题。
- 原生主题结构跟随 egui／egui_dock 版本；不承诺跨版本通用。函数型设置（如 egui 的数值格式化回调）不属于 TOML 数据。

`charming_egui.toml` 中距离使用逻辑点 pt，动画时长使用现实时间毫秒。颜色使用 `[R, G, B, A]` 数组。滑块圆角省略时跟随 egui 的 inactive 控件圆角；控件高度跟随 egui 交互高度。选择列表行间距省略时继承 egui。通用条目的交互颜色继续沿用 egui，不维护另一份重复色表。

## 参数列表

```rust
use charming_egui::{ParameterList, ParameterSlider};

ParameterList::new("water").show(ui, |list| {
    list.row("depth", "水深", "单位：米", |ui| {
        ui.add(ParameterSlider::new(&mut depth, 0.0..=100.0))
    });
});
```

每个逻辑组使用一个两列 Grid，默认名称／控件占扣除间距后的 42%／58%。保留隔行底色、长名称换行、整行垂直居中，以及超宽子控件的裁切。右列闭包允许任意 egui 控件，返回值原样传回调用方。

行 ID 与名称独立；同一列表中应唯一、稳定。需要沿用 Ui 闭包写法时可使用 `parameter_table(ui, id, |ui| ...)`、`parameter(ui, name, description, control)` 和 `group(...)`，它们使用同一实现；其中 `parameter` 以名称作为行 ID，因此组内名称必须唯一。

列表不持有业务参数，不做单位换算、参数合法性校验或保存。`.style(ParameterListStyle)` 可覆盖单个列表外观。

## 选择列表

```rust
use charming_egui::SelectionList;

SelectionList::new("profiles")
    .max_height(260.0)
    .show(ui, |list| {
        let response = list.item(profile_id, selected, name);
        if response.clicked() {
            // 由应用发起加载，而不是由列表提交业务选中状态。
        }
        response.context_menu(|ui| {
            // 应用自己的重命名或其他菜单。
        });
    });
```

- 条目占满内容宽度，默认允许换行；返回原生 `egui::Response`，支持 hover、键盘激活和右键菜单。
- 点击已选条目仍报告点击。选择状态、载入失败回滚、删除和持久化均由调用方持有。
- `.item_enabled(id, selected, enabled, text)` 单独控制条目是否可激活；只读业务对象不一定要禁用选择。
- 稳定条目 ID 在重命名或重排时保持交互身份；不同列表独立命名空间，不以显示文字作为身份。
- 当前是可变行高的普通滚动列表，不提供多选、虚拟化、树形结构或拖拽排序。`.style(SelectionListStyle)` 可覆盖单个容器外观。

Ocean 的配置面板已使用该组件。默认配置只读、禁用保存仍可载入、重复点击重新载入、失败回滚等规则继续留在原业务面板。

## 标题组件

```rust
charming_egui::h1(ui, "一级标题");
charming_egui::h2(ui, "二级标题");
charming_egui::h3(ui, "分组标题").on_hover_text("标题说明");
```

| 组件 | 默认字号 | 加粗 | 上／下 margin |
| --- | --- | --- | --- |
| h1 | 20 pt | 是 | 12／8 pt |
| h2 | 16 pt | 是 | 10／6 pt |
| h3 | 14 pt | 否 | 8／4 pt |

三个组件均使用 RichText + Label，颜色取当前 egui 的 `strong_text_color()`，不硬编码白色；随深浅主题保持高对比。egui 0.33 的 `RichText::strong()` 只改变颜色，h1／h2 因此采用轻微偏移叠绘实现合成加粗，不要求宿主提供粗体字体；h3 不叠绘。文字允许换行，返回文字本身的 Response，可添加 hover；留白不接收文字交互。局部竖直布局保证在工具栏和网格内上下 margin 也不会变成左右间距。

字号和 margin 放在 `charming_egui.toml` 的 `[headings.h1]`、`[headings.h2]`、`[headings.h3]`。旧主题没有 `headings` 时补内嵌标题默认值，不丢弃其他配置。字号必须为有限正数，margin 必须为有限非负数。

项目原先加粗的标题／强调文字统一改用 h3，错误和警告仍保留各自的语义颜色。`group` 使用 h3；旧 `parameter_list.group_spacing` 表示分组的最小上方间距，h3 上 margin 已计入，不重复叠加。

## 其他控件

- `ParameterSlider` 保留原生数值编辑、整数／步长、clamping 和有限线性／非负对数范围，只覆盖轨道和竖条绘制。`.appearance(&SliderAppearance)` 不改变高度；实验窗口用 `.height(pt)` 显式覆盖最小交互高度。
- `PanelButton` 保留原生按钮交互、主题 padding 和居中文本。组件主题默认上下外边距各 4 pt；`action_row` 在每按钮可用宽度不足 96 pt 时纵向排列。
- `choices` 用有界水平互斥按钮显示少量固定选项。

组件读取当前 Context 的 `CharmingStyle`；未调用 `Theme::apply` 时使用内嵌自定义默认，不主动替换 egui 原生样式。`CharmingStyle::get` 可读取当前配置。

## 验证与许可

```powershell
cargo test -p charming_egui --no-default-features
cargo test -p charming_egui --features dock
cargo clippy -p charming_egui --all-targets --all-features -- -D warnings
```

库测试覆盖主题读取／失败、Context 与字体隔离、列表布局与条目交互、滑块输入语义；真实窗口仍需通过 Ocean 和 slider-lab 检查，离屏海洋快照不包含 UI。

默认 egui／Dock 主题从原 `wgpu_pbr` 主题迁移并转换为 TOML，保留 [来源许可](../../crates/charming_egui/src/theme/default/LICENSE-wgpu_pbr.txt)。这份资源许可不代表整个项目或新 crate 已获得同一许可证。
