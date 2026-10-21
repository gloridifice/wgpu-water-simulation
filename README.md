> 新增三维 GPU 海洋：见 [wgpu FFT 海洋说明](README-ocean.md)，运行 `cargo run --release --features ocean --bin ocean`。

# 二维侧视 CPU PIC 水体模拟器

Rust 实现的自由表面液体模拟器。粒子可以向水平和竖直方向运动，水面可以前倾、翻折、落下和飞溅。终端显示真实粒子位置，不把它们压成单一高度曲线。

默认场景包含右侧浅滩、左侧往复挡板，以及一次局部顺时针动量脉冲，方便直接观察卷浪。求解器是纯 PIC，尚未混入 FLIP 或 APIC。

## 运行和观察卷浪

```powershell
Set-Location G:\MyProjects\Rust\water-simulation
cargo run --release
```

建议使用 Windows Terminal，窗口 120×40 或更大；最低支持 70×22。默认 112×64 网格，格宽 0.1 米，6736 个水粒子。默认播放倍率为 0.25×，让最初约 0.2～0.4 秒的翻折过程更容易观察；具体形态随物理步长和分辨率略有变化。

```powershell
# 从暂停状态开始，按 N 逐个物理步观察。
cargo run --release -- --paused

# 从静水开始，之后按 B 主动产生一次卷浪，按 W 控制挡板。
cargo run --release -- --no-pulse --no-waves

# 较低网格分辨率；改变网格也会改变水槽物理尺寸。
cargo run --release -- --nx 80 --ny 48

# 无终端运行，保存每个输出时刻的粒子轨迹；目标文件必须不存在。
cargo run --release -- --headless --time 2 --frames 40 --csv pic-frames.csv

# 保留原有的一维浅水模拟器和鼠标水面扰动。
cargo run --release -- --shallow --paused
cargo run --release -- --shallow --headless --quiet

cargo run -- --help
```

在终端中运行默认进入二维 TUI；重定向输入/输出或传入 `--headless` 后使用批处理模式。`--time / --frames / --csv / --quiet / --animate` 也会选择批处理模式。原来的 `--cells 41` 仍会选择一维浅水模式。

## 交互

| 操作 | 效果 |
| --- | --- |
| Space | 暂停 / 继续 |
| N | 暂停并推进一个 CFL 物理步 |
| R | 重置到场景初态并暂停，保留播放速度、挡板开关和网格显示 |
| W | 开关持续往复的挡板；关闭后挡板停在当前位置 |
| B | 再施加一次局部顺时针动量脉冲，推动水面翻卷 |
| 鼠标左键 | 在点击处对附近水粒子施加向上、向右的局部速度脉冲 |
| + / - | 播放速度加倍 / 减半，范围 0.0625×～4× |
| G | 显示 / 隐藏 MAC 网格 |
| Left / Right | 选择前一个 / 后一个粒子 |
| Home / End / C | 选择首个 / 最后 / 中间编号的粒子 |
| Q / Esc / Ctrl+C | 退出并恢复终端与鼠标 |

鼠标的横、纵坐标都参与二维位置映射。点击空气且附近没有水粒子时不会加水；点击边框、状态栏或坐标标签会忽略。暂停时可以施加脉冲，再单步观察。浅棕色是固体坡面，橙色是挡板，青色是水粒子，白色表示速度较快的粒子，黄色是选中的粒子。白色并非独立泡沫模型。

视图默认聚焦静水位附近，省略上方部分空白空气。粒子接近视图顶端时扩大显示范围；坐标标签和鼠标映射使用相同的物理范围，重置后恢复默认视图。

持续挡板主要产生向右传播的涌浪。B 和默认初态的卷浪脉冲是可控的外部动量输入：给坡面入口附近的水添加顺时针速度场，随后由压力、重力和粒子运动生成翻折。它不会直接弯曲水面、预设粒子轨迹或在渲染中画一个卷浪形状。这个演示不声称仅靠挡板就能稳定产生精细的海岸卷浪。

## 核心代码

```text
src/
  pic/
    mod.rs        参数、粒子、步进、造浪、鼠标脉冲、诊断
    grid.rs       MAC 交错网格、空气/水/固体、坡面与边界
    transfer.rs   粒子→网格、外推、网格→粒子、RK2 移动与碰撞
    pressure.rs   稀疏压力方程、Jacobi 预条件共轭梯度、速度投影
  tui/            二维粒子视图与交互，复用原有暂停/单步/调速/鼠标生命周期
  pic_batch.rs    二维 ASCII 与粒子 CSV 输出
  solver.rs       原有一维浅水方程核心，保持独立
  shallow_tui/    原有一维水面和速度曲线界面
  batch.rs        原有一维批处理输出
```

算法模块只依赖 Rust 标准库，不读取键盘、时钟或终端。Ratatui 与 Crossterm 只用于应用界面。库接口示例：

```rust
use water_simulation::pic::{Config, Simulation};

fn run() -> Result<(), String> {
    let mut water = Simulation::new(Config::default())?;
    water.wave_pulse();
    water.advance_to(0.3)?;
    for particle in water.particles() {
        println!("x={} y={} u={} v={}", particle.x, particle.y, particle.u, particle.v);
    }
    Ok(())
}
```

也可独立运行无界面示例：

```powershell
cargo run --release --example pic_probe -- 2 frames.csv pulse
```

## 一步 PIC 在做什么

粒子保存 `(x, y, u, v)`。与原来的 `(h, hu)` 不同，同一个 x 上可以有多个水体边界，因而能够表示浪头下方的空气空隙。

MAC 网格将水平速度 u 存在竖直面上，竖直速度 v 存在水平面上，压力 p 和单元类型存在格心。一个完整物理步为：

1. 更新挡板位置，标记固体、含水粒子的单元和空气。
2. 将粒子速度以双线性权重平均到相应的 u/v 网格面。
3. 对水附近的竖直面速度加入重力，设置墙面法向速度。
4. 求压力，修正速度，使含水单元中的净流出接近零。
5. 把有效速度向自由表面外侧外推三层，供边界附近的粒子插值。
6. 直接插值网格速度给粒子——这就是本版的纯 PIC 更新。
7. 用中点 RK2 移动粒子，处理水槽、阶梯坡面和挡板碰撞。

压力投影最重要的关系是：

```text
u_new = u_star - dt * grad(p)        密度取 1
div(u_new) ≈ 0
```

离散后，每个水单元只与四个邻居相连。`pressure.rs` 不创建稠密矩阵，而是直接计算矩阵乘向量，并使用 Jacobi 预条件共轭梯度求解。空气压力为零；固体面的法向速度由边界给定。默认最多 100 次迭代，状态栏显示求解前后的 RMS 散度和实际迭代数，达到迭代上限并不代表已经精确收敛。

CFL 根据粒子速度、挡板最大速度和重力尺度自适应选取 dt，每步还不超过 1/90 秒。TUI 使用时间累积器，积累到一个完整物理步才推进，避免刷新越快就越频繁执行 PIC 传递、增加数值耗散。每次界面循环最多做 8 个物理步；高负载时实际播放会慢于目标倍率。

## 诊断与限制

`particles` 应保持恒定：鼠标和造浪都只改变运动，不生成或删除粒子。每个粒子代表固定面积，`volume()` 返回其总和。这个守恒量不等于重建自由表面体积误差为零；有限网格和粒子采样仍会产生几何体积误差。

`overhangs` 统计同一网格列中“下方有水、中间至少两格为空气、上方又有水”的数量。它能帮助识别翻折或飞溅，但不是卷浪品质评分，也不区分独立水滴与连通的水片。

本版模拟二维侧视的水，空气忽略惯性并取零压力，包含重力和自由滑移固壁。坡面使用阶梯近似。没有空气卷入动力学、表面张力、三维泡沫或湍流模型。PIC 的速度平均会衰减旋转和细节，卷浪落下后会较快变平；这些是后续升级 FLIP 的动机。

后续 FLIP 的接入点在 `transfer.rs::grid_to_particles()`：保存压力/外力更新前的网格速度，插值得到网格速度变化，再累加到旧粒子速度中。当前版本尚未启用这条路径。

## 验证

```powershell
cargo fmt --check
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
```

新增检查包括常量速度 PIC 传递、压力投影降低散度、静水、CFL、粒子不丢失及固体边界、实际翻折指标、局部脉冲、暂停/单步/重置、不同刷新频率下的相同步进、鼠标位置映射和终端尺寸。原有一维质量守恒、对称性、稳定性及鼠标扰动测试继续运行。

原有一维算法的逐段解释保存在 [README-shallow.md](README-shallow.md)。算法背景可参考 [Matthias Müller 的 PIC/FLIP 讲义](https://matthias-research.github.io/pages/tenMinutePhysics/18-flip.pdf) 和 [Bridson 的流体模拟课程](https://www.cs.ubc.ca/~rbridson/fluidsimulation/)。

