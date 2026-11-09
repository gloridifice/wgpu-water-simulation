# Fog 导航图标

来源：[Lucide](https://github.com/lucide-icons/lucide/tree/main/icons)，原始 SVG 未修改；许可证见同目录 `LICENSE`。

- `haze.svg`：模拟
- `palette.svg`：外观
- `sliders-horizontal.svg`：配置
- `panel-left-open.svg`：添加面板
- `rotate-ccw.svg`：重排布局

编译期嵌入；应用初始化时将 `currentColor` 转为白色，再由 egui 按主题着色。SVG 加载器缓存光栅结果，无运行时网络请求。
