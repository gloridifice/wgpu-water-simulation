# Implementation design

## 1. Baseline and goals

基线提交 a49d651；工作区已有 [个人设置](../../../../../configs/ocean-settings.json) 用户修改，仅读取，不覆盖。当前配置为 256² 二阶、模型地形、双浓度泡沫和浪花开启。既有四场景基准不覆盖双浓度、造泡和非零 FFT，不能直接代表此配置。

主要入口：
- [shore_compute.wgsl](../../../../../src/ocean/simulation/nearshore/shaders/shore_compute.wgsl)：`reconstruct`、`face`、`bed_source`、`bed` 和生产 `main`。
- [shore.wgsl](../../../../../src/ocean/simulation/nearshore/shaders/shore.wgsl)：共享地形、碰撞体查询。
- [benchmark.rs](../../../../../src/bin/foam_lab/benchmark.rs)：固定输入生产内核基准。
- [terrain.rs](../../../../../src/ocean/simulation/nearshore/verification/terrain.rs)：真实上传地形／静水／域编辑验证。

`Shore::solver_pipeline` 已按阶数缓存三个 override 变体，泡沫 MUSCL 已复用轴向邻域；不重复实施。`Shore::encode` 的 RK2 两阶段、状态复制及异步计时不变。

已检查重叠：`add-gpu-spray-particles` 的源 pass 与状态读取保持原状；`reorganize-ocean-modules` 已交付，沿用新职责路径；工作区 UI 变更无代码重叠。

## 2. Overall approach

先扩充现有 benchmark 的一个固定模型双浓度＋入射波场景并保存旧内核结果，保持原四场景输入不变。随后逐项修改着色器，分别构建、计时和比较输出。最后执行完整生产 GPU 验证及静态检查。

只在现有函数附近抽取实际复用的数据，不增加生产缓冲区、pass、trait、配置开关或测试专用生产入口。不把旧内核保留为运行时回退；旧二进制／结果只留在忽略的 target 中供 A/B。

## 3. APIs and data model

所有 Rust 公共 API、uniform/storage 布局、五块状态及绑定不变。

WGSL 内部拟使用一个单元值结构，持有坐标、原水深、水面／速度及床面。`reconstruct` 接受已加载的左中右单元，`face` 接受两份重构和方向；轴向函数返回该轴的外向通量和中心床面源项。两方向结果沿用 main 原有算术顺序，不把源项折入通量导致额外舍入差。

将共享 `bottom_height` 内的碰撞体抬高部分抽为接收基础高度的辅助函数，供世界坐标路径与整数床面路径共用。整数路径 clamp 幽灵格到有效格，在模型启用时直接取 `terrain_cell` 高度；解析沙滩仍按原世界坐标公式。碰撞体编辑也使用相同的单元中心床面，避免求解与编辑读取两个略有差别的床面。

没有新持久状态，因此不引入失效缓存、生命周期钩子或回读。资源错误与设备不支持 timestamp 时的既有行为不变。

## 4. Algorithms and rules

### 重构复用

- 每轴读取 c±1、c±2，中心值在 X/Z 间共享；每轴计算左、中心、右三份重构。
- 保留 theta=1.3 限制器、边缘坐标判定、0.02 m 邻域干湿退阶和 0.005 m 面深保护，顺序不变。
- 正、负面的外向通量分别调用原静水重构公式；保留单侧压力修正，不以一个反号通量替代两侧源项。
- 中心床面源项使用同一份中心重构。一阶继续调用原一阶通量，不额外加载五格邻域。
- 不新增 NaN 过滤、任意密度上限或稳态／干格捷径。

### 整数床面

- 单元中心直接读取已上传模型网格；保留幽灵格 clamp 和碰撞体最高顶面语义。
- `model_terrain` 的任意坐标插值与渲染行为不变，不增加有效床面缓存。
- 扩展现有地形生命周期验证中的域编辑为非二进制精确的原点／尺寸，继续用独立解析斜床检查，覆盖原来整数尺寸不能揭示的回映射误差。

### FFT 区域裁剪

- 重置路径不变。正常 main 只有 `edge>0` 时执行入射波采样及松弛操作；后续泡沫仍使用原 edge。
- 不修改 `incident`、FFT 前后场时间插值、过滤、增益或限幅顺序。不针对零振幅添加额外特判。

## 5. Fixed decisions and discretion

固定：首批三项、原数值法和 ABI、原步长／阶段、不新增缓存和运行时选择。没有待决核心问题。

可自行选择局部结构／函数名、基准场景构造细节和测量批次。某项若没有可重复收益，可依据独立 A/B 撤回该项代码并记录结果，不堆叠分支补救；数值行为不符时先定位，不放宽合约。

后续阶段专用化、状态轮换、共享内存、自适应 CFL 不在本变更范围。

## 6. Verification and documentation impact

- 基线及各变体：release `foam-lab --benchmark --resolutions 256,512`，每次新输出目录。原四场景输入／地形指纹应一致，新场景在本轮 A/B 间一致；新增记录完整 uniform 和 FFT 输入指纹，使不同强迫条件不能误当相同输入。
- 逐格比较五块输出，先报告 bitwise 一致性，再报告绝对误差与 绝对差除以 max(1, abs(基线值))；固定单步缩放误差上限 5e-5。此比较是重排误差检查，不代替物理验证。
- GPU：`ocean --verify-terrain` 后运行完整 `ocean --verify`，包含两阶、泡沫、FFT、碰撞体、浪花和渲染消费端。新测试仅扩展现有地形场景；不新增复述 helper 的源码测试。
- `cargo fmt --all --check`、`cargo clippy --features ocean --all-targets -- -D warnings`、`cargo test --features ocean --lib`。
- 浪花与状态复制代码不改，固定输入内核基准仍不包含其成本；不宣称已完成个人实景性能归因。
- 更新 [当前架构](../../../../architecture.md) 的水体重构／床面读取事实及 [foam-lab 指南](../../../../guides/foam-lab.md)、[Ocean 指南](../../../../guides/ocean.md) 的实现和基准说明。既有浪花 specs 行为不变，不增加重复数值规范或 ADR。
