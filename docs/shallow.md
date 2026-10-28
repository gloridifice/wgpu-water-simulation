# 一维 CPU 浅水模拟器

> 以下保留一维版本的说明。`cargo run` 默认启动 GPU Ocean；本页命令指定 CPU 程序 `--no-default-features --bin water-simulation`，并通过 `--shallow` 选择一维模式。这里提到的 `src/tui/` 现位于 `src/shallow_tui/`，求解核心仍是 `src/solver.rs`。

可交互的一维 CPU 浅水模拟器。中央水位凸起分成左右两列波，遇到两端的墙后反射回来。核心算法只使用 Rust 标准库，界面使用 Ratatui 与 Crossterm。

## 直接运行

在 PowerShell 中：

```powershell
Set-Location G:\MyProjects\Rust\water-simulation
cargo test --no-default-features
cargo run --no-default-features --bin water-simulation -- --shallow
```

在交互终端中，默认启动 TUI 并以 1 倍速持续模拟，按 Q 退出。建议使用 Windows Terminal，终端至少 70 列 × 22 行，120 × 36 时曲线更清楚。重定向输入或输出时自动使用批处理模式，也可以显式传入 `--headless`。

```powershell
# 从暂停状态开始，用 N 逐个 CFL 时间步观察。
cargo run --no-default-features --bin water-simulation -- --shallow --paused --cells 41

# 优化构建启动交互界面。
cargo run --release --no-default-features --bin water-simulation -- --shallow

# 只输出最终统计；CSV 保存初态及每个输出时刻的所有网格。
cargo run --no-default-features --bin water-simulation -- --shallow --headless --quiet --csv snapshots.csv

# 更长时间 / 更多输出帧 / 调整 CFL。
cargo run --no-default-features --bin water-simulation -- --shallow --headless --time 12 --frames 24 --cfl 0.4
cargo run --no-default-features --bin water-simulation -- --help
```

`--time / --frames / --csv / --quiet / --animate` 也会选择批处理模式，兼容之前的命令。这些批处理选项不能与 `--tui / --paused` 混用。`--cells` 和 `--cfl` 同时适用于两种模式。

## 交互操作

| 按键 | 操作 |
| --- | --- |
| Space | 暂停 / 继续 |
| 鼠标左键 | 点击水面图的绘图区，在对应水槽位置添加局部波包 |
| N | 暂停并只推进一个 CFL 时间步 |
| R | 重建初始高斯凸起、清零时间并暂停 |
| + / - | 播放速度加倍 / 减半，范围 0.125×～8×；等号键也可加速 |
| G | 切换 41 / 201 网格，重新初始化并暂停 |
| Left / Right | 选择前一个 / 后一个网格 |
| Home / End / C | 选择首格 / 末格 / 中央格 |
| Q / Esc / Ctrl+C | 退出并恢复终端 |

水面图中青色曲线是当前水深，暗蓝色曲线是初始水面，灰线是静水位。黄色点标记选中的网格，状态栏显示其 `h / hu / u`。下方绿色曲线是速度，正值向右、负值向左。

水面纵轴保持不缩小，波峰或扰动超出范围时向外扩展，重置后恢复初始范围；速度使用自动纵轴，观察流向时要同时看轴刻度和 `max|u|`。所有网格都参与求解，窗口缩放只改变显示。质量相对误差按当前初态计算；重置或切换网格会建立新的参考质量。

鼠标点击的横坐标决定水槽中的扰动位置，纵坐标只用于判断是否位于水面绘图区。边框、坐标轴、速度图和其他面板的点击会忽略；按住拖动不会连续添加扰动。点击后黄色标记会移到该网格，`ripples` 显示本次重置后的扰动次数。暂停时立即显示扰动，仍保持暂停，可以按 N 单步观察；运行时继续传播。退出后会关闭鼠标捕获，恢复终端的鼠标行为。

点击产生约 0.08 倍静水深的局部凸起，两侧稍微降低，不添加总水量，也不直接注入动量或重置时间。实现位于独立求解器的 `Simulation::add_disturbance()`：用窄、宽两条高斯曲线之差构造波包，再按实际网格上的离散和归一化，使靠墙的扰动也保持质量。连续点击导致局部水浅时，整体减小本次扰动振幅，保留至少 75% 的原水深。

播放速度仅决定每秒墙钟时间推进多少模拟时间，每个物理小步仍受 CFL 限制。每次交互循环最多使用 0.1 秒墙钟增量，避免窗口挂起后追赶长时间积累而卡住输入；机器负载高时实际播放会慢于设定倍率。界面刷新目标约为 30 帧/秒，单步模式每次只走一个求解器时间步。

## 模块结构

```text
src/
  lib.rs          库入口，公开 solver 并保留原有导入路径
  solver.rs       Cell / Flux / Config / Simulation，Rusanov、CFL、双缓冲、边界
  main.rs         参数解析和 TUI / 批处理模式选择
  batch.rs        原有 ASCII 快照、动画和 CSV 输出
  tui/
    mod.rs        终端生命周期、按键与刷新循环
    app.rs        暂停、单步、调速、重置、网格选择
    view.rs       曲线、诊断数值、按键提示与窗口布局
```

依赖方向是 `tui / batch → solver`。`solver.rs` 不读取键盘、时钟或终端，也不包含 Ratatui/Crossterm 类型，可以单独阅读或移入其他 Rust 项目。

```rust
use water_simulation::solver::{Config, Simulation};

let mut simulation = Simulation::new(Config::default())?;
simulation.advance_to(1.0)?;
let center_depth = simulation.cells()[100].h;
```

这里的 `?` 用在返回 `Result` 的调用方中。TUI 通过相同的公开接口驱动求解器，绘图不改变求解状态。界面实现可参考 [Ratatui 官方示例](https://ratatui.rs/examples/widgets/chart/)。

## 批处理输出

默认批处理模拟 6 秒，输出初态和之后的 12 个等间隔快照。CSV 的目标文件必须不存在，避免覆盖已有结果。每行字段为 `t,cell,x,h,hu,u`，`x` 是网格中心坐标，`u=hu/h`。显示帧数不等于模拟步数；求解器会在两次输出之间自动走许多个 CFL 小步，末步缩短以对齐输出时间。批处理动画每帧延迟 50 毫秒，只控制播放节奏。

ASCII 中 `*` 表示水面，`.` 表示初始静水位。纵轴固定并放大水面起伏，最多显示 81 列；201 个网格仍全部参与计算。每帧附带水深范围、最大速度、时间步和质量相对误差。

默认参数在 `src/solver.rs` 的 `Config::default()`：

| 参数 | 默认值 |
| --- | --- |
| 网格数 N | 201（可选 41 或其他不少于 3 的数量） |
| 水槽长度 L | 20 m |
| 静水深 h₀ | 1 m |
| 中央凸起高度 | 0.2 m |
| 高斯凸起标准差 | 0.8 m |
| 初始速度 u | 0 |
| 重力 g | 9.81 m/s² |
| CFL | 0.45，允许范围 (0, 0.5] |

海底固定为零，所以水深 `h` 也就是水面高度。初始化在网格中心采样高斯函数，作为单元平均水深的近似；这不是高斯函数在单元内的精确积分。所有计算使用 `f64`。

## 从代码理解一次更新

建议先看 `src/solver.rs`，再看 `src/tui/app.rs` 如何调用求解器，最后看 `tests/physics.rs`。`src/tui/view.rs` 负责显示，求解器不依赖终端。

### 1. 一个格子保存什么

```rust
pub struct Cell {
    pub h: f64,
    pub hu: f64,
}
```

`h` 是一个固定网格内的平均水深；`hu` 是水深乘速度，不是速度本身。需要速度时才计算 `hu / h`。同一段水可以流出这个格子，格子本身始终留在原处。

把 `Q=(h, hu)` 当成两个一起更新的数字，浅水方程为：

```text
∂Q/∂t + ∂F(Q)/∂x = 0
F(Q) = (hu, (hu)²/h + g h²/2)
```

代码中的 `Cell::physical_flux()` 对应这个 `F`。质量通量是 `hu`；动量通量包含流动搬运动量的部分和静水压力部分。即使初速为零，不同水深仍有不同的压力通量，因而会产生运动。方程和波速可参照 [Clawpack 浅水方程教材](https://www.clawpack.org/riemann_book/html/Shallow_water.html)。

### 2. 相邻格子如何交换水和动量

同一个界面左侧是 `left`，右侧是 `right`。两边状态一般不同，`rusanov_flux()` 给出双方共同使用的界面通量：

```rust
let alpha = left.wave_speed(gravity).max(right.wave_speed(gravity));
// 质量分量；动量分量使用完全相同的结构。
mass: 0.5 * (fl.mass + fr.mass) - 0.5 * alpha * (right.h - left.h)
```

对应：

```text
F_hat = (F_L + F_R)/2 - alpha * (Q_R - Q_L)/2
alpha = max(|u_L| + sqrt(g h_L), |u_R| + sqrt(g h_R))
```

前半项平均两侧物理通量；后半项根据状态差添加数值耗散。两边一样时，差值为零。以 `h_L=1.2, h_R=1.0, hu_L=hu_R=0` 为例，`alpha≈3.431 m/s`，界面质量通量约为 `+0.343 m²/s`，方向从左侧高水位指向右侧低水位。

为什么这个修正能抑制震荡？暂时把相邻界面的 `alpha` 当成同一个常数，把耗散项代入更新式，得到额外的：

```text
alpha * dt/(2 dx) * (Q[i+1] - 2 Q[i] + Q[i-1])
```

括号是离散二阶差分，会平滑格子之间的尖锐变化。在空间离散层面，它近似于系数 `alpha*dx/2` 的人工扩散。这里没有模拟真实黏性；这是数值方法带来的平滑，粗网格上更明显。它是局部 Lax–Friedrichs / Rusanov 近似通量，不是精确黎曼解。公式也可核对 [MFEM 的 RusanovFlux 文档](https://docs.mfem.org/4.10/classmfem_1_1RusanovFlux.html)。

### 3. 如何更新整个水槽

`Simulation::step()` 的计算顺序是：

1. 从当前状态求 CFL 时间步。
2. 计算 N+1 个界面通量，包括两端墙面。
3. 每个格子读取左右界面通量，写入 `next`。
4. 检查新状态，然后交换 `current` 和 `next`。

核心两行：

```rust
h: q.h - dt_over_dx * (right.mass - left.mass),
hu: q.hu - dt_over_dx * (right.momentum - left.momentum),
```

就是：

```text
新状态 = 旧状态 - dt/dx * (右侧流出 - 左侧流入)
```

所有界面通量都按向右为正定义，负值表示向左输运。一个内部界面的通量，对左格是流出，对右格是流入，累计时正好抵消。因此质量守恒来自共同使用同一份界面通量，不需要额外把总水量修正回去。

`std::mem::swap(&mut self.current, &mut self.next)` 交换两个 `Vec` 的所有权，不复制整段数据。这也保证一轮更新完全使用旧状态，不会让右边格子读到左边格子刚写出的新状态。

### 4. 墙面与时间步

两端墙外使用虚拟状态 `(h, -hu)`。左墙通量的输入顺序为 `(ghost, first)`，右墙为 `(last, ghost)`。两侧水深相等、动量相反，墙面的质量通量恰好为零，而动量通量仍然存在，表示墙对水的作用。这会产生反射。参见 [Clawpack 的反射边界说明](https://www.clawpack.org/bc.html)。

波相对网格的传播速度是 `u ± sqrt(g*h)`，所以每一步使用：

```text
dt_cfl = CFL * dx / max_i(|u_i| + sqrt(g*h_i))
dt = min(dt_cfl, 距下次输出的剩余时间)
```

水面越高或流速越快，就需要越小的时间步。默认初始步长约为 0.013 秒。初始 1 米水深上的小振幅波速约为 3.13 m/s，从中央到墙面大约需要 3 秒，因此默认 6 秒演示能看到反射。

## 验证覆盖

```powershell
cargo fmt --check
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
```

物理测试覆盖：相同状态通量、手算水深跳跃通量、墙面零质量通量、静水保持、41/201 网格长期质量守恒、水深镜像与动量反镜像、动态 CFL 限制、小振幅传播速度、撞墙后传播方向反转。另有较陡的全湿凸起在 CFL=0.5 下运行 30 秒的有限性、正水深、有界性及能量检查，以及无效输入、时间对齐和命令行检查。

TUI 测试覆盖暂停、单步、重置、切换网格、选格边界、速度倍率、按键和鼠标到求解器的连接，以及 120×36、80×24、70×22 和过小窗口的绘制。鼠标命中测试对照实际渲染的坐标轴位置，检查只有水面绘图区的左键按下才添加扰动。扰动测试另检查中央和墙边的质量守恒、后续传播、连续扰动的正水深和无效输入。可以用 `tui-test` 在实际 PTY 中继续检查按键、鼠标、屏幕与正常退出。

守恒检查使用 `sum(h_i)*dx`，即单位宽度水体体积；密度固定为 1 时也可视作质量。允许浮点舍入误差。反射墙会交换动量，所以一般初态下不要求总动量守恒；对称初态下总动量应接近零。Rusanov 方法具有数值耗散，因此也不要求能量严格守恒。

此版本使用一阶空间离散和显式 Euler 时间推进；它适合逐行学习和验证基础行为。测试只验证所列工况，不构成任意初态下稳定性的证明。若出现非正水深或非有限状态，程序报错并保留上一份有效状态，不通过裁剪水深改变总质量。

范围限于一维、CPU、平底、全湿水槽，没有二维、GPU、地形源项或干湿边界。
