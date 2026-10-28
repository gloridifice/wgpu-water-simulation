# 当前架构

本文记录当前代码已实现的边界；未实现方案保存在变更目录，不视为当前功能。

## 程序入口

- Rust 2024 workspace，根包 `water-simulation` 默认启用 `ocean` feature；默认成员只有根包，`cargo run` 启动 GPU Ocean。
- `src/bin/ocean.rs` 提供 GPU 海洋入口、验证与预览参数。
- `src/bin/foam_lab.rs` 提供独立离线泡沫输送对照：默认诊断入口调用生产泡沫通量／源项／纹理运动，在固定解析水流下输出密度快照及误差；`foam_lab/benchmark.rs` 的 `--benchmark` 模式则调用生产 SWE 管线，在固定稀疏／密集／模型地形／障碍物输入下预热并重复计时、导出单步状态。两者均不读写个人配置，见 [泡沫诊断与基准测试](guides/foam-lab.md)。
- `crates/tui-2d-prototype/` 是独立 CPU 包：`src/main.rs` 统一选择二维 PIC／一维浅水及 TUI／批处理，`src/pic/`、`src/solver.rs` 提供不依赖终端的求解器，`src/tui/`、`src/shallow_tui/` 负责交互。测试与 `pic_probe` 示例归该包所有，Cargo 包名为 `tui-2d-prototype`，库名为 `tui_2d_prototype`，二进制仍名为 `water-simulation`；从根目录用 `cargo run -p tui-2d-prototype` 启动。
- 根包不依赖 CPU 包、Ratatui 或 Crossterm，两包共用根 `Cargo.lock` 和构建目录；`cargo test --workspace` 覆盖两包。`crates/tall-cell-fluid/` 保持独立 workspace，不纳入此工作区。

## GPU 海洋

`src/ocean/` 按职责分为 `app/`（窗口与快照）、`gpu/`（设备与计时）、`parameters/`（参数默认值与校验）、`simulation/`（远海、近岸、浪花）、`scene/`（模型、地形、碰撞体）、`render/`（场景绘制、光学、纹理）、`ui/`（面板和布局）、`settings/`（会话与命名库）、`labs/`（实验）。`camera.rs` 放共享相机和 CameraUniform；不依赖窗口。参数层不依赖应用、界面或 GPU 资源。`gpu/context.rs` 不引用模拟或渲染。

`ocean/mod.rs` 保留原 `shore`、`foam`、`spectrum`、`spray`、`model`、`terrain`、`collider`、`optics`、`skylight`、`viewer`、`foam_weight_lab` 公开路径；`gpu/mod.rs` 保留 Gpu、Ocean、Renderer、CameraUniform、Error 的重导出。兼容入口不持有第二套实现；内部使用实际职责路径。原天空深度图入口仍由兼容门面导出，图表实现归 `ui/panels/optics.rs`，天空计算不依赖 GUI。下文 Rust 简写路径相对于 `src/ocean/`。

- WGSL 文件归所属模块的 `shaders/` 子目录：FFT/远海泡沫在 `simulation/offshore/`，SWE 公共定义/计算/读取在 `simulation/nearshore/`，浪花在 `simulation/spray/`，绘制与光学在 `render/`，颜色 mip 在 `render/textures/`，泡沫权重实验在 `labs/foam_weight_lab/`。离线输送诊断保留 `src/bin/foam_lab/shaders/`；独立液体库分别使用 `src/gpu/shaders/` 和 `src/demo/shaders/`。Rust 相对源文件使用 `include_str!` / `include_bytes!` 编译期嵌入；跨模块引用唯一原件，保留字节内容和拼接顺序，不依赖运行目录或外部着色器文件。
- `simulation/offshore/spectrum.rs` 与 `fft.rs::Ocean`：波谱初始化、GPU 演化、逆 FFT，以及向近岸提供过滤后的波浪输入。
- `simulation/nearshore/mod.rs::Shore` 与所属 `shore*.wgsl`：局部二维 SWE、一阶/二阶求解、水面泡沫输送和沙滩湿润历史。耦合方向为 FFT → SWE；`uniform.rs` 保留原 CPU → WGSL 布局。
- 默认单浓度模式的近岸泡沫量与动量共用守恒 Rusanov 通量；`foam_transport_order` 默认 1，在二阶 SWE 下可启用受限 MUSCL 空间重构。密度与动量共享正性／速度约束，干湿、障碍物和域边缘局部退回一阶；不改变源项、拖曳或材质。`foam-lab` 独立选择空间／时间阶数并用相同 CFL 对照，设备支持时记录 GPU timestamp。
- MUSCL 泡沫按轴复用五格邻域的水深、密度、动量和地形阻挡，中心斜率供两侧共用，不增加生产网格缓冲区或计算 pass。`Shore::solver_pipeline` 按水体／泡沫阶数惰性编译并缓存三个共享绑定布局的管线，用 WGSL override 去掉未启用的求解分支；首次使用可能有编译开销。诊断入口可保留 uniform 动态选择。
- 近岸源项保留旧前沿／背景路径；默认关闭的 `foam_ridge_enabled` 实验在同一 SWE 阶段沿局部流向读取五点速度，只向局部压缩峰窄带发射，并禁用背景源。它不是 FFT/J 调制，也不改动水动力、输送、拖曳、衰减或材质。`foam_emission_enabled` 停发但不清空历史；状态第三块的 x/y 在实验或诊断视图下存最近子步实际源率／权重，z/w 保持水面高程／速度含义。`simulation/nearshore/verification/foam_ridge.rs` 通过完整 GPU 求解调度验证窄带、切向连通、矩形网格／旋转、干湿保护、两阶注入和停发后的守恒输送。尚无自动波前追踪／滞回，不保证网状形态。
- `foam_dual_phase` 是默认关闭的双浓度实验，和默认惯性／守恒路径互斥。现有密度槽保存新鲜 N + 水面残余 R，第五块 w 保存 R，五块 storage 大小不变；R 不属于沙地。开启后两者共用 SWE 粗速度与半拉格朗日回溯，并同时应用面积压缩／膨胀响应，不再叠加旧通量、拖曳或单浓度衰减。所有现有源只直接注入 N，每个完整子步联合解析积分生成、N→R 转化与独立衰减；默认半衰期 1.4／9 模拟秒，退出转化比例 0.65。切换模式重置 SWE 历史，寿命参数实时更新；暂停、干湿、重置和碰撞体编辑覆盖 R。`simulation/nearshore/verification/foam_dual.rs` 检查实际 GPU 解析解、两阶调度和输运／边界行为。此 SL 路径不承诺严格质量守恒或整体帧率无关。
- `Shore::encode` 按稳定步长拆分子步；二阶有预测与校正阶段。`simulated_dt` 是本次实际推进量，长帧可能被限制，不能直接用墙钟时间替代。
- `simulation/offshore/foam.rs` 与所属 `foam.wgsl`：远海泡沫与当前位移雅可比，独立于近岸泡沫。
- `simulation/spray/mod.rs` 与所属 `spray*.wgsl`：近岸纹理浪花。Shore 持有 `SpraySource`，每完成 SWE 子步向独立网格 storage buffer 累积发射信号；Renderer 持有 `SprayParticles`，消费信号，维护 GPU 余量、双粒子列表和间接绘制参数。粒子不反作用水体或泡沫，运行时不回读信号/粒子计数。配置放在 `OpticsConfig::spray`，默认关闭。
- `app/viewer.rs`：窗口、配置应用和帧调度；正常帧先计算 FFT、远海泡沫和 SWE，再编码粒子更新，最后调用场景、浪花和 GUI 绘制。`app/snapshot.rs` 保留离屏暖机与调度，同样推进粒子；两条路径没有合并或重排。
- `render/renderer.rs::Renderer` 持有 `Shore`，组织天空、海底、模型与水面绘制。水面写入 `Depth32Float` 深度；主深度具有 `RENDER_ATTACHMENT | TEXTURE_BINDING` 用途，场景 pass 保存深度，后续浪花 pass 只采样，不把该深度同时挂为可写附件。
- 近岸材质坐标跟随泡沫输运速度，双相位重置周期由 `foam_texture_period` 控制（默认 2 模拟秒，0 为持续输运）；只重置不可见的坐标层，不修改密度／动量。`foam_prototype` 默认关闭，开启后在输运坐标上使用三尺度 value noise 和密度阈值显示孔洞，沿用现有光学、颜色和密度尺度参数；不使用 NSH 微法线。材质调试 5 显示输运坐标网格。此阶段没有新增细波传播、撞岩源或高分辨率泡沫场。
- 双浓度实验开启时，`render.wgsl` 使用共享输运坐标显示两尺度三角格梯度噪声的亮白 N 与多尺度窄带、稍暗的 R；N 的细噪声层旋转并错开坐标，减少稀疏白沫的方格排列，不改变密度响应或模拟，覆盖原 NSH／原型选择；仍使用同一水面几何，没有细尺度湍流或单气泡。密度调试 6／9／10 分别读取 N+R／N／R，不经过材质阈值；双坐标重置周期继续有效。参数及启用方式见 [Ocean 使用说明](guides/ocean.md)。
- `render.wgsl` 的近岸 NSH 泡沫保留双相位孔隙覆盖，使用原始面密度的指数响应控制连续不透明度；`ShoreConfig::foam_opacity_scale` 仅控制外观，不修改密度／动量或重置历史。原灰度和远海泡沫路径独立。
- `parameters/` 保留原参数类型名称、字段、默认值及校验。`OpticsConfig` 仍是既有场景参数聚合，不改变序列化结构。`render/{optics,skylight,seabed,model}.rs` 与所属着色器负责光学、水底、天空与模型绘制；`scene/{terrain,collider,model}.rs` 提供地形、碰撞几何与导入模型数据。
- `render/textures/sand.rs` 与所属 `color_mips.wgsl`：沙地和导入模型颜色贴图在 CPU 解码 JPEG、上传原始层后，通过 GPU render pass 在线性空间生成完整 sRGB mip 链，不同步等待 GPU；每级写回 8 位 sRGB。沙地法线／标量贴图保留 CPU 过滤。`sand::verify` 回读实际 GPU mip，检查线性光平均及矩形纹理边界。

## 界面、配置与验证

- `ui/gui.rs` 使用 egui_dock，`ui/panels/` 按职责拆分面板，共用 `ui/controls.rs` 的网格和控件；`ui/workspace/{mod,store}.rs` 管理工作区与独立布局 sidecar。`settings/{session,library,store}.rs` 分别管理当前参数快照、命名库与加载/原子保存/自动保存。`SavedSettings` 使用默认值兼容缺失字段，读取路径由 `default_path()` 决定：当前工作目录已有 `configs/ocean-settings.json` 时使用该 Git 跟踪会话，否则回退原用户目录；显式 `--settings` 优先。`--config` 严格只读加载顶层会话，用于窗口和快照，不自动保存或替换坏文件。快照保留指定相机／远海泡沫参数，支持模拟时间发射窗口；均不改写配置。原会话、命名库及派生礁石实验配置见 [../configs/README.md](../configs/README.md)。
- `verify.rs` 只组织完整 GPU 行为检查；DFT/耦合、近岸、浪花及渲染验证归各自 `verification.rs` 或 `verification/`，仍编译到非测试程序供 CLI 调用，不通过暴露私有字段来迁移。`gpu/timer.rs` 使用设备支持的 GPU timestamp；功能运行不能依赖同步回读。
- 配置、特性边界、参数面板和资源许可约束见根目录 `AGENTS.md`；使用与算法指南集中在 `doco/guides/`，包括 [Ocean](guides/ocean.md)、[CPU PIC](guides/pic.md)、[一维浅水](guides/shallow.md) 和 [泡沫诊断](guides/foam-lab.md)。根目录 `README.md` 提供项目入口，`doco/specs/` 保留精确行为合约。

浪花合约见 [specs/gpu-spray.md](specs/gpu-spray.md)。首版只在纯 SWE 区域发射；池满丢弃新增整数请求，不积欠。暂停保留最后信号供调试，但模拟步长为零、不重复消费。源计算包含在 SWE GPU 时间戳区间内，粒子更新/绘制不包含在该指标中。GPU 行为和软交界验证位于 `simulation/spray/verification.rs`；纹理来源见 `assets/spray/README.md`。
