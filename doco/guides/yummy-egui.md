# yummy_egui 接入与默认主题

通用控件使用 crates.io 的 [`yummy_egui`](https://crates.io/crates/yummy_egui) 0.1 系列，最低版本 0.1.1。本仓库不再维护组件库源码、主题 TOML 或外部主题加载／保存入口。

## 依赖与边界

- `yummy_egui` 0.1.1 使用 egui 0.34.3、可选 egui_dock 0.19.1，需要 Rust 1.92 或更新版本。
- 宿主同步使用 egui／egui_extras／egui-winit／egui-wgpu 0.34.3、egui_dock 0.19.1 和 wgpu 29，避免不同版本的 UI 或 GPU 类型混用。
- 根包只在 `ocean` feature 下启用 `yummy_egui`，并开启库的 `dock` feature。独立 `crates/tall-cell-fluid` workspace 仅在 `demo` feature 下启用该 UI 依赖。
- CPU 包 `tui-2d-prototype` 不依赖 egui、yummy_egui 或 GPU；根 workspace 只有根包和 CPU 包。
- 配置选中状态、载入失败回滚、模拟参数保存及 Dock 布局均归宿主，不由组件库管理。

## 默认主题

Ocean、slider-lab、foam-weight-lab 和 tall-cell-fluid demo 直接应用库默认主题：

```rust
let theme = yummy_egui::Theme::default();
theme.apply(&context);
// 由宿主安装字体；theme.apply 不会替换字体。
let dock_style = theme.dock_style().clone();
// 创建 DockArea 时传入 .style(dock_style.clone())。
```

主题资源随 crates.io 依赖提供，无需从本仓库或运行目录读取文件。不复制库内主题，也不保存主题配置。Ocean 仍由 `src/ocean/ui/fonts.rs` 安装 MiSans，液体 demo 独立安装同一字体。

“不保存主题”不影响现有模拟会话、命名配置和 `.workspaces.json` 布局 sidecar。Dock 边距调试及 slider-lab 外观调整仅保留在运行内存中，退出即丢弃，不会写回库默认主题。

## 控件与文档

业务面板直接调用 `yummy_egui::ParameterSlider`、`ParameterList`、`SelectionList`、`PanelButton`、`group`、`parameter`、`choices` 和 h1／h2／h3。逻辑参数组共用两列网格，保留 42%／58% 列宽和名称 hover 提示；选择列表只返回交互响应，业务状态仍由调用方提交。

库 API 与主题实现见 [docs.rs](https://docs.rs/yummy_egui) 和[上游仓库](https://github.com/gloridifice/yummy-egui)，不在本项目维护第二份接口手册。

## 验证

```powershell
cargo test --features ocean --lib
cargo clippy --features ocean --all-targets -- -D warnings
cargo test -p tui-2d-prototype
cargo check --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --all-targets
```

组件自身测试由上游库维护；宿主保留配置业务、Dock 布局与相机交互测试。升级 GPU 后端时还需构建并运行 `ocean --verify`，真实界面通过有限帧窗口检查，不能用不含 UI 的海洋快照替代。

若已有 Windows 锁文件将 `gpu-allocator` 的宽版本依赖解析到旧 `windows`，而 wgpu 29 使用 `windows` 0.62，可能出现 Direct3D12 类型不匹配。应在受影响的 workspace 中更新旧版本，例如 `cargo update -p windows@0.58.0 --precise 0.62.2`；不要修改 Cargo 缓存中的第三方源码或关闭图形后端来绕过。

`yummy_egui` 的 MIT 许可证与主题来源声明由上游包维护；它不改变本项目或其他随附资源的许可。
