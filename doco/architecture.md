# 当前架构

本文记录当前代码已实现的边界；未实现方案保存在变更目录，不视为当前功能。

## 程序入口

- Rust 2024 项目，默认启用 `ocean` feature；`cargo run` 启动 GPU Ocean。
- `src/bin/ocean.rs` 提供 GPU 海洋入口、验证与预览参数。
- `src/main.rs`、`src/pic/`、`src/tui/` 保留 CPU 二维 PIC；`src/solver.rs`、`src/shallow_tui/` 提供一维浅水模式。不启用 `ocean` 时仍需可用。

## GPU 海洋

- `src/ocean/spectrum.rs` 与 `gpu.rs::Ocean`：波谱初始化、GPU 演化、逆 FFT，以及向近岸提供过滤后的波浪输入。
- `src/ocean/shore.rs::Shore` 与 `shore*.wgsl`：局部二维 SWE、一阶/二阶求解、水面泡沫输送和沙滩湿润历史。耦合方向为 FFT → SWE。
- `Shore::encode` 按稳定步长拆分子步；二阶有预测与校正阶段。`simulated_dt` 是本次实际推进量，长帧可能被限制，不能直接用墙钟时间替代。
- `src/ocean/foam.rs`、`foam.wgsl`：远海泡沫与当前位移雅可比，独立于近岸泡沫。
- `src/ocean/spray.rs` 与 `spray*.wgsl`：近岸纹理浪花。Shore 持有 `SpraySource`，每完成 SWE 子步向独立网格 storage buffer 累积发射信号；Renderer 持有 `SprayParticles`，消费信号，维护 GPU 余量、双粒子列表和间接绘制参数。粒子不反作用水体或泡沫，运行时不回读信号/粒子计数。配置放在 `OpticsConfig::spray`，默认关闭。
- `src/ocean/viewer.rs`：窗口、相机、配置应用和帧调度；正常帧先计算 FFT、远海泡沫和 SWE，再编码粒子更新，最后调用场景、浪花和 GUI 绘制。快照暖机同样推进粒子。
- `gpu.rs::Renderer` 持有 `Shore`，组织天空、海底、模型与水面绘制。水面写入 `Depth32Float` 深度；主深度具有 `RENDER_ATTACHMENT | TEXTURE_BINDING` 用途，场景 pass 保存深度，后续浪花 pass 只采样，不把该深度同时挂为可写附件。
- `render.wgsl` 的近岸 NSH 泡沫保留双相位孔隙覆盖，使用原始面密度的指数响应控制连续不透明度；`ShoreConfig::foam_opacity_scale` 仅控制外观，不修改密度／动量或重置历史。原灰度和远海泡沫路径独立。
- `optics*`、`seabed*`、`skylight*`、`render.wgsl`：光学参数、水底、天空照明与水面材质。`terrain*`、`collider*`、`model*` 提供地形、碰撞物与导入模型支持。

## 界面、配置与验证

- `gui.rs` 使用 egui_dock，`spray_panel.rs` 提供独立浪花页签；`settings.rs` 管理当前会话与命名配置。`SavedSettings` 使用默认值兼容缺失字段，读取路径由 `default_path()` 决定。
- `verify.rs`、`*_verify.rs` 和各模块验证入口负责 GPU 行为检查。`gpu_timer.rs` 使用设备支持的 GPU timestamp；功能运行不能依赖同步回读。
- 配置、特性边界、参数面板和资源许可约束见根目录 `AGENTS.md`；使用方式见 `README.md`、`README-ocean.md`、`README-shallow.md`。

浪花合约见 [specs/gpu-spray.md](specs/gpu-spray.md)。首版只在纯 SWE 区域发射；池满丢弃新增整数请求，不积欠。暂停保留最后信号供调试，但模拟步长为零、不重复消费。源计算包含在 SWE GPU 时间戳区间内，粒子更新/绘制不包含在该指标中。GPU 行为和软交界验证位于 `spray_verify.rs`；纹理来源见 `assets/spray/README.md`。
