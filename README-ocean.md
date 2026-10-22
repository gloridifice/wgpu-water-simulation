# 三维 FFT 海洋（wgpu + egui）

在独立窗口里运行的海面演示。CPU 初始化 Phillips 波谱，GPU 执行频谱演化、二维逆 FFT 和水面绘制。默认 128×128 个频率、128 米周期块，渲染 3×3 个重复块。原有 CPU PIC / 浅水 TUI 的入口保持不变。

## 运行

```powershell
Set-Location G:\MyProjects\Rust\water-simulation
cargo run --release --features ocean --bin ocean

# 改变频谱分辨率、风速与随机种子。
cargo run --release --features ocean --bin ocean -- --resolution 256 --wind 18 --seed 7
```

需要支持计算着色器和顶点阶段存储缓冲区的图形设备；通过 wgpu 使用本机后端，不依赖浏览器、模型、贴图或外部素材。首次构建会下载图形相关依赖。`ocean` 是可选 feature，普通 `cargo run` 仍运行原来的 TUI。

| 操作 | 效果 |
| --- | --- |
| 鼠标左键拖动 | 围绕海面中心旋转相机 |
| 滚轮 | 拉近 / 拉远 |
| Space | 暂停 / 继续 |
| C | 开关水平位移，比较圆润与尖锐浪峰 |
| F | 开关 Jacobian 泡沫显示 |
| ↑ / ↓ | 增减位移幅度，0.1×～2× |
| R | 相机、位移幅度、泡沫显示和时间重置；保留暂停状态 |
| Esc / Q | 退出 |

标题栏显示帧率、时间、暂停状态和幅度。没有把 PIC 的点击扰动接入 FFT；这里的鼠标用于相机。

## egui 参数面板

egui 控件和 egui_dock 使用从 `wgpu_pbr` 移植的深色主题与 MiSans 字体。主题保存在 `assets/egui_themes/default.ron`，字体保存在 `assets/fonts/MiSans-Normal.ttf`；两者在编译时内嵌，运行时无需访问原项目或从特定目录启动。修改主题后需重新编译。

启动后左上角显示 Ocean controls。面板使用英文标签，可拖动标题栏移动；小窗口内可以滚动。修改频谱、尺寸或种子后点击 **Apply changes**；播放和外观参数即时生效。Defaults 只恢复待应用值，再点击 Apply。

| 控件 | 含义 |
| --- | --- |
| A (power) | Phillips 频谱的功率系数，默认 0.002；设为 0 得到平面，乘 4 则波幅乘 2 |
| L (m) | Phillips 大波尺度，默认约 19.98 m，独立于海面块尺寸；显示等效风速 sqrt(gL) |
| Wind angle (deg) | 公式中 omega-hat 的单位风向，由角度得到 (cos θ, sin θ)；不是时间角频率 |
| Patch side (m) | 正方形周期海面块的边长；场景绘制 3×3 块 |
| FFT spectrum | 频域数组的 N×N 分辨率，8～512 的 2 次幂 |
| Mesh cells / axis | 每块网格每轴格子数 M，8～512；顶点为 (M+1)×(M+1)，可与 FFT 分辨率不同 |
| Seed / New | 固定随机种子 / 换一个随机海面 |
| Paused / Restart time / Time speed | 暂停、时间归零、播放倍率 |
| Height gain / Horizontal choppiness / Show foam | 渲染位移倍率、水平位移开关、泡沫显示 |

网格通过周期双线性插值读取 FFT 高度与位移；增加网格密度不会凭空增加频谱中的波。网格比 FFT 粗时可能漏掉短波形状。Apply 会重建计算缓冲区与网格绑定，保留当前时间及暂停状态；尺寸改变时按比例调整相机距离。面板输入优先，不会同时旋转相机或触发海面快捷键。

命令行也支持 `--amplitude 0.002 --phillips-length 20 --wind-angle 45 --mesh-resolution 192`。旧的 `--wind 14` 仍可用，作用是设置 `L=U²/g`；若同时提供 `--phillips-length`，后出现的选项生效。

## 每帧的数据流

```text
CPU，初始化一次：
  A、L、风向、种子 → Phillips 波谱 → 随机复数系数 h0(k)

GPU，每帧：
  h0(k)、时间 t
      ↓ evolve.wgsl
  高度频谱 H(k,t) 与水平位移频谱 Dx(k,t)、Dz(k,t)
      ↓ ifft.wgsl，先逐行，再逐列
  空间中的高度 h(x,z)、水平位移 dx(x,z)、dz(x,z)
      ↓ foam.wgsl：Jacobian 压缩检测、泡沫生成与消散
  累积泡沫覆盖 F 和当前 Jacobian J
      ↓ render.wgsl
  海底 mesh → 线性场景颜色 / 深度预绘制
  网格顶点位移 → 位移后的差分法线 → 水中光程与解析积分 → Fresnel / 阳光 / 泡沫
```

### 频谱与时间

`src/ocean/spectrum.rs` 把数组索引转换为波向量 `k = 2π * frequency / patch_length`。数组采用未移位 FFT 顺序：`0,1,...,N/2-1,-N/2,...,-1`。

Phillips 波谱控制不同波长和方向的能量；高斯随机数给出复数初始系数。初始化使用 `sqrt(P/2) * (ξr+iξi)`，再乘 `N² * Δk`，其中 `Δk=2π/patch_length`：N² 补偿归一化 IFFT，Δk 来自二维频率单元面积的平方根。取消旧版固定 RMS 波高归一化，避免抵消 A 的作用。A、L 均会真实改变海面。保留短波阻尼和逆风方向 0.07 的权重。零频率和 Nyquist 行列设为零。

`evolve.wgsl` 用深水重力波色散关系 `ω = sqrt(g * |k|)`，计算：

```text
H(k,t) = h0(k) * exp(iωt) + conjugate(h0(-k)) * exp(-iωt)
```

这保证 `H(-k,t) = conjugate(H(k,t))`，所以逆变换的虚部应该接近零。`exp(iωt)` 表示复平面中的旋转，对应波的相位变化。初始化后不再每帧产生随机噪声，也不从上一帧累计更新波形。

水平位移使用 `D(k,t) = -i * k/|k| * H(k,t)`。在本实现的逆变换符号约定下，渲染使用负的缩放系数 λ = -1.15，让顶点向浪峰聚拢而不是向浪谷聚拢；按 C 可以观察区别。

### GPU 逆 FFT

`ifft.wgsl` 使用 radix-2 DIT 蝶形：每个轴的第一级通过位反转读取输入，之后逐级把跨度从 2 扩大到 N。逆变换的旋转因子使用正号，每轴最后一级除以 N。

`gpu.rs` 用两个存储缓冲区交替读写。每一级单独提交一个 compute pass，由 wgpu 负责阶段间的资源同步。各级使用独立、不变的参数缓冲区，防止在一次提交前多次写同一个 uniform 导致所有阶段读到最后一组参数。

128×128 网格每轴 7 级，共 14 个 FFT pass。高度、X/Z 位移和近岸输入的低通水位、X/Z 动量放在同一个缓冲区的六个通道中一起变换。总级数为偶数，最终结果回到初始输出缓冲区，直接供顶点着色器读取，不需要每帧下载到 CPU。

### 绘制

网格顶点由 `vertex_index` 生成；索引缓冲区在初始化或 Apply 时创建。顶点位置为 `(x + dx, h, z + dz)`，法线根据位移后的相邻点计算。周期采样使各块接缝的高度、水平位移和法线一致。

天空使用程序化颜色，水面包含 Fresnel 反射、太阳高光、简化水色；不使用距离雾。白沫来自位移 Jacobian 及独立历史场，带周期程序化细节和远处细节过滤。它是视觉白浪模型，不是独立泡沫粒子或海流输运模型。

## 验证和导出

```powershell
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings

# 无窗口运行真实 GPU 计算，和独立 f64 直接傅里叶求和逐点比较。
cargo run --features ocean --bin ocean -- --verify

# 使用同一套 GPU 计算和绘制管线导出图片；目标文件必须不存在。
cargo run --release --features ocean --bin ocean -- --snapshot ocean.png --time 3

# 打开真实窗口，成功呈现 120 帧后自动退出，便于启动检查。
cargo run --features ocean --bin ocean -- --frames 120
```

CPU 测试检查随机种子复现、有限数值、零频率、Nyquist 处理、A 的平方根振幅缩放、A=0 的平面、网格密度不影响频谱，以及共轭谱得到实数输出和时间变化。`--verify` 在 8×8 和 16×16 网格的三个时刻，比较高度、两个水平位移以及三个近岸输入通道的全部实部、虚部；同时检查零均值和波形是否随时间变化。普通测试不强制要求 GPU；GPU 验证通过独立命令明确运行。

## 文件

```text
src/bin/ocean.rs       参数与入口
src/ocean/spectrum.rs  初始化波谱，独立于窗口与 GPU
src/ocean/evolve.wgsl  GPU 时间演化和位移频谱
src/ocean/ifft.wgsl    GPU 二维逆 FFT 蝶形
src/ocean/gpu.rs       缓冲区、计算和绘制管线
src/ocean/foam.rs      泡沫 GPU 资源、CPU 参考及数值验证
src/ocean/foam.wgsl    Jacobian 与局部泡沫历史更新
src/ocean/render.wgsl  三维水面、泡沫与天空着色
src/ocean/gui.rs       egui 参数面板与叠加绘制
src/ocean/viewer.rs    窗口、相机、交互、离屏 PNG
src/ocean/verify.rs    独立直接 DFT 与 GPU 验证
```

基础 FFT 生成周期海面，不模拟水体内部速度或岸边碰撞。沙滩预设现已叠加程序化岸浪、泡沫输运和湿沙历史，详见文末；仍不包含守恒浅水求解或真实翻卷。水平位移过大使网格折叠并不等于物理破浪。

公式参考：[Jerry Tessendorf, Simulating Ocean Water，§3.2–3.4、3.7](https://jtessen.people.clemson.edu/reports/papers_files/coursenotes2002.pdf)。图形接口：[wgpu](https://docs.rs/wgpu/27.0.1/wgpu/)、[winit ApplicationHandler](https://docs.rs/winit/0.30.13/winit/application/trait.ApplicationHandler.html)。


GUI 使用 [egui-wgpu 0.33.3](https://docs.rs/egui-wgpu/0.33.3/egui_wgpu/)，与 wgpu 27 配套。

## Jacobian 泡沫：调研与实现

采用 **IFFT 后中心差分 + 压缩触发 + 有记忆的覆盖率**。

参考资料：
- [Crest 的 UpdateFoam.compute](https://github.com/wave-harmonic/crest/blob/master/crest/Assets/Crest/Crest/Shaders/Resources/UpdateFoam.compute)：从水平位移计算行列式，在压缩区域生成泡沫，并对历史衰减；也有独立 flow 输运。
- [Crest 泡沫参数说明](https://crest.readthedocs.io/en/4.15.1/user/ocean-simulation.html#foam)：生成强度、覆盖阈值、消散速度分开控制。
- [Aalto 海洋渲染论文，第 3.8 节](https://aaltodoc.aalto.fi/bitstreams/805df4ad-f08a-40cc-a9bd-3d25947da66d/download)：整理 Tessendorf 位移 Jacobian 和频域导数公式。

### 为什么选空间差分

频域中乘 `ik` 再 IFFT 可以得到导数，但需要增加导数通道及 FFT 工作量。当前已经有 Dx、Dz，使用一个 O(N²) compute pass 即可近似求导，易于独立验证。差分步长用 `patch_length / FFT_resolution`，与 mesh 密度无关；更改 FFT 分辨率会改变解析到的短波及导数精度，因此视觉阈值可能需要重新调整。

令实际水平位移 `d = lambda * gain * D`。lambda 使用渲染中的负号（默认 -1.15），不可取绝对值，也不能漏掉 Height gain。

```text
J = (1 + ∂dx/∂x) * (1 + ∂dz/∂z) - (∂dx/∂z) * (∂dz/∂x)
```

所有导数在周期边界上取中心差分。分别计算两个交叉导数，避免把离散差分下不一定相等的量误写成同一个平方。

J=1 表示水平面积不变；0<J<1 表示压缩；J 接近 0 表示局部水平映射接近奇异；J<0 表示局部方向翻转。J 是水平映射的有符号面积比，不是三维水面面积，也不是严格的破浪判据。使用 `threshold > 0` 可在几何翻折前产生白沫。

### 历史更新

覆盖率 F 在 0～1 之间，定义：

```text
s = generation * clamp(threshold - J, 0, 1)
dF/dt = s*(1-F) - decay*F
r = s + decay
F_new = s/r + (F_old - s/r) * exp(-r*dt)
```

r=0 或 dt=0 时保留原值。上式是本项目选择的有界源项模型，对每步冻结的 J 精确积分；对固定源而言 30/60/120 FPS 的结果一致。运动波形仍有时间采样误差，并不声称任意帧率下整个动态结果完全相同。

历史存于未位移海面的周期参数坐标，随 mesh 位移；不是世界空间海流输运。每个线程只读写自己的 F，因此无需 ping-pong。增加邻域输运、扩散时必须改成双缓冲。每个 FFT 像素保存 `(F,J)`，新增内存为 `8*N²` 字节（128² 时 128 KiB，512² 时 2 MiB），每帧额外一个 compute pass。

### 面板和生命周期

独立的 **Jacobian foam** 面板直接显示在海洋参数面板右侧，所有泡沫控件即时生效，无需 Apply。

- **Chop strength**：实际水平位移强度，也会影响 J。
- **Show foam / F 键**：只控制显示；历史继续演化。
- **J threshold**：默认 0.65，越大越容易产生泡沫；最大 1，平面不会自己产生泡沫。
- **Generation / s**：默认 4，增加生成速度；设为 0 可单独观察旧泡沫衰减。
- **Decay / s**：默认 0.6，越大消失越快；设为 0 会保留历史。
- **Foam view**：正常着色、Jacobian（红：负值，橙：压缩，白：1，蓝：拉伸）、Coverage（黑白 F）。
- **Clear foam history**：清空历史，暂停时也有效。

暂停或 Time speed=0 时不增长也不衰减；时间重置、R 重置和 Apply 重建时清空历史。关闭 choppiness 或把 gain 设为 0 后 J=1，不生成新泡沫，已有泡沫继续衰减。无雾渲染保持不变。

### 验证与导出

`cargo test --all-features` 包含平面 J=1、无源衰减、暂停、清空、交叉导数、位移倍率为零、固定源下帧率一致性测试。`--verify` 额外运行真实 GPU 泡沫着色器，与 f64 CPU 参考逐点比较 F、J，涵盖周期接缝及历史更新。

```powershell
cargo run --features ocean --bin ocean -- --snapshot foam.png --time 3
cargo run --features ocean --bin ocean -- --snapshot jacobian.png --time 3 --foam-view 1
cargo run --features ocean --bin ocean -- --snapshot coverage.png --time 3 --foam-view 2
```

离屏导出按 60 Hz 预演历史，最多预演目标时间前 8 秒（长时间导出是有限预热近似）；t=0 时初始覆盖为零。输出日志包含 J 最小值、F 最大值及 F>0.02 的网格占比，方便与反射高光区分。

## 水下地面与解析水体积分

新增真实海底平面 mesh（两个三角形），默认位于平均海面以下 6 米。棋盘格用于观察远近变化，可切换 RGB 色带观察通道衰减。海底使用简化光照，场景先绘制到 `Rgba16Float` 线性颜色纹理和可采样的 `Depth32Float` 纹理；主 pass 再绘制天空、海底与水面。海底仅有几何与着色，不改变 FFT 波浪模拟。

水面使用同一个屏幕像素处的海底深度，经逆视投影矩阵重建世界位置，计算 `d=length(Pbottom-Psurface)`。因此使用的是水中直线光程（米），不是相机到海面的距离、垂直水深或原始深度缓冲差。WebGPU 的 NDC 深度按 0～1 重建，纹理 Y 与 NDC Y 的方向已转换。本版不做折射。

```text
T_rgb = exp(-sigma_t_rgb * d)
background_rgb = Lbg_rgb * T_rgb
manual_rgb = S_manual_rgb * (1 - T_rgb) / sigma_t_rgb
scattering_rgb = manual_rgb + integrated_sky_rgb
medium_rgb = background_rgb + scattering_rgb
water_rgb = (1-Fresnel)*medium_rgb + Fresnel*reflection_rgb
```

随后合成太阳高光和 Jacobian 泡沫。积分之前的计算在线性 RGB 中进行，最后由 sRGB 输出目标编码。不要对指数计算的输入做 gamma 编码；调试图中的 sRGB 显示数值也不等于线性数值。光程视图以米为尺度映射为灰度。

sigma_t 是消光系数（吸收+出散射），单位 1/m；S 是沿观察方向每米增加的入散射辐亮度，不是 sigma_s。手动 S 是可直接调整、沿光路恒定的近似源项；新增天光源项由天空、散射反照率和深度求解，详见后文。太阳入散射尚未接入。海底 Lbg 使用简单环境光与方向光，不包含太阳到海底的水中入射路径衰减。该模型适合逐项观察公式，不等于完整的多次散射求解。

`optics.wgsl` 在小光学厚度 tau=sigma_t*d 时使用泰勒展开，避免除零和 `1-exp(-tau)` 的消减误差。sigma_t=0 时严格使用权重 d，输出趋于 `Lbg+S*d`；d=0 时为 Lbg。RGB 每个通道独立处理。场景颜色和采样深度与主 pass 的写入目标分离，避免纹理反馈；resize 会同时重建场景目标、主深度以及对应绑定。

### Water optics 面板

控件即时生效，无需 Apply，不会重建海浪频谱或清空泡沫历史。

| 控件 | 含义 |
| --- | --- |
| Seabed depth (m) | 海底相对平均水位的深度，默认 6 米 |
| Extinction sigma_t R/G/B | RGB 消光系数，默认 (0.18, 0.06, 0.025) 1/m |
| Manual source S（颜色 + 强度） | RGB 恒定入散射源项，默认 (0.0025, 0.009, 0.006) 辐亮度/米 |
| Water view | 下表所列调试输出 |
| Path view white at (m) | 光程灰度图达到白色时对应的距离，默认 50 米 |
| Sky reflection and highlights | 显示/隐藏天空反射和高光，便于单独观察海底 |
| Seabed RGB test bands | 在棋盘格上显示 R/G/B 测试色带 |
| Checker size (m) | 每个棋盘格的世界尺寸 |
| Reset optics | 恢复上述默认值 |

| Water view / CLI 编号 | 输出 |
| --- | --- |
| Shaded water / 0 | 完整水面合成 |
| Water path (m) / 1 | clamp(d/path_range,0,1) 灰度 |
| Transmittance T / 2 | RGB 透射率 T |
| Attenuated seabed (absorption) / 3 | Lbg*T，直观观察吸收/消光后的海底颜色 |
| In-scattering / 4 | 手动 S 与天光的累计入散射之和 |
| Extinction loss / 5 | Lbg*(1-T)，被消光移除的颜色，包含出散射 |
| Raw seabed / 6 | 未经过水体积分的 Lbg |
| Medium sum / 7 | background+scattering，不混合表面反射和泡沫 |

水体调试视图优先于泡沫调试，并绕过反射、高光和泡沫。先用 Raw seabed 看地面，再比较 Attenuated seabed 与 In-scattering，可直接观察两个加和项。

### 运行、导出与验证

```powershell
cargo run --release --features ocean --bin ocean
cargo run --release --features ocean --bin ocean -- --seabed-depth 12 --sigma-t 0.2,0.08,0.03 --source 0.003,0.008,0.005
cargo run --release --features ocean --bin ocean -- --snapshot water-path.png --water-view 1 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot transmittance.png --water-view 2 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot attenuated-bottom.png --water-view 3 --time 3
cargo run --release --features ocean --bin ocean -- --snapshot in-scattering.png --water-view 4 --time 3
cargo run --features ocean --bin ocean -- --verify
```

地面覆盖当前 3×3 海面块的范围。场景深度没有海底命中（例如远处超出地面或裁剪范围）时，以黑色背景、1000 米光程作为有限的深水近似；光程视图中表现为白色。S 非零而 sigma_t=0 时，恒定源会随光程线性累积；大参数的辐亮度超过 1 时，当前非 HDR 显示目标会截断，不能据此判断积分结果上限为 1。地面调得很浅时，波谷可能低于地面，深度测试会让地面挡住水面；水平海底模式不模拟干湿边界；沙滩模式使用下述局部 SWE。

CPU 测试包含零距离、零消光、纯衰减、RGB 独立性、源项缩放和数值积分对照。`--verify` 运行共享的生产 WGSL 积分函数，检查 40 组参数下所有 RGB 分量；另用真实海底预绘制+水面绘制管线在两种尺寸（包含 resize）、三种深度、七种调试视图和三个像素位置，与独立解析平面交点计算的光程/颜色逐点比较。

代码：`src/ocean/optics.rs`（参数和参考计算）、`optics.wgsl`（共享积分）、`seabed.rs` / `seabed.wgsl`（海底预绘制）、`optics_verify.rs`（光栅管线验证）。


## 天光深度查找表与分段解析积分

### 沙滩观察场景

启动时添加 `--beach`，或点击 Shallow water (SWE) 面板的 **Beach scene**，即可载入沙坡、水体光学参数和俯视斜向岸线的相机预设。预设使用较小波幅，手动 S 为零、天光开启，方便比较从可见沙底的浅水到深色深水的变化。

沙坡是实际几何平面 `y = -beach_slope * (x - shore_x)`，负 X 为陆地、正 X 为深水。面板 **Beach slope** 控制坡度，**Shore X** 移动平均水位岸线，**Beach camera** 恢复观察角度；取消 **Sloped sand** 返回水平棋盘海底。沙坡启用时平底深度控件禁用，实际水深由坡面决定。上述设置支持自动保存。

沙地使用干湿颜色与抗锯齿细纹，光照法线随坡面倾斜。深度预绘制和世界坐标重建计算真实斜向光程，水色没有按岸线距离手工渐变；可切换 Water path、Transmittance、Attenuated seabed 检查。岸上沙坡通过深度测试遮住海面。`--beach --no-shore` 可回到只有 FFT 与沙坡的观察模式；默认沙滩启用下述局部二维 SWE。

```powershell
cargo run --release --features ocean --bin ocean -- --beach
```

`--beach` 每次启动都会重新套用预设；希望继续调整后的状态时，下次省略该参数。`--snapshot beach.png --beach` 可离屏导出相同观察视角。

Water optics 的 **Sky scattering** 区域即时生效，不需要 Apply：

| 控件 | 含义 |
| --- | --- |
| Enable sky scattering | 启用水下天光源项；不影响天空背景和表面反射开关 |
| Sky tint / Sky intensity | egui 原生线性 RGB 颜色选择器及 0～20 强度，共用于天空背景、反射、水下天光；白色 tint 保留原有天空渐变 |
| Scattering albedo RGB | 各通道 α=σ_s/σ_t，范围 0～1；σ_s=α σ_t，σ_a=(1-α) σ_t |
| Precise sky RGB | 可直接输入颜色和反照率的线性 RGB 数值 |
| Sky integration steps | 8 / 16 / 32 段，默认 16；更高档提高积分精度 |
| Sky source vs depth | 显示水面到海底的 RGB 局部源项曲线，纵轴为 radiance/m，自动缩放 |
| Manual source S | 原手动常量源项保留，可归零来单独观察天光 |

天光使用无太阳圆盘和光晕的程序化天空背景，太阳亮斑仍显示在天空/反射里。采用各向同性相函数 `p=1/(4π)`，在上半球按立体角积分；不乘表面光照的 N·L。均匀天空、无衰减时半球因子为 1/2，不是 1。

```text
z = max(-world.y, 0)                 # 平均海面 y=0
mu_i = (i + 0.5) / 32               # 当前天空轴对称，32 个等立体角采样
J_c(tau) = sum_i Lsky_c(mu_i) * exp(-tau / mu_i) / 64
Ssky_c(z) = sigma_t_c * albedo_c * J_c(sigma_t_c * z)
S_total = S_manual + S_sky
```

GPU 将 J 存入 **256×1 Rgba32Float** 查找表（4 KiB）。横轴使用光学深度 tau，而非固定米数，因此 RGB 不同消光系数可以共用同一张表。采样点 `tau=32*(i/255)^2` 在水面附近更密集，读取时在 tau 上做线性插值。tau>=32 的天光作为零处理，其透射上界小于 1.3e-14。只有天空颜色或强度变化时更新表；改变 σ_t、反照率、海底深度无需重建，resize 重新绑定原表。

沿视线从水面向海底累积天光，每段在中点求 Ssky，在段内使用原有解析权重 `(1-exp(-sigma_t*ds))/sigma_t`，再乘从该段到水面入口的透射率。手动 S 仍单独使用整条视线的解析解。

每通道独立按光程平方分段，并将天光积分尾部截断于 16 个观察光学深度（遗漏贡献的上界与 exp(-16) 成正比），避免长光程让第一段跨过整个浅层散射区。此截断只作用于天光，不改变手动 S、海底透射率或光程调试值。σ_t=0 或 α=0 时，对应通道的天光散射严格为零；手动 S 仍遵循原有 `S*d` 极限。

### 新增调试视图

| Water view / CLI 编号 | 输出 |
| --- | --- |
| Sky source at ray midpoint / 8 | 水下视线中点的局部 Ssky（radiance/m） |
| Integrated sky scattering / 9 | 已考虑观察光程衰减的累计天光颜色 |
| Integrated manual source / 10 | 原手动 S 的累计入散射 |
| Total source at ray midpoint / 11 | 中点处 S_manual+S_sky（radiance/m） |

4 为累计天光+累计手动源项，7 再加衰减后的海底。所有调试视图绕过反射、高光和泡沫；8、11 是局部源项，不能直接与累计颜色比较。局部源项通常较暗，可通过深度曲线读取数值。

```powershell
cargo run --release --features ocean --bin ocean -- --skylight
cargo run --release --features ocean --bin ocean -- --sky-color 0.8,0.9,1 --sky-intensity 2 --scattering-albedo 0.3,0.5,0.65 --sky-steps 32
cargo run --release --features ocean --bin ocean -- --snapshot sky-scattering.png --source 0,0,0 --water-view 9
cargo run --release --features ocean --bin ocean -- --snapshot manual-scattering.png --water-view 10 --no-skylight
```

边界：本版为单次、各向同性散射，入射天光按平均平面 y=0 计算深度，位于平均水面以上的浪峰按 z=0 处理。未计算入水折射、入口 Fresnel 损失、波面遮挡、焦散、太阳体积散射或多次散射。海底本身仍是原来的简化光照。天空颜色只控制天空背景，不改变独立的太阳亮斑颜色。

`--verify` 对真实 GPU LUT 与 f64 角度求和逐项比较，再用独立的逐方向闭式线积分检查生产 WGSL。覆盖零/极小/较大消光、不同 RGB、关闭天光、零反照率、强度/颜色变化、0～1000 米光程、不同水深与观察方向、8/16/32 段收敛；光栅检查包括 resize、LUT 参数更新及局部/累计/合成调试视图。

代码：`sky.wgsl` 共享天空背景，`skylight_lut.wgsl` 生成查找表，`skylight.wgsl` 查表和分段积分，`skylight.rs` 管理参数与 GPU 资源，`skylight_verify.rs` 独立验证。

## 本地自动保存与颜色选择器

所有海洋参数、未 Apply 的频谱草稿、泡沫参数、水体参数（包括天光）、相机、暂停和播放速度，默认保存于 `%LOCALAPPDATA%\water-simulation\ocean-settings.json`。停止编辑 0.5 秒后自动保存，窗口失去焦点和正常退出时立即保存；Ocean controls 顶部显示状态，提供 Save now 和文件位置。时间与泡沫历史不保存。

使用 `--settings PATH` 指定独立配置，`--no-settings` 禁止读取/写入配置。明确传入的 CLI 参数覆盖对应保存值，其余字段继续恢复。`--snapshot`、`--verify` 始终使用默认值加 CLI 参数，不读写用户配置。

旧配置不含 sky 段时默认关闭天光，以保留原效果；可勾选 Enable sky scattering 或加 `--skylight`。新配置默认启用天光。设置采用临时文件加原子替换写入，损坏或不支持的文件在重新保存前会保留 `.invalid-时间戳.json` 备份；写入失败会显示状态并重试，不会静默覆盖为成功。

σ_t 和手动 S 使用 egui 原生颜色选择器配合 Strength，实际线性系数为 `color * strength`，可展开精确 RGB 输入；颜色表示通道比例，强度不会受颜色选择器 0～1 范围限制。天空颜色和反照率直接使用 0～1 线性 RGB，天空另有独立强度。

## Local 2D shallow water, FFT offshore and stranded foam

```powershell
cargo run --release --features ocean --bin ocean -- --beach --no-settings
cargo run --release --features ocean --bin ocean -- --beach --snapshot shore.png --time 24
cargo run --release --features ocean --bin ocean -- --beach --shore-view 5 --snapshot stranded.png --time 27
cargo run --release --features ocean --bin ocean -- --beach --no-shore
```

The independent **Shallow water (SWE)** panel contains the SWE, foam, wet-sand and beach controls and opens by default. FFT Phillips power, wind direction and spectral length now drive both the ocean and the SWE inlet. Height gain scales inlet elevation and both momentum components together. The independent shore amplitude/period controls have been removed; old saved fields are ignored. The retired `--shore-amplitude` / `--shore-period` flags report a migration message. The entire SWE patch starts with FFT elevation and momentum; repeated run-up and foam still need time to develop. CLI overrides belong after `--beach`, which installs the beach preset.

| Shore view | Meaning |
| --- | --- |
| 0 | Shaded |
| 1 | Geometric water depth; white at 3 m |
| 2 | Cross-shore velocity: blue incoming, orange outgoing |
| 3 | Waterborne foam concentration |
| 4 | Stationary sand wetness |
| 5 | Stranded foam on the ground |
| 6 | Surface/deposited foam density: green (low) to red (reference or higher), no texture |

### Solver and rendering

`shore_compute.wgsl` advances a rectangular, world-space 256² finite-volume grid. It offers the original first-order hydrostatic Rusanov + Euler solver and a second-order MUSCL + SSP-RK2 solver. The latter reconstructs free surface, velocity and bed using a generalized minmod limiter (theta=1.3), with hydrostatic face corrections and a matching centered bed source. Reconstruction falls back to first order near dry cells and patch boundaries. Both retain wet/dry handling and the 12 m/s velocity safeguard; RK2 stages include boundary relaxation and bottom friction. Foam generation/decay and ground memory update once per completed substep and remain split first-order source updates. This is not a port of the reference project's KP07 solver.

The stored variables are `(h, hu, hv, surface_foam_density)`. Foam is an effective amount per unit surface area in arbitrary units, not a depth-weighted concentration or physical mass density. A separate conservative upwind flux transports foam using shared face velocities: convergence concentrates it and divergence dilutes it, without creating foam. Both orders use first-order spatial foam transport; second-order SWE uses RK2 for its transport time integration. Foam is not divided by water depth or clipped at 1. The offshore sponge removes foam; dry cells deposit their remaining amount on the sand. Closed boundaries have zero foam flux.

Generation uses rapid water-level rise gated by wet depth, with a stronger source at a supercritical/compressive front. It is a visual proxy for air entrainment, not resolved overturning. Fast shallow backwash alone no longer generates foam. Generation and exponential decay are integrated analytically with a frozen source per completed substep. Density may exceed the color reference, retaining accumulated amount rather than clipping it.

The patch spans at least 64 m across the beach and 384 m along it for a 128 m FFT tile. Its inland coverage expands with a conservative spectral height bound and Height gain, up to 64 m inland. Gentle slopes can also expand the offshore region. Alongshore spacing is 1.5 m at the default tile size. The bed remains the existing straight sand slope, shared by simulation and rendering. Landward and alongshore boundaries reflect; the offshore edge samples FFT elevation and matching depth-integrated momentum in a relaxation zone occupying the outer 20% of the patch. There is no arbitrary obstacle editor or terrain SDF in this version.

Substeps use a conservative speed/depth estimate and the rectangular-grid bound `dt <= 0.2 / ((12 + sqrt(g * depth_bound)) * (1/dx + 1/dz))`. A frame advances at most 0.1 simulated seconds and 128 substeps; excess time from a long stall is discarded. This estimate is intended for the built-in terrain and wave controls, not arbitrary externally injected states. Boundary forcing, thin-film removal and velocity safeguards mean the entire open scene is not a closed conservative system.

The water mesh reads the SWE free surface and fades to the existing FFT displacement between the configured blend depth and 2.5 times that depth. Normals come from the combined surface. The original three FFT render channels remain unchanged; three additional channels drive the SWE inlet. The coupling is **one-way FFT → SWE**: shore reflections and currents do not modify the ocean spectrum. Changing mesh density affects how much of the simulated front is visible. Water thinner than 8 mm is discarded by the water material.

Each substep writes a separate output buffer and copies it to the previous-state buffer; water and ground use a fixed output binding. Parameter copies are ordered in the command encoder so substeps cannot accidentally all read the final uniform. The two state buffers use 6 MiB total at 256². There is no per-frame CPU readback. The copied lace image has a CPU-built mip chain in a read-only GPU buffer (about 5.3 MiB); shader sampling performs bilinear/trilinear filtering.

### FFT → SWE boundary coupling

For each wave vector, the existing ocean evolution has two signed branches:

```text
A = h0(k) exp(+i omega t)                travels along -k
B = conjugate(h0(-k)) exp(-i omega t)    travels along +k
H = A + B
Q = (omega / |k|²) k (B - A)
omega = sqrt(g |k|)
```

`Q` is depth-integrated horizontal momentum in m²/s, derived from linear continuity. It is not the FFT choppiness displacement and not the time derivative of that displacement. Both signed branches are retained, so the boundary carries the prescribed external FFT field, including its directional distribution; this is a relaxation boundary, not a characteristic decomposition that injects only incoming waves.

The three boundary spectra `(H, Qx, Qz)` receive the same configurable smooth low-pass. In **Shallow water (SWE) → FFT input filter**, **Filter short waves** enables it; **Reject wavelength <= (m)** sets the rejected wavelength limit, and **Transition width (m)** sets the smooth transition above that limit. The default is a fixed 12 m cutoff plus a 12 m transition: wavelengths <= 12 m are rejected and >= 24 m pass fully. Unlike the earlier automatic rule, these values do not change with FFT resolution. A zero transition gives a hard cutoff. Disabling the filter passes all FFT modes. The **3–6 m** preset admits more short waves for comparison, but the first-order SWE grid can still dissipate unresolved waves and does not recover their dispersion.

Filter changes apply immediately and reset SWE water/foam/sand history, while FFT time keeps advancing. The patch is reseeded from the current filtered FFT, rather than restarted as still water. The settings persist with the other controls. No additional short-wave normal detail is added. CLI equivalents (put overrides after `--beach`):

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-filter-cutoff 3 --swe-filter-transition 3
cargo run --release --features ocean --bin ocean -- --beach --no-swe-filter
```

The six spectra share the existing IFFT stages. SWE samples channels 3–5 at `(world / FFT_tile_length + 0.5) * FFT_resolution`, with periodic wrapping and bilinear interpolation. This matches the ocean's undeformed world-coordinate convention. It interpolates the previous and current FFT boundary frames at each SWE substep; no FFT or momentum readback to the CPU is required.

On reset, the entire patch samples the current filtered FFT elevation and momentum. The six-second startup ramp has been removed. Height gain multiplied by the SWE-specific input gain applies to elevation and momentum equally. For extreme inputs, a common safety scale limits elevation to half the local still-water depth and momentum magnitude to eight times that depth. The scale preserves their ratio, although those clipped cases no longer reproduce the raw FFT amplitude.

The relaxed target is `(max(eta-Bed, 0), Qx, Qz, 0)`, blended with weight `1-exp(-4*edge_weight*dt)`. Foam is generated inside the SWE region rather than imported from FFT. The existing Jacobian foam remains on the far ocean.

The boundary momentum matches the **linear deep-water FFT**. SWE remains a non-dispersive shallow-water model, so this coupling does not eliminate phase-speed differences or reflection for intermediate/deep-water components. Short-wave filtering, the relaxation zone and the visual transition reduce the mismatch but do not constitute a finite-depth dispersion solver.

Adding three channels doubles the FFT field/scratch storage and butterfly work relative to the original three-channel ocean. At 128² it adds 768 KiB across the two FFT buffers, plus 384 KiB for the previous boundary frame. Normal rendering still performs no CPU readback.

### Ground memory and foam appearance

Ground memory is stationary. When water depth crosses below 3 mm, the last wet foam concentration becomes stranded foam at that location. It fades with a 10-second time constant and is cleared rapidly by rewetting (0.12 seconds). Wet sand dries with the configurable time constant, default 18 seconds. Waterborne foam continues to move with the conservative flux.

The water material uses two offset flow phases driven by the simulated velocity and coarse/fine Voronoi lace samples. Concentration changes an erosion threshold, opening holes as foam disappears. The ground uses the same lace in fixed world coordinates. Mip filtering and derivative-smoothed thresholds reduce distant shimmer. F / Show foam affects both waterborne and stranded foam without stopping their evolution.

The bundled `assets/shorewaves/voronoi_lace.png` comes from Reboot16 / Enrico K.'s Shorewaves project. Its MIT notice is retained in `assets/shorewaves/LICENSE.txt`. The solver and material implementation here are adapted to Rust/wgpu/WGSL and the existing FFT renderer; no Godot runtime is needed.

### Lifecycle and validation

Pause freezes simulation and texture flow. R, Restart time and **Reset shallow water, foam and sand** reinitialize the local simulation from the current FFT and clear foam/sand history. Terrain, Height gain, SWE input gain, blend-depth and enable changes also reset it; applying a new FFT spectrum rebuilds the coupled resources; foam-rate, drying-time and view changes preserve history. Settings persist, but simulation state does not. Snapshot mode prewarms up to 60 seconds of shore history (8 seconds offshore); later snapshots approximate a finite history window.

`--verify` exercises the actual GPU shader: lake at rest, local pulse mass conservation away from the inlet, propagation and foam transport in both axes, stranded deposition/decay/rewetting, extreme slope/amplitude settings, terrain reset, run-up, finite/nonnegative states, pause, reset and disable. Raster verification prewarms 24 seconds and checks water history at two target sizes. The FFT test compares all six channels to a double-precision direct DFT and checks analytic oblique waves in both directions. An inlet probe checks production world-coordinate wrapping, time interpolation and gain against CPU samples. Zero FFT power must generate no shoreline flow/foam. Existing optics and sky tests remain enabled. `cargo test --all-features` covers configuration compatibility and the existing CPU suites.

The heightfield cannot overturn or produce airborne spray. First-order numerical diffusion and the coarse alongshore spacing limit small-scale hydrodynamic detail; the lace texture supplies visual detail. Refraction remains the existing optical path implementation. Performance depends on substep count, mesh resolution and GPU; no frame-rate guarantee is implied.


### Dock 参数工作区

交互窗口使用 `egui_dock 0.18`（匹配 egui 0.33），替代原有浮动参数窗口：

- 左侧：Ocean controls，下方 Jacobian foam。
- 中央：Ocean view，按实际区域和显示缩放比例创建渲染目标；拖动旋转相机，滚轮缩放。
- 右侧：Shallow water (SWE) / Water optics 标签，点击切换。

可以拖动标签重新停靠、拖动分隔线调整宽度，顶部 **Reset layout** 恢复默认布局。参数区各自滚动，不会触发相机旋转或缩放。为避免丢失入口，标签不提供关闭或浮动到独立窗口的操作。布局在当前运行期间保留，重启恢复默认；已有参数保存方式保持不变。命令行离屏截图仍输出完整海面，不包含 Dock 界面。


### 近岸小波法线

SWE 面板的 **Small-wave normals → Normal detail strength** 控制近岸视觉细节，默认 1，范围 0～3，0 为关闭。调整即时生效，不重置 SWE；参数随其他设置保存。CLI 对应 `--shore-normal-strength`（放在 `--beach` 后）。

片元着色器读取未过滤 FFT 水位，以 1 格与 4 格跨度的中心差分坡度之差构造近似高频细节；它独立于 SWE 输入过滤，不是严格的频域带通。细节按近岸混合权重补入法线，在 2～25 厘米水深内渐隐，并按像素足迹抑制远处闪烁。只影响水面反射、高光及表面光照，不修改几何位移、水位、流动、泡沫生成和沙滩残留。需启用 Water optics 的 Sky reflection and highlights；物理调试视图保持原样。这不是短波在 SWE 中的真实传播或折射。


### SWE 一阶／二阶与 GPU 计时

**Shallow water (SWE) → SWE solver** 可切换 **First order (Euler)** 和 **Second order (MUSCL + RK2)**。默认保留一阶，选择会保存。切换会重置近岸水体、泡沫和湿沙，FFT 时间继续推进。两种模式使用相同 256² 网格和 CFL 子步条件，二阶每子步执行两阶段并额外使用 3 MiB 预测状态缓存。

面板显示最近异步采样的 **SWE GPU 毫秒数、子步数、推进的模拟毫秒数及每子步耗时**。时间戳覆盖本帧首个 SWE pass 开始至最后一个 pass 结束，包括中间参数拷贝、子步状态拷贝和泡沫/湿沙更新；不包含 FFT、渲染、CPU 提交、末尾状态拷贝及计时读回。只有一个读回在途，未完成时跳过采样，不阻塞帧循环。暂停/关闭显示 idle；不支持 timestamp query 的显卡显示 unsupported，不以 CPU 时间冒充 GPU 时间。

对比时先将 **Normal detail strength** 设为 0，使用相同 FFT、过滤参数、网格和时间倍率。切换后等待波重新到岸；需要同一相位时点击 Ocean controls 的 Restart time。比较耗时时同时检查子步数，避免帧长变化造成工作量差异。二阶仍是非色散 SWE，不能恢复网格无法分辨的波。

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-order 2 --shore-normal-strength 0
```

GPU 验证包含两种模式的静水保持、局部脉冲质量与输运、湿干及退水泡沫、极端坡度、切换重置，以及 4 米波长小振幅波在平底上的传播误差检查。该波形测试每波长 16 格，不代表默认沿岸 1.5 米格距也能分辨 4 米波。


### 水面泡沫与沙滩残留颜色

SWE 面板新增 **Foam colors**：**Water foam** 控制水面泡沫（含远海白沫），**Stranded foam (sand)** 控制退水后的沙滩残留泡沫。默认水面近白，残留为较暗的冷灰蓝，便于区分。**Distinct defaults** 恢复这组配色，**Match previous colors** 恢复原来的近白配色。颜色即时生效、自动保存，不重置水体或残留历史；光照和水下吸收仍会影响最终显示色。CLI 对应 `--water-foam-color R,G,B` 和 `--stranded-foam-color R,G,B`，使用 0～1 线性 RGB，放在 `--beach` 后。


残留可见性修正：沙滩残留现在以浓度调节透明度，纹理只调制斑驳程度，避免低浓度残留被纹理阈值整体抹掉。近岸水面覆盖改用采样的 SWE 水深，避免较粗的水面三角形横跨干格子遮住残留。**Identify foam: water green / sand red** 可用绿色标记水面泡沫、红色标记真正的沙滩残留，**Distinct defaults** 恢复正常配色。较宽的临水泡沫带通常仍属于水面泡沫；残留是靠陆一侧的细窄退水痕迹。


### 泡沫面密度调试

在 **Shallow water (SWE)** 面板点击 **Debug foam density: green / red**，或在 **Shore view** 选择 **Foam density (green / red)**。CLI：`--beach --shore-view 6`。水面与沙滩残留使用同一色标：低密度绿色，中间黄色，达到参考值及以上红色；无泡沫保留背景。调试颜色不经过泡沫纹理、光照或远海白沫混合，低于 0.01 的极弱密度淡入背景。正常 Shaded 视图保留已有材质。

- **Foam generation / s**：有效泡沫生成速率，设为 0 可观察已有泡沫的输送与汇聚。
- **Shore foam decay / s**：每秒指数衰减系数，0 表示关闭水面泡沫衰减。
- **Foam compression threshold / s**：生成模型中压缩增强项的阈值；不改变守恒输送本身的汇聚效果。
- **Density color reference**：红色对应的密度，默认 2；只调整显示映射，不裁剪模拟密度。

这些参数即时生效；需要公平比较时使用 **Reset shallow water, foam and sand** 或重启时间。退水残留由干格子剩余泡沫量沉积，沿用 10 秒衰减时间与再淹没洗除。此阶段不新增孔隙纹理模型。GPU 验证覆盖两种求解模式的泡沫总量守恒、平移、汇聚增密、扩散减密、指数衰减及干湿转换。


### FFT 初始化与输入增益

**Shallow water (SWE) → FFT input → FFT to SWE gain**，范围 **0～3**，默认 **1**。该系数同时乘到 FFT 水位扰动与两个水平动量分量上，并同时用于全域初始化和持续边界输入；远海 FFT 的显示不受此系数影响。它与原有 **Height gain** 相乘，0 会把 SWE 重置为静水并关闭后续输入。参数自动保存；修改后即使暂停也重新初始化 SWE，清除泡沫和湿沙历史。

初始化取当前时刻、经过当前过滤参数的 FFT 场，不使用上一帧 FFT，也不再等待 6 秒渐强。水深由“FFT 水位减地形高度”得到并限制为非负，干区动量清零；极浅水保留速度保护。因此强增益受安全限幅时，不保证实际振幅严格增至三倍。初始化不是完整的浅水稳态解，之后仍会调整传播相位、建立冲岸和泡沫。

CLI 示例：`cargo run --release --features ocean --bin ocean -- --beach --swe-input-gain 1.5`。
