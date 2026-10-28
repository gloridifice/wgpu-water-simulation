# 当前架构

本文记录当前代码已实现的边界；未实现方案保存在变更目录，不视为当前功能。

## 程序入口

- Rust 2024 项目，默认启用 `ocean` feature；`cargo run` 启动 GPU Ocean。
- `src/bin/ocean.rs` 提供 GPU 海洋入口、验证与预览参数。
- `src/bin/foam_lab.rs` 提供独立离线泡沫输送对照：默认诊断入口调用生产泡沫通量／源项／纹理运动，在固定解析水流下输出密度快照及误差；`foam_lab/benchmark.rs` 的 `--benchmark` 模式则调用生产 SWE 管线，在固定稀疏／密集／模型地形／障碍物输入下预热并重复计时、导出单步状态。两者均不读写个人配置，见 [../docs/foam-lab.md](../docs/foam-lab.md)。
- `src/main.rs`、`src/pic/`、`src/tui/` 保留 CPU 二维 PIC；`src/solver.rs`、`src/shallow_tui/` 提供一维浅水模式。不启用 `ocean` 时仍需可用。

## GPU 海洋

- `src/ocean/spectrum.rs` 与 `gpu.rs::Ocean`：波谱初始化、GPU 演化、逆 FFT，以及向近岸提供过滤后的波浪输入。
- `src/ocean/shore.rs::Shore` 与 `shore*.wgsl`：局部二维 SWE、一阶/二阶求解、水面泡沫输送和沙滩湿润历史。耦合方向为 FFT → SWE。
- 默认单浓度模式的近岸泡沫量与动量共用守恒 Rusanov 通量；`foam_transport_order` 默认 1，在二阶 SWE 下可启用受限 MUSCL 空间重构。密度与动量共享正性／速度约束，干湿、障碍物和域边缘局部退回一阶；不改变源项、拖曳或材质。`foam-lab` 独立选择空间／时间阶数并用相同 CFL 对照，设备支持时记录 GPU timestamp。
- MUSCL 泡沫按轴复用五格邻域的水深、密度、动量和地形阻挡，中心斜率供两侧共用，不增加生产网格缓冲区或计算 pass。`Shore::solver_pipeline` 按水体／泡沫阶数惰性编译并缓存三个共享绑定布局的管线，用 WGSL override 去掉未启用的求解分支；首次使用可能有编译开销。诊断入口可保留 uniform 动态选择。
- 近岸源项保留旧前沿／背景路径；默认关闭的 `foam_ridge_enabled` 实验在同一 SWE 阶段沿局部流向读取五点速度，只向局部压缩峰窄带发射，并禁用背景源。它不是 FFT/J 调制，也不改动水动力、输送、拖曳、衰减或材质。`foam_emission_enabled` 停发但不清空历史；状态第三块的 x/y 在实验或诊断视图下存最近子步实际源率／权重，z/w 保持水面高程／速度含义。`foam_ridge_verify.rs` 通过完整 GPU 求解调度验证窄带、切向连通、矩形网格／旋转、干湿保护、两阶注入和停发后的守恒输送。尚无自动波前追踪／滞回，不保证网状形态。
- `foam_dual_phase` 是默认关闭的双浓度实验，和默认惯性／守恒路径互斥。现有密度槽保存新鲜 N + 水面残余 R，第五块 w 保存 R，五块 storage 大小不变；R 不属于沙地。开启后两者共用 SWE 粗速度与半拉格朗日回溯，并同时应用面积压缩／膨胀响应，不再叠加旧通量、拖曳或单浓度衰减。所有现有源只直接注入 N，每个完整子步联合解析积分生成、N→R 转化与独立衰减；默认半衰期 1.4／9 模拟秒，退出转化比例 0.65。切换模式重置 SWE 历史，寿命参数实时更新；暂停、干湿、重置和碰撞体编辑覆盖 R。`foam_dual_verify.rs` 检查实际 GPU 解析解、两阶调度和输运／边界行为。此 SL 路径不承诺严格质量守恒或整体帧率无关。
- `Shore::encode` 按稳定步长拆分子步；二阶有预测与校正阶段。`simulated_dt` 是本次实际推进量，长帧可能被限制，不能直接用墙钟时间替代。
- `src/ocean/foam.rs`、`foam.wgsl`：远海泡沫与当前位移雅可比，独立于近岸泡沫。
- `src/ocean/spray.rs` 与 `spray*.wgsl`：近岸纹理浪花。Shore 持有 `SpraySource`，每完成 SWE 子步向独立网格 storage buffer 累积发射信号；Renderer 持有 `SprayParticles`，消费信号，维护 GPU 余量、双粒子列表和间接绘制参数。粒子不反作用水体或泡沫，运行时不回读信号/粒子计数。配置放在 `OpticsConfig::spray`，默认关闭。
- `src/ocean/viewer.rs`：窗口、相机、配置应用和帧调度；正常帧先计算 FFT、远海泡沫和 SWE，再编码粒子更新，最后调用场景、浪花和 GUI 绘制。快照暖机同样推进粒子。
- `gpu.rs::Renderer` 持有 `Shore`，组织天空、海底、模型与水面绘制。水面写入 `Depth32Float` 深度；主深度具有 `RENDER_ATTACHMENT | TEXTURE_BINDING` 用途，场景 pass 保存深度，后续浪花 pass 只采样，不把该深度同时挂为可写附件。
- 近岸材质坐标跟随泡沫输运速度，双相位重置周期由 `foam_texture_period` 控制（默认 2 模拟秒，0 为持续输运）；只重置不可见的坐标层，不修改密度／动量。`foam_prototype` 默认关闭，开启后在输运坐标上使用三尺度 value noise 和密度阈值显示孔洞，沿用现有光学、颜色和密度尺度参数；不使用 NSH 微法线。材质调试 5 显示输运坐标网格。此阶段没有新增细波传播、撞岩源或高分辨率泡沫场。
- 双浓度实验开启时，`render.wgsl` 使用共享输运坐标显示两尺度三角格梯度噪声的亮白 N 与多尺度窄带、稍暗的 R；N 的细噪声层旋转并错开坐标，减少稀疏白沫的方格排列，不改变密度响应或模拟，覆盖原 NSH／原型选择；仍使用同一水面几何，没有细尺度湍流或单气泡。密度调试 6／9／10 分别读取 N+R／N／R，不经过材质阈值；双坐标重置周期继续有效。参数及启用方式见 [../docs/ocean.md](../docs/ocean.md)。
- `render.wgsl` 的近岸 NSH 泡沫保留双相位孔隙覆盖，使用原始面密度的指数响应控制连续不透明度；`ShoreConfig::foam_opacity_scale` 仅控制外观，不修改密度／动量或重置历史。原灰度和远海泡沫路径独立。
- `optics*`、`seabed*`、`skylight*`、`render.wgsl`：光学参数、水底、天空照明与水面材质。`terrain*`、`collider*`、`model*` 提供地形、碰撞物与导入模型支持。
- `sand.rs` 与 `color_mips.wgsl`：沙地和导入模型颜色贴图在 CPU 解码 JPEG、上传原始层后，通过 GPU render pass 在线性空间生成完整 sRGB mip 链，不同步等待 GPU；每级写回 8 位 sRGB。沙地法线／标量贴图保留 CPU 过滤。`sand::verify` 回读实际 GPU mip，检查线性光平均及矩形纹理边界。

## 界面、配置与验证

- `gui.rs` 使用 egui_dock，`spray_panel.rs` 提供独立浪花页签；`settings.rs` 管理当前会话与命名配置。`SavedSettings` 使用默认值兼容缺失字段，读取路径由 `default_path()` 决定：当前工作目录已有 `configs/ocean-settings.json` 时使用该 Git 跟踪会话，否则回退原用户目录；显式 `--settings` 优先。`--config` 严格只读加载顶层会话，用于窗口和快照，不自动保存或替换坏文件。快照保留指定相机／远海泡沫参数，支持模拟时间发射窗口；均不改写配置。原会话、命名库及派生礁石实验配置见 [../configs/README.md](../configs/README.md)。
- `verify.rs`、`*_verify.rs` 和各模块验证入口负责 GPU 行为检查。`gpu_timer.rs` 使用设备支持的 GPU timestamp；功能运行不能依赖同步回读。
- 配置、特性边界、参数面板和资源许可约束见根目录 `AGENTS.md`；使用方式见 `README.md`、`docs/ocean.md`、`docs/shallow.md`。

浪花合约见 [specs/gpu-spray.md](specs/gpu-spray.md)。首版只在纯 SWE 区域发射；池满丢弃新增整数请求，不积欠。暂停保留最后信号供调试，但模拟步长为零、不重复消费。源计算包含在 SWE GPU 时间戳区间内，粒子更新/绘制不包含在该指标中。GPU 行为和软交界验证位于 `spray_verify.rs`；纹理来源见 `assets/spray/README.md`。
