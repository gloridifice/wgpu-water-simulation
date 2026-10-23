# tall-cell-fluid

独立的三维不可压缩欧拉液体库，使用 Rust 2024，默认启用 **wgpu 计算后端**，另保留 f64 CPU 参考实现。提供从液体界面初始化到压力投影、时间推进、三角形表面输出的完整调用链。核心库不创建窗口；可选 `demo` feature 提供独立可视化程序。两者均不依赖主项目，不读取或保存个人配置。

它实现四叉树水平划分、自适应光学层与上下双压力采样的纵长单元思想。压力算子采用背景 MAC 网格上的 Galerkin 缩减，**不是论文专用变分离散/虚拟面结构的逐项复现**。本库不声称论文的性能或视觉精度。

## 独立可视化 demo

在仓库根目录启动：

```powershell
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo
```

默认显示水滴落入水盆；另一个场景是水柱溃坝。演示域是边长 2 米的立方体，默认每轴 16 个单元。渲染读取每个成功模拟子步后**真实 GPU 提取**的三角形水面，没有单独的动画波形。

| 操作 | 按键 |
| --- | --- |
| 播放 / 暂停 | 空格 |
| 暂停时推进一个 CFL 子步 | N |
| 重置当前场景 | R |
| 切换液滴 / 溃坝并重置时间 | 1 / 2 |
| 切换水面材质 / 三角网格 / 法线 | V |
| 环绕 / 缩放 | 鼠标左键拖动 / 滚轮 |
| 恢复相机 / 退出 | Home / Esc |

窗口使用 egui + egui_dock：中央为液体视图，右侧为耗时和控制面板；标签与分隔线可以拖动，顶部可恢复布局。面板可以播放/暂停、单步、重置、切换场景和显示方式、恢复相机。鼠标环绕与缩放仅在液体视图中生效。数值失败会暂停并显示错误，可按 R 重置。场景切换或暂停指令会在当前在途子步完成后确认。

| 耗时块 | 单位与范围 |
| --- | --- |
| 模拟耗时 | 墙钟 ms/子步，包含 CFL 步长选择、CPU 拓扑与调度、GPU 求解和同步回读等待；排除初始化与表面生成 |
| Mesh 耗时 | 墙钟 ms/次表面更新，包含显示用 halo、GPU 三角形/法线提取、回读和 CPU 整理；排除向渲染设备上传 Mesh |
| 着色耗时 | 真实 GPU timestamp，ms/渲染帧，统计水面、地面和槽框的完整场景 render pass，包含顶点处理、光栅化及片元着色；排除 egui、上传和呈现等待 |

显示最近一次完成的样本，暂停时保留模拟与 Mesh 数值；初始状态和重置后，模拟耗时显示“等待样本”。着色计时异步回读，不为计时每帧等待 GPU；设备不支持 timestamp 时显示“设备不支持”，读取失败时明确提示。三块更新频率不同，**不能将它们相加当作窗口帧耗时**。详细计时范围可悬停名称查看。

```powershell
# 溃坝；网格预览。每轴分辨率可选 8..40，提高分辨率会增加求解等待。
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --scene dam --view mesh --resolution 16

# 有限帧数的真实窗口冒烟运行。
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --frames 300

# 固定相机，推进精确 18 个子步后保存画面并退出；输出文件必须不存在。
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --steps 18 --snapshot crates/tall-cell-fluid/target/drop-step18.png

# 无窗口验证两种场景的运动、生成表面、CFL 和暂停/单步/重置协议。
cargo run --release --manifest-path crates/tall-cell-fluid/Cargo.toml --features demo --bin fluid-demo -- --verify --steps 6
```

`--paused` 可用于观察初态；`--snapshot` 未配 `--steps` 时保存第 0 步。`--steps N` 模式逐帧请求单步，避免后台最新快照跳过指定截图时刻；该模式不响应空格或面板的播放切换。所有参数见 `--help`。截图只包含场景，不含 egui 面板和操作系统标题栏。

模拟在后台线程推进，窗口独立刷新；每个子步仍存在回读与 CPU 拓扑组装，因此**模拟时间不保证与现实时间同步**。窗口和模拟分别创建设备，水面回读后上传给预览渲染器；这版以观察、检查完整流程为目的。

水面预览使用双面光照、Fresnel 天空反射与不透明材质，没有透明折射或完整体积散射。仅用于显示的隐式表面添加一层空气采样，并与物理槽域求交以封闭槽边；它不改变模拟的 phi、压力或体积。该演示独立于 Ocean 的 FFT/SWE、窗口和设置。

## 验证与调用

在仓库根目录执行：

```powershell
cargo fmt --manifest-path crates/tall-cell-fluid/Cargo.toml --check
cargo clippy --manifest-path crates/tall-cell-fluid/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml
# 实际 GPU 端到端验证：适配器不可用或任一断言失败会以非零状态结束。
cargo run --manifest-path crates/tall-cell-fluid/Cargo.toml --example gpu_verify
# 额外的 GPU 数值与边界测试（默认单元测试显式忽略这些硬件测试）。
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml --lib -- --ignored --nocapture --test-threads=1
# 不构建 wgpu 的 CPU 参考验证。
cargo test --manifest-path crates/tall-cell-fluid/Cargo.toml --no-default-features
```

crate 使用独立 workspace，不加入主程序的默认构建；未来使用方可添加本地 path 依赖。`gpu_verify` 是无窗口离线示例；`fluid-demo` 二进制仅在启用 `demo` 时构建。依赖已缓存时可在命令中加 `--offline`，不需要下载场景资源。

GPU 调用：

```rust,no_run
use tall_cell_fluid::{Domain, Grid};
use tall_cell_fluid::gpu::{self, GpuContext, GpuSimulation};
use tall_cell_fluid::surface::SurfaceLimits;

fn simulate_gpu() -> Result<(), String> {
    let context = pollster::block_on(GpuContext::new())?;
    let mut grid = Grid::new(Domain::new([16, 32, 16], 0.1)?);
    grid.set_liquid_sdf(|p| p[1] - 1.8);
    let mut simulation = GpuSimulation::from_grid(&context, grid, gpu::default_config())?;
    let reports = simulation.advance(0.05)?;
    let mesh = gpu::extract_surface(&context, &simulation.grid, SurfaceLimits::default())?;
    assert!(!reports.is_empty() && !mesh.indices.is_empty());
    Ok(())
}
```

CPU 参考调用：

```rust
use tall_cell_fluid::{Domain, Grid, Simulation, SimulationConfig};
use tall_cell_fluid::surface::extract_surface;

fn simulate() -> Result<(), Box<dyn std::error::Error>> {
    let domain = Domain::new([16, 32, 16], 0.1)?;
    let mut grid = Grid::new(domain);
    // y 为竖直方向，水面 1.8 m；phi<0 表示液体。
    grid.set_liquid_sdf(|p| p[1] - 1.8);
    let mut simulation = Simulation::from_grid(grid, SimulationConfig::default())?;
    let reports = simulation.advance(0.05)?;
    let mesh = extract_surface(&simulation.grid)?;
    assert!(!reports.is_empty());
    assert!(!mesh.indices.is_empty());
    Ok(())
}
```

可通过 `set_liquid_sdf` 初始化任意采样可解析的三维形状，例如球滴、液柱或多个形状的并集。`set_solid_sdf` 定义静态障碍物；solid_phi<0 表示固体。两者都以米为单位。初始化闭合水滴时应与计算域边缘留出采样余量。

## 数据与模块

| 模块 | 内容 |
| --- | --- |
| `grid` | 单元中心 phi、solid_phi、压力；三个独立 MAC 面速度数组；边界、采样、体积与散度诊断 |
| `adaptivity` | 水柱速度拟合误差 q、积分评价 I、L1 膨胀、光学层平滑、四叉树叶子、纵长单元、压力插值基底 |
| `pressure` | 空气零压边界、闭域压力参考、Galerkin 算子、Jacobi-PCG、速度修正、细网格回退 |
| `simulation` | CFL 子步、RK2 半拉格朗日输送、重力、空气速度外推、level-set 重建与可选体积校正 |
| `surface` | 六四面体一致分解、共享交点去重、外法线、索引三角形、输出资源上限 |
| `optics` | 反射、Snell 折射、介电 Fresnel、Beer–Lambert 吸收和简化单界面颜色组合 |
| `gpu/transport.*` | WGSL RK2 输送、速度外推、边界与重力、Jacobi Eikonal 距离重建、并行体积归约和二分校正 |
| `gpu/volume.wgsl` | 独立的两级体积归约核，屏蔽固体和最后工作组的无效单元 |
| `gpu/adaptivity.*` | WGSL q/I 评价、层厚种子、L1 膨胀与高度平滑；回读后整理 CPU 拓扑 |
| `gpu/pressure.*` | WGSL 散度/RHS、CSR 乘法、Jacobi-PCG、点积归约、真实残差、压力重建和面速度修正 |
| `gpu/surface.*` | WGSL Marching Tetrahedra、梯度法线、原子输出计数和容量检测 |
| `gpu/simulation` | 完整 GPU 子步调度、事务状态提交与 CPU 可检查快照 |
| `demo` | 独立窗口、egui 停靠面板、三阶段耗时、后台模拟、两种初始场景、表面/网格/法线预览、PNG 和运行协议验证 |

`Domain` 的尺寸是单元数，`cell_size` 是米，`origin` 是世界坐标。各字段按 x 最快、然后 y、最后 z 存储：`x + Nx * (y + Ny * z)`。三个面数组的尺寸分别为 `(Nx+1,Ny,Nz)`、`(Nx,Ny+1,Nz)`、`(Nx,Ny,Nz+1)`。

CPU 参考求解使用 `f64`，GPU 使用 `f32`；GPU 回读为 `Grid` 的 f64 字段只是转换，不会增加精度。`gpu::default_config()` 提供适合单精度的残差与散度容限；不要直接套用 CPU 默认的 `1e-9` 相对残差容限。

`SurfaceVertex` 是 `repr(C)` 的 24 字节顶点格式：`position: [f32;3]`、`normal: [f32;3]`，可对应两个 `Float32x3` 顶点属性。GPU 表面输出先使用两个 `vec4<f32>` 的 32 字节 storage 格式，读回后转换为这个顶点格式。uniform 参数使用 16 字节分组；状态、标志、CSR 下标和系数使用显式偏移的紧密数组，Rust 与 WGSL 对齐一致。

## GPU 执行边界

GPU 实际执行速度/界面更新、评价函数与光学层、压力线性迭代和速度修正、表面顶点与法线生成。CPU 负责输入校验、CFL 决策、四叉树与稀疏连接整理、`BᵀLB` 的 CSR 系数组装，以及结果诊断；没有用 CPU 压力求解器替代 GPU 失败路径。

首版保留阶段间同步回读。PCG 每轮回读收敛标量，点积采用单工作组分段归约；体积校正使用两级并行归约。它适合离线验证，**尚未做全程 GPU 驻留或实时性能优化**。管线会缓存，但各阶段仍分配临时缓冲区。`GpuContext::new` 无适配器时返回错误，不静默跳过硬件验证。

`gpu_verify` 检查 CPU/GPU q/I 一致性、不同拓扑下的光学层、闭域压力与 CPU 速度对照、GPU 细网格回退、静水多步平衡、液滴/静态固体/重建/体积、空域和 CFL 拒绝，以及表面平面/球面/容量溢出。硬件单元测试额外覆盖解析输送、斜平面距离重建、精确零值表面及球面焊接后的闭合性。

## 每个时间步

1. 校验状态与参数，确定 CFL 步长；保存旧状态并外推表面附近的空气速度。
2. 在规则背景网格上回溯输送速度和 phi，处理闭合域和静态固体。
3. 按配置重建符号距离，并可通过统一 phi 偏移校正体积估计。
4. 加重力，强制静态固体和域边界的零法向速度。
5. 使用最新速度与界面构建四叉树/光学层，形成自适应压力基底。
6. 求解缩减压力，映射到背景单元并修正 MAC 速度。
7. 检查**细网格散度**；必要时从同一临时速度重做全细网格压力投影。
8. 外推空气速度，校验有限数，提交新状态并返回诊断。表面按需提取，不强制每个子步生成。

背景网格始终规则，因此水平网格重建不承担输送；这是与论文先建层级网格再输送的顺序差异。

`step(dt)` 拒绝超过稳定上限的 dt，失败时不提交新状态；`advance(seconds)` 自动分子步。如果后续子步失败，先前成功子步保留，`time` 表示实际完成时间。

## 光学层与压力空间

局部评价 q 为六个 MAC 面的速度重建绝对差之和，单位 m/s。水柱两侧的水平速度分别按高度线性最小二乘拟合，竖直速度取平均；水柱累计 `I = sum(q) / Ny` 仍具有 m/s 单位。

`optical_scale * I` 得到以最细网格格数计的扩张距离，系数单位是格/(m/s)。下限为 `max(2*leaf_width,4)`，上限参考 `Ny/8`。对小测试域，下限限制在域高以内、上限提升到至少等于下限，避免非法区间。扩张距离向上取整。独立空中液体使用固定扩张距离，并保留细分压力表示。

扩张使用曼哈顿距离；高度平滑只增加精细区域。由细网格映射回粗叶子时采用“任一细格需要则整层保留”的保守规则，区别于论文的过半数规则。材料边界或液体连通区间不同的柱不会横向合并，也不跨空气或固体合并纵长单元。

每个液体细单元对应压力插值矩阵 B 的一行，最多有两个非负权重，其和为一。普通聚合单元用一个压力自由度，纵长单元用上下两个自由度线性插值。真正求解的是 `Bᵀ L B p = Bᵀ b`，随后用同一面算子修正速度。

缩减系统残差很小，并不意味着每个细单元的散度都小。默认启用 `fine_fallback`；无法满足 `divergence_tolerance` 时重做完整压力投影。`ProjectionReport` 分别报告缩减与最终自由度、残差、迭代次数、细网格散度和 `fallback_used`。复杂流动可能频繁回退，因此不能把自适应单元数减少当作实际加速证明。

## 表面与光学

表面提取读取 phi 的零等值面，与压力基底独立。相邻八个**单元中心**形成采样立方体，再按一致的六四面体拆分进行 Marching Tetrahedra；没有复制外部 Marching Cubes 查找表。使用一致面对角线和共享交点去重，避免相邻立方体接缝。法线由距离场梯度得到，退化时用几何法线。

输出范围从第一层到最后一层单元中心。接触计算域边界的液体不会自动封口；表面也不会按 solid_phi 自动裁切。提取自由水面时，固体遮挡应由以后接入的场景渲染处理。需要封闭导出网格或精确接触线时，应增加边界封口/固体裁切。

`extract_surface_with_limits` 在达到资源上限时返回错误，不返回截断网格。表面拓扑会随流体变化，顶点索引不保证跨帧稳定。

GPU 提取同样遇容量溢出就报错，输出为每个三角形独立三个顶点的 triangle soup，索引为连续序号；不做 GPU 顶点焊接。原子分配顺序不保证跨次调用稳定。要检查网格拓扑，应先按坐标焊接，不能直接用独立索引判断接缝。

光学模块只提供可测试的数学运算。它接收调用者提供的反射光、背景光、法线/折射率、光在水中传播的长度等数据，不提供场景追踪、屏幕空间折射、渲染pass或完整体积散射。光程不能使用模拟的光学层厚度代替。

## 当前限制

- 两个后端都保留密集背景网格；GPU 阶段之间有同步回读。demo 的模拟和 Mesh 显示包含 CPU 工作与等待的墙钟耗时，仅场景渲染使用真实 GPU timestamp。几何预览尚无完整水体光学。
- 纯网格半拉格朗日欧拉输送，没有实现 EXNBFLIP/APIC 粒子、表面张力、双向刚体耦合或持续注水源。
- 固体使用静态体素无穿透边界，没有实现论文的部分面比例和直接嵌入固体的纵长单元边界。
- 压力为背景离散上的 Galerkin 参考方案，没有逐项复制论文的最小面/虚拟面算子与 ICCG 预条件器。
- 半拉格朗日输送有数值耗散。可选体积校正只保持平滑 Heaviside 估计的**全局**体积，最多平移一个格距；不保证逐单元质量守恒或精确几何体积，报告始终返回实际结果。
- 薄于背景采样间距的液膜和液滴可能丢失。最细背景数据始终分配，压力未知量减少不等于内存减少。

## 来源

- Narita & Kanai, *Beyond Horizontal Refinement: Extending Quadtree Tall Cells with Adaptive Optical Layers*, Computer Graphics Forum 45(7), 2026. DOI: https://doi.org/10.1111/cgf.70629
- Narita & Kanai, *Adaptive Optical Layers: Efficient Tall Cell Grids for Liquid Simulation*, Computer Graphics Forum 45(2), 2026. DOI: https://doi.org/10.1111/cgf.70357

代码为独立实现，未复制论文源码或第三方表面查找表，仓库不包含论文 PDF。
