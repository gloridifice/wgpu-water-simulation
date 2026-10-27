# 三维 FFT 海洋（wgpu + egui）

在独立窗口里运行的海面演示。CPU 初始化 Phillips 波谱，GPU 执行频谱演化、二维逆 FFT 和水面绘制。默认 128×128 个频率、128 米周期块，渲染 3×3 个重复块。`cargo run` 默认启动此程序；CPU PIC / 浅水 TUI 可通过独立的 `water-simulation` 二进制启动。

## 运行

```powershell
Set-Location G:\MyProjects\Rust\water-simulation
cargo run

# 优化构建，用于视觉和性能对比。
cargo run --release

# 改变频谱分辨率、风速与随机种子。
cargo run --release --features ocean --bin ocean -- --resolution 256 --wind 18 --seed 7
```

需要支持计算着色器和顶点阶段存储缓冲区的图形设备；通过 wgpu 使用本机后端，素材随仓库提供。首次构建会下载图形相关依赖。`ocean` feature 默认启用；使用 `cargo run --no-default-features --bin water-simulation` 可启动 CPU TUI 并避免编译图形依赖，再加 `-- --shallow` 启动一维浅水模式。

| 操作 | 效果 |
| --- | --- |
| 鼠标左键拖动 | 围绕当前观察目标旋转相机 |
| 滚轮 | 拉近 / 拉远 |
| W / S | 按当前朝向在水平面上前进 / 后退 |
| A / D | 按当前朝向在水平面上向左 / 向右平移 |
| Space | 暂停 / 继续 |
| C | 开关水平位移，比较圆润与尖锐浪峰 |
| F | 开关 Jacobian 泡沫显示 |
| ↑ / ↓ | 增减位移幅度，0.1×～2× |
| R | 相机、位移幅度、泡沫显示和时间重置；保留暂停状态 |
| Esc / Q | 退出 |

WASD 在鼠标位于渲染视图内、窗口有焦点且未编辑参数时生效。按住连续移动，相机和观察目标一起平移，高度和俯仰角不变；斜向移动不加速。移动速度为当前相机到观察目标距离的 0.5 倍／秒，使用现实时间，不受暂停和播放倍率影响。平移位置随相机保存，旧配置从原点开始；R、重置相机和沙滩相机预设均恢复对应的初始位置。

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
| 波高增益 / Horizontal choppiness / Show foam | 渲染位移倍率、水平位移开关、泡沫显示 |

网格通过周期双线性插值读取 FFT 高度与位移；增加网格密度不会凭空增加频谱中的波。网格比 FFT 粗时可能漏掉短波形状。Apply 会重建计算缓冲区与网格绑定，保留当前时间及暂停状态；尺寸改变时按比例调整相机距离。面板输入优先，不会同时旋转相机或触发海面快捷键。

命令行也支持 `--amplitude 0.002 --phillips-length 20 --wind-angle 45 --mesh-resolution 192`。旧的 `--wind 14` 仍可用，作用是设置 `L=U²/g`；若同时提供 `--phillips-length`，后出现的选项生效。

## 扫描海岸模型与可调 SWE 区域

新增 **模型与区域** Dock 页签。模型场景使用 `assets/models/challaborough_pillbox.glb` 显示岩岸，使用预烘焙的 `.sweh` 高度场驱动 SWE；保留原沙滩／平面场景，不改变默认启动场景。资源来源、CC BY 4.0 署名、裁切面清理和磁盘格式见 [`assets/models/README.md`](assets/models/README.md)。

```powershell
# 首次生成磁盘高度场；已有输出不会被覆盖。此步骤不需要 GPU。
cargo run --release --bin terrain-bake

# 启动模型预设，避免覆盖个人设置。
cargo run --release --bin ocean -- --model --no-settings

# 设置包围盒中心、尺寸和网格精度；覆盖参数写在场景预设后面。
cargo run --release --bin ocean -- --model --no-settings --swe-center 0,4,0 --swe-size 128,64,128 --swe-resolution 512

# 专项真实 GPU 验证，不依赖扫描资产，也不读写个人设置。
cargo run --release --bin ocean -- --verify-terrain
```

模型预设会覆盖会话中的场景参数，类似 `--beach`。恢复自己的已保存模型摆放时只用 `cargo run`，不要重复加 `--model`。模型参数、已应用包围盒、开放边界和显示开关进入会话及命名配置；旧配置缺少模型字段时默认关闭，继续原有场景。待应用草稿不保存。命令行快照仍使用显式参数，而不是个人配置。

### 离线资产与运行时重建

离线工具以约 0.05 个模型单位的格距生成 984×890 单元中心高度及有效覆盖标记；初始化时从磁盘加载一次并核对源 GLB SHA-256。修改区域时不重新读 GLB、不逐三角形求交。目前在 CPU 对驻留源高度场做 O(N²) 重采样，再上传静态 GPU 床面缓冲；求解子步只读该缓冲。它不是 GPU 重烘焙，也不在每帧重采样。控制台的重建毫秒数是 CPU 重采样及上传调用耗时，不是 GPU timestamp。

有效扫描区域使用模型高度，不与原沙坡处处取最大值。扫描包围盒外沿最近边缘高度，在可调的**海床衔接宽度**内平滑过渡到水体光学面板设置的海底深度；无效内部采样使用该背景深度。补充海床不是当地实测水下地形，也不是自动修复扫描洞穴。俯视最高表面只构成单层地形代理，不能模拟悬挑下穿流、洞穴与屋顶同时有水或真实三维流体破碎；纹理浪花只做视觉补充。

### 参数与应用语义

| 参数 | 含义与生效方式 |
| --- | --- |
| 启用模型 | 应用后切换模型／原场景，重新初始化近岸历史 |
| 模型位置 X/Y/Z（米）、统一比例、旋转 Y（度） | 调整摆放；统一比例是米／模型单位。应用后从源高度场重采样，不重新离线烘焙。俯仰／翻滚不支持，需要重新烘焙 |
| 海平面高程（米） | 与模型位置 Y 使用同一高程基准；内部渲染和求解以该海平面为 Y=0。修改是静态场景编辑并重置，不是动态潮汐 |
| 包围盒中心／尺寸 X/Y/Z（米） | XZ 决定实际二维计算域；Y 只用于线框和范围检查，不截平床面，不是三维网格，也不是封闭顶盖 |
| 按模型范围自动覆盖 | 加载资产后，根据变换后的包围盒和松弛带自动计算近方形区域，留出绕流余量；先修改草稿，再点击应用 |
| 每轴网格数 | 在 SWE 页签选择 64～1024；独立于离线高度场与 FFT。模型场景另绘制与 SWE 同密度的局部水面网格 |
| 四边开放开关 | 顺序 −X、+X、−Z、+Z；开启是零梯度开放边界加 FFT 松弛输入，关闭为反射边界。开放边界允许泡沫与泡沫动量流出，不导入外部泡沫 |
| 松弛带宽（米） | 每个开放边缘的过渡宽度，必须小于 X/Z 最小尺寸的一半。模型及主要交互区应避开该带；角点取最大权重，不重复叠加 |
| 海床衔接宽度（米） | 默认 12 米，扫描外人为海床的过渡距离；应留在计算域及松弛带以内 |
| 显示包围盒 | 橙色三维线框；仅外观 |

模型和范围控件先编辑草稿，点击 **应用模型与区域** 后才生效。XZ 范围、模型变换、水位、边界、补充海床或精度变化会重建／重新初始化近岸水体，清空近岸泡沫与湿润历史；FFT 时间和远海泡沫继续推进。仅改变 Y 显示范围和线框开关不重置。水体光学页的海底深度控件仍为即时生效，模型场景下修改会即时重建补充海床并重置。暂停时也能应用；尚未实现不同计算网格之间的守恒历史迁移。

模型内部及主要交互区以 SWE 水面为主，外缘渐变回 FFT；耦合仍是 **FFT → SWE 单向**。保留原输入的水位／两个动量通道、共同增益与短波过滤。当前开放处理不是严格无反射特征边界；需要结合输入波长调整松弛带，尚未标定反射系数。提高网格精度也不会修复深水 FFT 与非色散 SWE 的相速差异。

模型同时写入不透明颜色／深度预绘制和最终场景，水下部分参与现有光程积分。近岸水面插值忽略干地高度，泡沫输送在未淹没地形处阻挡；湿润和残留只投影到接近床面的模型表面，不涂满整个垂直岩壁。没有接入独立的岩壁水痕模型。

可用 `--model-position X,Y,Z`、`--model-scale N`、`--model-yaw DEG`、`--sea-level M`、`--swe-center X,Y,Z`、`--swe-size X,Y,Z`、`--swe-resolution N`、`--swe-sponge M`、`--swe-open-edges 1,1,1,1`、`--hide-swe-box` 设置初始参数。模型比例与默认水位仅供预览，未做现场测量校准。

`--verify-terrain` 使用生产 GPU 求解路径，覆盖一阶／二阶的解析斜床上传、干湿静水、运动水脉冲水量守恒、Y 范围／线框编辑不改变暂停状态、暂停时改域重采样，以及四个开放边缘的静水保持。CPU 库测试覆盖磁盘资产往返、源哈希不匹配／截断拒绝、旋转缩放后解析床面采样和无效覆盖。完整 `--verify` 也包含这些 GPU 检查。

## GPU 浪花粒子

左下 Dock 的 **浪花粒子** 页签控制离开水面的碎浪／水滴，和水面泡沫分开。默认关闭，旧配置缺字段时也保持关闭。

```powershell
# 有界预览，不写个人配置；浪花选项放在场景预设后。
cargo run --release --bin ocean -- --beach --spray --no-settings --frames 600
# 查看 GPU 发射强度（绿低红高），同时启用浪花。
cargo run --release --bin ocean -- --beach --spray-debug --no-settings --frames 600
```

恢复个人参数时用 `cargo run`，不要重复指定 `--beach`；单独加 `--spray` 只覆盖开关，不重置已保存的发射参数。调试视图显示最近一次实际模拟帧的源强，暂停时保留；关闭调试后恢复粒子绘制。

发射信号存储在 GPU storage buffer。每个完成的 SWE 子步计算水面坡度、正向上升和有效水深门控，再按单元面积、实际模拟时间积分；二阶预测阶段不重复生成。粒子 compute 读取信号并保留小数余量，GPU 更新粒子列表和间接绘制参数。运行时不回读信号、粒子或数量，不由 CPU 组装发射列表。发射不读取水面泡沫密度，浪花也不反向修改水体、泡沫或沙地状态。

| 参数 | 默认值与范围 | 生效方式 |
| --- | --- | --- |
| 发射密度 | 150，0～2000 粒子/平方米/模拟秒 | 实时影响新增量，0 停止生成 |
| 坡度阈值 | 0.03，0～2（无量纲），固定过渡宽度 0.1 | 水面高程梯度，不是水深梯度 |
| 上升阈值 | 0.02，0～5 米/模拟秒，固定过渡宽度 0.2 | 只有上升且陡峭的水面发射 |
| 最小水深 | 0.02，0.001～1 米 | 低于此值不发射，到两倍水深达到满权重 |
| 向上／前向速度 | 2／0.5，各 0～10 米/模拟秒 | 新生粒子叠加水流水平速度及少量随机扰动 |
| 最长寿命 | 1.2，0.05～5 模拟秒 | 新生粒子实际寿命为其 75%～100% |
| 尺寸 | 0.2，0.01～2 米 | 基础四边形直径，最终直径 = 尺寸 × 随机尺寸倍率；仅影响新生粒子 |
| 随机尺寸倍率下限／上限 | 0.65／1.35，各 0～2，无量纲 | GPU 在区间内均匀随机；下限不能超过上限，相等时固定倍率。实时影响新生粒子并保存，不重置模拟；旧配置缺字段时保持原有随机范围。尺寸 2 米、倍率 0.5～1 对应实际 1～2 米；最终直径可超过 2 米 |
| 空气阻力 | 0.5，0～10/模拟秒 | 实时影响全部粒子；0 为重力弹道 |
| 软交界距离 | 0.15，0.001～1 米 | 只影响与场景交界的深度淡出 |

参数随会话和命名配置保存，不保存运行中的粒子。暂停冻结运动、年龄和发射；关闭释放浪花资源；重置时间、重建网格／波谱或改变近岸域会清空浪花，碰撞体编辑也清空旧粒子。粒子在落水、触地、寿命耗尽或离开有效近岸域时消失。水面泡沫的外观高度不改变粒子运动。

粒子池最多 65,536 个，先保留存活粒子，再填充空余槽；满池直接丢弃新增请求，不欠账、不覆盖存活粒子。信号和余量合计约每个 SWE 单元 40 字节，双粒子列表约 6 MiB，另有少量参数及纹理。设备限制或分配失败会禁用浪花并显示错误，不改用 CPU 粒子路径。

首版只覆盖纯 SWE 区域（FFT 混合权重不大于约 0.001），不在远海或 FFT 混合带喷射。纹理采用 Effekseer 官方 Pierre 样例中的 CC0 `Splash.png`，按原图 Alpha 拆成四种随机水片／碎滴，使用独立透明边和完整 mip 链避免串图及远处闪烁；原图、作者、许可和离线烘焙说明见 [assets/spray/README.md](assets/spray/README.md)。绘制使用预乘透明度、已保存的场景深度和软交界；没有透明粒子排序、OIT、风场或独立水雾层。高度场碰撞不表示任意三维模型表面的精确碰撞，也不能生成真实翻卷浪。

完整 `--verify` 包含发射门控、世界坐标/速度的 GPU 信号布局、面积/时间积分、小数发射、容量饱和与复用、暂停/重置、落水，以及两种尺寸下的真实粒子深度遮挡和淡出检查。一阶／二阶开启浪花后，既有水体与泡沫状态仍保持一致。仅验证程序允许末尾读回检查结果。

## 性能面板

左下 Dock 的 **性能** 标签页与 **远海泡沫**、**浪花粒子** 并列，使用两列表格显示指标和数值；原浅水面板中的 GPU 耗时行已移至此处。数据每 1 秒更新一次，CPU 指标为该窗口内每帧平均值，GPU 指标只统计新读回且匹配当前求解阶数的样本。

| 指标 | 测量范围 |
| --- | --- |
| 帧率、端到端帧间隔 | 相邻绘制开始之间的现实时间，含垂直同步、GPU 背压与事件循环等待；不是输入到屏幕的延迟，不使用模拟步长的 0.1 秒截断 |
| CPU 帧处理 | 绘制入口到提交、present 返回；含界面、配置处理、编码与阻塞等待，不代表 GPU 完成 |
| 获取画面 | 获取交换链图像的 CPU 等待 |
| 界面 CPU | egui 布局与交互，不含界面绘制编码 |
| 模拟 CPU | FFT、远海泡沫、SWE 与浪花源/粒子的 CPU 指令编码及参数上传，不是 GPU 执行时间 |
| 渲染编码 CPU | 场景及 egui 绘制指令编码 |
| 提交与呈现 CPU | 命令缓冲完成、队列提交与 present 返回，不等待 GPU 完成或屏幕输出 |
| SWE GPU、SWE GPU／子步 | 真实 GPU 时间戳；包含 SWE、近岸泡沫与中间子步复制，开启浪花时包含全部发射信号 pass；不含 FFT、远海泡沫、粒子更新/绘制和首尾复制 |
| SWE 模拟推进、子步／帧、求解阶数、GPU 采样数 | GPU 样本对应的实际模拟时长和子步统计；模拟时长不是计算耗时 |

时间单位为毫秒；具体说明在行名 hover 中。GPU 时间戳异步读取，不为计时阻塞每帧；设备不支持、读取失败、暂停／时间倍率为零／关闭 SWE 和等待新求解器样本均有对应状态。切换阶数或进入空闲时，丢弃当前尚未发布的旧 GPU 样本；表格仍按每秒一次刷新。

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

## 沙地纹理

沙滩场景的岸上沙地与水下海床共用 ambientCG **Ground061** 2K 材质（CC0），资源及来源说明位于 `assets/sand/Ground061/`，编译时内嵌，无需访问下载目录。

颜色贴图按 sRGB 解码；OpenGL 法线、粗糙度和环境遮蔽（AO）按线性数据读取。初始化时生成完整 mip 链，使用重复、三线性与各向异性过滤。沙地和导入模型的颜色贴图仅在 CPU 解码 JPEG 并上传原始层，后续 mip 由 GPU 在线性空间逐级平均、写回 sRGB；数据贴图仍使用原有 CPU 过滤。沙滩参数组中的 **沙地纹理尺寸（米）** 控制每块纹理沿沙坡和沿岸覆盖的长度，范围 0.25～128 米，默认 4 米；数值越大，沙纹越大、重复越少，可先尝试 16～32 米。修改实时生效、不重置模拟，并随当前状态及命名配置保存；旧配置缺少此字段时仍使用 4 米。这是视觉尺度设定，素材未提供实测尺寸。法线仅改变光照，不改变海床几何或 SWE 床面。

同组的 **沙地纹理旋转（度）** 范围为 -180～180 度，默认 0 度；绕世界原点的沙坡平面坐标旋转，正角将纹理横轴从坡向 +X 转向沿岸 +Z。颜色、法线、粗糙度及 AO 同步旋转，法线光照方向随之调整。实时生效、不重置模拟，并随配置保存；旧配置默认不旋转。

湿润历史仍控制沙地变暗，并降低材质粗糙度；AO 仅影响环境光部分。保留残留泡沫及湿润、密度调试视图，非沙滩场景仍使用光学检查棋盘格。当前高光仍为简化光照模型，并非完整 PBR；未接入位移贴图。

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

基础 FFT 生成周期海面，本身不求解岸边碰撞。沙滩和扫描模型场景叠加局部二维 SWE、泡沫输运及湿润历史，详见相关章节；耦合方向为 FFT → SWE。仍不包含真实翻卷或三维空气卷入，水平位移过大使网格折叠并不等于物理破浪。

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

令实际水平位移 `d = lambda * gain * D`。lambda 使用渲染中的负号（默认 -1.15），不可取绝对值，也不能漏掉 波高增益。

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

启动时添加 `--beach`，或点击 浅水模拟（SWE） 面板的 **Beach scene**，即可载入沙坡、水体光学参数和俯视斜向岸线的相机预设。预设使用较小波幅，手动 S 为零、天光开启，方便比较从可见沙底的浅水到深色深水的变化。

沙坡是实际几何平面 `y = -beach_slope * (x - shore_x)`，负 X 为陆地、正 X 为深水。面板 **沙滩坡度（高／宽）** 控制坡度，**岸线 X（米）** 移动平均水位岸线，**沙滩相机** 恢复观察角度；取消 **倾斜沙滩** 返回水平棋盘海底。沙坡启用时平底深度控件禁用，实际水深由坡面决定。上述设置支持自动保存。

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

所有海洋参数、未 Apply 的频谱草稿、泡沫参数、水体参数（包括天光）、相机、暂停和播放速度，默认保存于 `%LOCALAPPDATA%\water-simulation\ocean-settings.json`。停止编辑 0.5 秒后自动保存，窗口失去焦点和正常退出时立即保存。时间与泡沫历史不保存。

左侧 **配置保存** 标签与 **海洋参数** 并列，使用 egui 原生可选按钮组成单选列表，点击配置即载入。下方只有 **新配置 / 删除配置** 和 **保存配置**：新配置展开名称输入及 **确定 / 取消**，确定后保存当前参数；删除配置展开对当前配置的删除询问及 **确定 / 取消**，确定删除后切回默认配置；保存配置用当前参数覆盖所选命名配置。右键配置按钮可 **重命名**，同样通过名称输入及 **确定 / 取消** 完成。配置名称不能为空、不能重复，也不能使用保留名称“默认配置”。自动保存只更新当前工作状态，命名配置仅在手动新建或保存时更新；重启会恢复最后的工作状态，并保留全部命名配置。面板只在出错时显示错误提示，操作说明放在按钮的 hover 提示中。

首次升级时，原有本地配置会封存为只读 **默认配置**；没有已有配置时使用程序初始默认值。默认配置不能重命名、删除或覆盖，点击 **默认配置** 可恢复全部参数，再次点击当前配置也会重新载入。载入任意配置会恢复其中的已应用波谱和未应用草稿，并重置模拟时间、泡沫与浅水历史；草稿仍需 **应用更改** 后生效。`--no-settings` 下禁止新建、删除、保存和重命名，仍可恢复本次启动时的初始配置。

使用 `--settings PATH` 指定独立配置，`--no-settings` 禁止读取/写入配置。明确传入的 CLI 参数覆盖对应保存值，其余字段继续恢复。`--snapshot`、`--verify` 始终使用默认值加 CLI 参数，不读写用户配置。

每次启动会恢复上次使用的配置选项，以及该配置下最后自动保存的工作状态（包括尚未手动保存到命名配置的调整）。配置选项在成功载入或新建后记录，重命名不影响恢复；删除当前配置后，下次启动使用默认配置。自动保存不会覆盖命名配置快照。旧文件没有记录选项或记录已失效时，选中默认配置并保留原有工作状态与命名配置。恢复个人参数时直接运行 `cargo run`，不要额外传入 `--beach` 等场景覆盖参数。

旧配置不含 sky 段时默认关闭天光，以保留原效果；可勾选 Enable sky scattering 或加 `--skylight`。新配置默认启用天光。设置采用临时文件加原子替换写入，损坏或不支持的文件在重新保存前会保留 `.invalid-时间戳.json` 备份；写入失败会显示状态并重试，不会静默覆盖为成功。

σ_t 和手动 S 使用 egui 原生颜色选择器配合 Strength，实际线性系数为 `color * strength`，可展开精确 RGB 输入；颜色表示通道比例，强度不会受颜色选择器 0～1 范围限制。天空颜色和反照率直接使用 0～1 线性 RGB，天空另有独立强度。

## Local 2D shallow water, FFT offshore and waterborne foam

```powershell
cargo run --release --features ocean --bin ocean -- --beach --no-settings
cargo run --release --features ocean --bin ocean -- --beach --snapshot shore.png --time 24
cargo run --release --features ocean --bin ocean -- --beach --no-settings --shore-view 6 --snapshot foam-density.png --time 27
cargo run --release --features ocean --bin ocean -- --beach --no-shore
```

The independent **浅水模拟（SWE）** panel contains the SWE, foam, wet-sand and beach controls and opens by default. FFT Phillips power, wind direction and spectral length now drive both the ocean and the SWE inlet. 波高增益 scales inlet elevation and both momentum components together. The independent shore amplitude/period controls have been removed; old saved fields are ignored. The retired `--shore-amplitude` / `--shore-period` flags report a migration message. The entire SWE patch starts with FFT elevation and momentum; repeated run-up and foam still need time to develop. CLI overrides belong after `--beach`, which installs the beach preset.

| 浅水调试视图 | Meaning |
| --- | --- |
| 0 | Shaded |
| 1 | Geometric water depth; white at 3 m |
| 2 | Cross-shore velocity: blue incoming, orange outgoing |
| 3 | Waterborne foam concentration |
| 4 | Stationary sand wetness |
| 6 | Waterborne foam density: green (low) to red (reference or higher), no texture |

### Solver and rendering

`shore_compute.wgsl` advances a rectangular, world-space finite-volume grid (default 256²). It offers the original first-order hydrostatic Rusanov + Euler solver and a second-order MUSCL + SSP-RK2 solver. The latter reconstructs free surface, velocity and bed using a generalized minmod limiter (theta=1.3), with hydrostatic face corrections and a matching centered bed source. Reconstruction falls back to first order near dry cells and patch boundaries. Both retain wet/dry handling and the 12 m/s velocity safeguard; RK2 stages include boundary relaxation and bottom friction. Foam generation/decay and ground memory update once per completed substep and remain split first-order source updates. This is not a port of the reference project's KP07 solver.

The stored variables are `(h, hu, hv, surface_foam_density)`. Foam is an effective amount per unit surface area in arbitrary units, not a depth-weighted concentration or physical mass density. A separate conservative upwind flux transports foam using shared face velocities: convergence concentrates it and divergence dilutes it, without creating foam. Both orders use first-order spatial foam transport; second-order SWE uses RK2 for its transport time integration. Foam is not divided by water depth or clipped at 1. The offshore sponge removes foam; cells at or below 3 mm water depth discard their remaining foam without leaving ground residue. Closed boundaries have zero foam flux.

Generation combines the original rising-water source (scaled by the background multiplier, default 0.15) with a directional front source: sample free-surface elevation (bed height plus water depth) ahead of the flow and emit where the surface falls. The front source does not require rising water or nearby dry land. Both retain wet-depth and offshore-sponge gating. This is a visual proxy, not resolved air entrainment; the criterion follows surface slope, so retreat can also emit when its forward water surface is lower; dry samples are excluded. Generation and exponential decay are integrated analytically with a frozen source per completed substep. Density may exceed the color reference, retaining accumulated amount rather than clipping it.

The patch spans at least 64 m across the beach and 384 m along it for a 128 m FFT tile. Its inland coverage expands with a conservative spectral height bound and 波高增益, up to 64 m inland. Gentle slopes can also expand the offshore region. Alongshore spacing is 1.5 m at the default tile size. The bed remains the existing straight sand slope, shared by simulation and rendering. Landward and alongshore boundaries reflect; the offshore edge samples FFT elevation and matching depth-integrated momentum in a relaxation zone occupying the outer 20% of the patch. There is no arbitrary obstacle editor or terrain SDF in this version.

Substeps use a conservative speed/depth estimate and the rectangular-grid bound `dt <= 0.2 / ((12 + sqrt(g * depth_bound)) * (1/dx + 1/dz))`. A frame advances at most 0.1 simulated seconds and 128 substeps; excess time from a long stall is discarded. This estimate is intended for the built-in terrain and wave controls, not arbitrary externally injected states. Boundary forcing, thin-film removal and velocity safeguards mean the entire open scene is not a closed conservative system.

The water mesh reads the SWE free surface and fades to the existing FFT displacement between the configured blend depth and 2.5 times that depth. Normals come from the combined surface. The original three FFT render channels remain unchanged; three additional channels drive the SWE inlet. The coupling is **one-way FFT → SWE**: shore reflections and currents do not modify the ocean spectrum. Changing mesh density affects how much of the simulated front is visible. Water thinner than 8 mm is discarded by the water material.

Each substep writes a separate output buffer and copies it to the previous-state buffer; water and ground use a fixed output binding. Parameter copies are ordered in the command encoder so substeps cannot accidentally all read the final uniform. The three state buffers (current, previous and RK predictor) use 15 MiB total at 256², scaling with the square of the selected resolution. There is no per-frame CPU readback. The copied lace image has a CPU-built mip chain in a read-only GPU buffer (about 5.3 MiB); shader sampling performs bilinear/trilinear filtering.

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

The three boundary spectra `(H, Qx, Qz)` receive the same configurable smooth low-pass. In **浅水模拟（SWE） → FFT 输入 filter**, **Filter short waves** enables it; **Reject wavelength <= (m)** sets the rejected wavelength limit, and **Transition width (m)** sets the smooth transition above that limit. The default is a fixed 12 m cutoff plus a 12 m transition: wavelengths <= 12 m are rejected and >= 24 m pass fully. Unlike the earlier automatic rule, these values do not change with FFT resolution. A zero transition gives a hard cutoff. Disabling the filter passes all FFT modes. The **3–6 m** preset admits more short waves for comparison, but the first-order SWE grid can still dissipate unresolved waves and does not recover their dispersion.

Filter changes apply immediately and reset SWE water/foam/sand history, while FFT time keeps advancing. The patch is reseeded from the current filtered FFT, rather than restarted as still water. The settings persist with the other controls. No additional short-wave normal detail is added. CLI equivalents (put overrides after `--beach`):

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-filter-cutoff 3 --swe-filter-transition 3
cargo run --release --features ocean --bin ocean -- --beach --no-swe-filter
```

The six spectra share the existing IFFT stages. SWE samples channels 3–5 at `(world / FFT_tile_length + 0.5) * FFT_resolution`, with periodic wrapping and bilinear interpolation. This matches the ocean's undeformed world-coordinate convention. It interpolates the previous and current FFT boundary frames at each SWE substep; no FFT or momentum readback to the CPU is required.

On reset, the entire patch samples the current filtered FFT elevation and momentum. The six-second startup ramp has been removed. 波高增益 multiplied by the SWE-specific input gain applies to elevation and momentum equally. For extreme inputs, a common safety scale limits elevation to half the local still-water depth and momentum magnitude to eight times that depth. The scale preserves their ratio, although those clipped cases no longer reproduce the raw FFT amplitude.

The relaxed target is `(max(eta-Bed, 0), Qx, Qz, 0)`, blended with weight `1-exp(-4*edge_weight*dt)`. Foam is generated inside the SWE region rather than imported from FFT. The existing Jacobian foam remains on the far ocean.

The boundary momentum matches the **linear deep-water FFT**. SWE remains a non-dispersive shallow-water model, so this coupling does not eliminate phase-speed differences or reflection for intermediate/deep-water components. Short-wave filtering, the relaxation zone and the visual transition reduce the mismatch but do not constitute a finite-depth dispersion solver.

Adding three channels doubles the FFT field/scratch storage and butterfly work relative to the original three-channel ocean. At 128² it adds 768 KiB across the two FFT buffers, plus 384 KiB for the previous boundary frame. Normal rendering still performs no CPU readback.

### Ground memory and foam appearance

Sand wetness is stationary and dries with the configurable time constant, default 18 seconds. Waterborne foam moves with the conservative flux and is removed when water depth falls to 3 mm or below. No foam is stored or rendered on exposed ground.

The water material uses two offset flow phases driven by the simulated velocity and coarse/fine Voronoi lace samples. Concentration changes an erosion threshold, opening holes as foam disappears. Mip filtering and derivative-smoothed thresholds reduce distant shimmer. F / Show foam affects waterborne foam without stopping its evolution.

The bundled `assets/shorewaves/voronoi_lace.png` comes from Reboot16 / Enrico K.'s Shorewaves project. Its MIT notice is retained in `assets/shorewaves/LICENSE.txt`. The solver and material implementation here are adapted to Rust/wgpu/WGSL and the existing FFT renderer; no Godot runtime is needed.

### Lifecycle and validation

Pause freezes simulation and texture flow. R, Restart time and **重置浅水、泡沫和沙地** reinitialize the local simulation from the current FFT and clear foam/sand history. Terrain, 波高增益, SWE input gain, blend-depth and enable changes also reset it; applying a new FFT spectrum rebuilds the coupled resources; foam-rate, drying-time and view changes preserve history. Settings persist, but simulation state does not. Snapshot mode prewarms up to 60 seconds of shore history (8 seconds offshore); later snapshots approximate a finite history window.

`--verify` exercises the actual GPU shader: lake at rest, local pulse mass conservation away from the inlet, propagation and foam transport in both axes, dry-cell foam removal and sand drying/rewetting, extreme slope/amplitude settings, terrain reset, run-up, finite/nonnegative states, pause, reset and disable. Raster verification prewarms 24 seconds and checks water history at two target sizes. The FFT test compares all six channels to a double-precision direct DFT and checks analytic oblique waves in both directions. An inlet probe checks production world-coordinate wrapping, time interpolation and gain against CPU samples. Zero FFT power must generate no shoreline flow/foam. Existing optics and sky tests remain enabled. `cargo test --all-features` covers configuration compatibility and the existing CPU suites.

The heightfield cannot overturn or produce airborne spray. First-order numerical diffusion and the coarse alongshore spacing limit small-scale hydrodynamic detail; the lace texture supplies visual detail. Refraction remains the existing optical path implementation. Performance depends on substep count, mesh resolution and GPU; no frame-rate guarantee is implied.


### Dock 参数工作区

交互窗口使用 `egui_dock 0.18`（匹配 egui 0.33），替代原有浮动参数窗口：

- 左侧：海洋参数 / 配置保存 标签，下方远海泡沫。
- 中央：Ocean view，按实际区域和显示缩放比例创建渲染目标；拖动旋转相机，滚轮缩放。
- 右侧：浅水模拟（SWE） / Water optics 标签，点击切换。

可以拖动标签重新停靠、拖动分隔线调整宽度，顶部 **Reset layout** 恢复默认布局。参数区各自滚动，不会触发相机旋转或缩放。为避免丢失入口，标签不提供关闭或浮动到独立窗口的操作。布局在当前运行期间保留，重启恢复默认；已有参数保存方式保持不变。命令行离屏截图仍输出完整海面，不包含 Dock 界面。


### 近岸小波法线

SWE 面板的 **小波法线 → 法线细节强度** 控制近岸视觉细节，默认 1，范围 0～3，0 为关闭。调整即时生效，不重置 SWE；参数随其他设置保存。CLI 对应 `--shore-normal-strength`（放在 `--beach` 后）。

片元着色器读取未过滤 FFT 水位，以 1 格与 4 格跨度的中心差分坡度之差构造近似高频细节；它独立于 SWE 输入过滤，不是严格的频域带通。细节按近岸混合权重补入法线，在 2～25 厘米水深内渐隐，并按像素足迹抑制远处闪烁。只影响水面反射、高光及表面光照，不修改几何位移、水位、流动、泡沫生成和沙滩残留。需启用 Water optics 的 Sky reflection and highlights；物理调试视图保持原样。这不是短波在 SWE 中的真实传播或折射。


### SWE 一阶／二阶与 GPU 计时

运行时按水体／泡沫阶数组合缓存专用计算管线，去掉未启用的高阶分支；首次使用一种组合可能有短暂编译开销。MUSCL 泡沫在同一计算 pass 内复用按轴邻域、中心斜率和地形阻挡查询，不额外增加网格缓冲区，也不改变默认阶数、CFL、生成、衰减或材质。

**浅水模拟（SWE） → SWE solver** 可切换 **First order (Euler)** 和 **Second order (MUSCL + RK2)**。默认保留一阶，选择会保存。切换会重置近岸水体、泡沫和湿沙，FFT 时间继续推进。两种模式使用相同的所选网格和 CFL 子步条件，二阶每子步执行两阶段。

面板显示最近异步采样的 **SWE GPU 毫秒数、子步数、推进的模拟毫秒数及每子步耗时**。时间戳覆盖本帧首个 SWE pass 开始至最后一个 pass 结束，包括中间参数拷贝、子步状态拷贝和泡沫/湿沙更新；不包含 FFT、渲染、CPU 提交、末尾状态拷贝及计时读回。只有一个读回在途，未完成时跳过采样，不阻塞帧循环。暂停/关闭显示 idle；不支持 timestamp query 的显卡显示 unsupported，不以 CPU 时间冒充 GPU 时间。

对比时先将 **法线细节强度** 设为 0，使用相同 FFT、过滤参数、网格和时间倍率。切换后等待波重新到岸；需要同一相位时点击 Ocean controls 的 Restart time。比较耗时时同时检查子步数，避免帧长变化造成工作量差异。二阶仍是非色散 SWE，不能恢复网格无法分辨的波。

```powershell
cargo run --release --features ocean --bin ocean -- --beach --swe-order 2 --shore-normal-strength 0
```

GPU 验证包含两种模式的静水保持、局部脉冲质量与输运、湿干及退水泡沫、极端坡度、切换重置，以及 4 米波长小振幅波在平底上的传播误差检查。该波形测试每波长 16 格，不代表默认沿岸 1.5 米格距也能分辨 4 米波。


### 水面泡沫与沙滩残留颜色

SWE 面板的 **泡沫颜色 → 水上泡沫** 控制水面泡沫（含远海白沫）。**恢复默认泡沫颜色** 恢复近白配色。颜色即时生效、自动保存，不重置水体；光照和水下吸收仍影响最终显示色。CLI 使用 `--water-foam-color R,G,B`，范围为 0～1 线性 RGB，放在 `--beach` 后。


残留可见性修正：沙滩残留现在以浓度调节透明度，纹理只调制斑驳程度，避免低浓度残留被纹理阈值整体抹掉。近岸水面覆盖改用采样的 SWE 水深，避免较粗的水面三角形横跨干格子遮住残留。**区分位置：水绿／沙红** 可用绿色标记水面泡沫、红色标记真正的沙滩残留，**默认区分色** 恢复正常配色。较宽的临水泡沫带通常仍属于水面泡沫；残留是靠陆一侧的细窄退水痕迹。


### 泡沫面密度调试

在 **浅水模拟（SWE）** 面板点击 **密度调试：低绿／高红**，或在 **浅水调试视图** 选择 **泡沫密度（低绿／高红）**。CLI：`--beach --shore-view 6`。水面与沙滩残留使用同一色标：低密度绿色，中间黄色，达到参考值及以上红色；无泡沫保留背景。调试颜色不经过泡沫纹理、光照或远海白沫混合，低于 0.01 的极弱密度淡入背景。正常 Shaded 视图保留已有材质。

- **背景生成速率（每秒）**：控制背景源，受背景生成倍率缩放；前沿生成独立控制。此速率和前沿速率都设为 0 后，可观察已有泡沫的输送与汇聚。
- **水上泡沫衰减（每秒）**：每秒指数衰减系数，0 表示关闭水面泡沫衰减。
- **汇聚增强阈值（每秒）**：生成模型中压缩增强项的阈值；不改变守恒输送本身的汇聚效果。
- **密度颜色参考值**：红色对应的密度，默认 2；只调整显示映射，不裁剪模拟密度。

这些参数即时生效；需要公平比较时使用 **重置浅水、泡沫和沙地** 或重启时间。退水后清除水上泡沫，不再沉积沙地残留。此阶段不新增孔隙纹理模型。GPU 验证覆盖两种求解模式的泡沫总量守恒、平移、汇聚增密、扩散减密、指数衰减及干湿转换。


### FFT 初始化与输入增益

**浅水模拟（SWE） → FFT 输入 → FFT → SWE 增益**，范围 **0～3**，默认 **1**。该系数同时乘到 FFT 水位扰动与两个水平动量分量上，并同时用于全域初始化和持续边界输入；远海 FFT 的显示不受此系数影响。它与原有 **波高增益** 相乘，0 会把 SWE 重置为静水并关闭后续输入。参数自动保存；修改后即使暂停也重新初始化 SWE，清除泡沫和湿沙历史。

初始化取当前时刻、经过当前过滤参数的 FFT 场，不使用上一帧 FFT，也不再等待 6 秒渐强。水深由“FFT 水位减地形高度”得到并限制为非负，干区动量清零；极浅水保留速度保护。因此强增益受安全限幅时，不保证实际振幅严格增至三倍。初始化不是完整的浅水稳态解，之后仍会调整传播相位、建立冲岸和泡沫。

CLI 示例：`cargo run --release --features ocean --bin ocean -- --beach --swe-input-gain 1.5`。


沙地残留泡沫的沉积、存储、衰减、渲染及对应颜色、沉积水深控件已删除，湿沙历史与干燥时间保留。水上泡沫在水深不超过 3 毫米时清除，不留下地面残留；水面几何裁剪和求解器干湿保护仍使用各自阈值。旧配置中的 `stranded_foam_color`、`foam_deposit_depth_mm` 被忽略；旧沙地泡沫视图 5 加载为正常着色，水上密度视图保留编号 6。CLI 已移除 `--stranded-foam-color` 和视图 5。


近岸背景泡沫生成条件可在 **浅水模拟（SWE）** 调整：
- **背景生成起始阈值**：起始阈值，默认 0.25，范围 0～2。判断值为“水深上升速度 ÷ 局部浅水波速”，无量纲；调低更容易产泡。
- **背景生成过渡范围**：从起始阈值到完整生成权重的过渡宽度，默认 0.40，范围 0～2。因此默认在 0.25 开始、0.65 达到完整权重。调小会更快增强，0 表示严格超过阈值后立即达到完整权重。

两项参数即时生效、自动保存，不清空已有泡沫。静水在两项均为 0 时仍不产泡。它们只控制背景生成权重，不影响新增前沿源；实际泡沫量还受生成速率、背景倍率、压缩增强、水深、输送与衰减影响。老设置文件缺少这些字段时使用默认值。


## 前沿泡沫：有方向的压缩

前沿生成只保留**有方向的压缩**，不再提供检测方式切换。压缩强度为 `max(当前速度沿采样方向的分量 − 前方速度沿该方向的分量, 0) ÷ 实际采样距离`，单位每秒。同向追赶、迎面相遇可以触发，沿流向拉开和纯横向速度差不触发。

沿当前水流方向在采样距离的 1/4、1/2、3/4、1 倍处取样，分别计算压缩强度后取最大值。使用同一求解阶段的水体速度，按世界距离计算，兼容非正方形网格。双线性插值只计有水角点；越界及完全干燥的点跳过。当前流速不超过 0.02 m/s 时关闭检测，0.02～0.1 m/s 平滑开启。

在 **浅水模拟（SWE） → 前沿泡沫** 调整：

| 参数 | 默认值 | 范围 / 含义 |
| --- | --- | --- |
| 前沿生成速率（每秒） | 8 | 0～30，满权重时的生成速率；0 关闭 |
| 前沿采样距离（米） | 1 | 0.05～10 |
| 速度变化起始阈值（每秒） | 0.1 | 0～20，压缩强度超过后开始生成 |
| 速度变化过渡范围（每秒） | 0.5 | 0～20，从阈值到阈值加此范围平滑升至满权重；0 为严格大于阈值的硬切换 |
| 背景生成倍率 | 0.15 | 0～1，设为 0 可单独观察前沿源 |

例如相距 0.5 米的水流，同向速度从后方 2 m/s 降到前方 0.5 m/s，压缩强度为 3/s；反过来则为 0。强度映射为 0～1 的生成权重，不直接限制累计泡沫密度。背景源与前沿源相加后应用 3～30 mm 薄水抑制和外海边界淡出，每个完整 SWE 子步只添加一次。

调参实时生效、自动保存，不清空已有泡沫，不改变水体方程。旧配置中的 `foam_front_mode`、`foam_front_depth_drop`、`foam_front_gain_points` 和 `foam_front_power` 被忽略，其余参数正常加载。可将背景倍率设为 0、使用密度调试，并固定时间、相机、FFT 和 SWE 阶数观察效果。压缩判据是浅水产泡的近似信号，未解析实际空气卷入。

## 泡沫动量、追赶与汇聚

在 **浅水模拟（SWE）→ 泡沫动量与水流拖曳** 调整：

| 参数 | 默认值 | 范围 | 含义 |
| --- | --- | --- | --- |
| 泡沫空间输送 | 一阶 | 一阶／二阶 MUSCL | 仅 SWE 二阶（RK2）时启用选择；SWE 一阶始终使用原通量 |
| 启用水流拖曳 | 开启 | 开／关 | 关闭后保留独立惯性，用于隔离验证；生成、衰减和干格子清除仍有效 |
| 水流跟随倍率 | 0.8 | 0～1 | 泡沫目标速度为水流速度乘此倍率 |
| 速度响应时间（秒） | 0.35 | 0～20 | 速度向目标靠拢的时间常数，0 表示立即跟随 |
| 新生泡沫速度倍率 | 1 | 0～1 | 新生速度为当地水流速度乘此倍率；只影响新增泡沫 |

每格保存“泡沫量／面积”和“泡沫量／面积 × 水平速度”。两者使用相同的迎风权重跨格输送；格内速度等于动量除以泡沫量。因此快泡沫追上慢泡沫时会积聚、增密，并按泡沫量加权混合速度。比如前团泡沫量 0.2、速度 0.2 m/s，后团泡沫量 0.8、速度 1 m/s，充分混合后总量为 1，总动量为 0.84，平均速度为 0.84 m/s。局部密度允许超过 1；汇聚不会额外制造泡沫。

新生泡沫注入“新增量 × 新生速度”的动量，衰减按相同比例移除泡沫量与动量。拖曳加速度为“目标速度与泡沫速度之差／响应时间”。对每子步内固定的水流与生成率，生成、衰减、拖曳联合使用解析积分；仅有拖曳时，速度按指数靠拢目标。拖曳关闭后，跟随倍率与响应时间不影响惯性输送。跟随倍率为 0 且拖曳开启时才会逐渐停下。此模型不反作用于水体。

SWE 历史第四块保存泡沫面动量和第一套纹理位移，第五块保存第二套纹理位移与更新相位，缓冲区大小不变。泡沫量与两个动量分量共用守恒通量，在 RK2 预测、校正中共同推进；生成和拖曳只在完成子步时积分一次。输送速度上限沿用 SWE 的 12 m/s 与 CFL 步长约束。两套纹理位移使用同一泡沫速度场反向追踪并累计，材质坐标为“世界位置 − 位移”。

为避免无限累计造成拉丝，每套纹理只保留最多 **2 秒模拟时间**的形变，两套错开 **1 秒**更新。更新时该套纹理的显示权重为零，两套覆盖率平滑交替；不混合坐标，也不直接把两张噪声平均后再做阈值。每套纹理在显示期间仍随水流移动、拉伸，但具体孔隙会逐渐更新，不能永久追踪同一个孔隙。泡沫密度与速度不会随纹理更新清空。

纹理追踪与渲染插值仅使用湿格子的位移，并重新归一化权重，避免干格子的零位移拉扯湿区纹理。完全没有湿采样时使用初始坐标。更新相位随实际 SWE 子步推进，暂停不更新，RK2 只在完成子步后更新一次；干格子仍保持同步相位。

参数实时生效并自动保存，不重置历史。暂停保持动量和纹理历史；重置和禁用清空历史，干格子清除泡沫及其动量，不留下沙地残留。背景生成中的汇聚判据继续使用水流速度；前沿生成仅使用有方向的压缩。薄水材质的 8 mm 裁剪不变。

泡沫速度使用深度平均水流作为目标；纹理位移的半拉格朗日输送仍有数值扩散，单次 2 秒内的强剪切仍可能产生拉伸。泡沫空间输送默认仍为一阶；可在二阶 SWE（RK2）下，将“泡沫空间输送”切为二阶 MUSCL。它对泡沫量与两个动量分量共同做受限线性重构，用统一缩放保证面密度非负、面速度不产生明显邻域过冲。干湿交界（重构邻域水深不超过 20 mm 或湿阈值）、障碍物阻挡和域边缘局部退回一阶。沿用共享 Rusanov 通量与现有 0.2 CFL；不通过截断到 0～1 保形，也不改变生成、衰减、拖曳、两秒纹理更新或材质。它减少数值扩散，但不会产生新的湍流或永久保留格子以下的细丝。

`foam_transport_order` 缺失时取 1，旧配置不会改变行为。选择实时生效且不清空历史；SWE 为一阶时保存的二阶选择暂不生效。固定场景可用以下命令启用，不读取或覆盖个人配置：

```powershell
cargo run --release -- --beach --no-settings --swe-order 2 --foam-transport-order 2
```

`--verify` 先用生产 GPU 管线验证前慢后快的两团泡沫：各长 2 m，相隔 1 m，后团密度 0.8、速度 1 m/s，前团密度 0.2、速度 0.2 m/s，静水中关闭生成、拖曳、衰减，推进 5 秒。检查总量与两方向总动量守恒、质心位移、汇聚密度增长和相对动能耗散，覆盖一阶 SWE、一阶泡沫 + RK2、MUSCL 泡沫 + RK2，并将 MUSCL 追赶场景缩放到 1e-7 初始密度检验近真空行为。二阶泡沫同样覆盖障碍物、干湿清除及完整海岸推进。另以独立 f64 RK4 对照 WGSL 的新生动量、拖曳、衰减及时间步一致性，随后验证海岸传播、干湿清除、暂停、参数修改与渲染。

原一阶空间通量在 RTX 3070 / Vulkan 验证中，Euler／RK2 的峰值密度分别为 1.0483／1.0441，总动量除以总量均约为 0.84 m/s；总量相对误差约百万分之一，动量误差约千万分之二。0.25 m 网格下，质心移动约 4.10 m，相比连续守恒方程的 4.20 m 有约 0.10 m 的一阶空间误差；总量和总动量守恒不等同于轨迹没有数值误差。

### 物理依据与推荐起点

“物理参考起点 → 应用推荐值”设置以下五项，实时生效并保存；旧配置仍保留原有跟随倍率、响应时间与衰减值，缺少新字段时补默认值。

| 参数 | 推荐起点 | 解释 |
| --- | --- | --- |
| 启用水流拖曳 | 开启 | 水流向泡沫传递动量 |
| 水流跟随倍率 | 1 | 仅有水流拖曳时，两者趋向同速；小于 1 是附加的视觉阻滞 |
| 速度响应时间 | 2 秒 | 约 2 秒消除初始相对速度的 63%；可在 2～5 秒比较保留惯性的程度 |
| 新生泡沫速度倍率 | 1 | 生成时继承当地水流速度，之后再由拖曳与输送改变 |
| 水上泡沫衰减 | 0.3 /秒 | 无源时半衰期约 2.31 秒，约 7.68 秒剩 10%；与显示消失时刻不同 |

[Wretborn 等，SIGGRAPH 2022，Guided Bubbles and Wet Foam](https://alexey.stomakhin.com/research/siggraph2022_whitewater.pdf) 使用相对水流速度产生线性表面拖曳，表 3 给出的模拟拖曳系数为 0.05～0.5 /秒。取其倒数得到本项目响应时间 2～20 秒；推荐 2 秒取较强拖曳端。这是生产模拟的调参依据，并非海岸实测通用常数。其完整模型还包含泡沫压力、黏性与内聚力，本实现只借鉴拖曳形式，不能直接等同整篇论文的物理模型。

[Callaghan 等，2024，实验与现场白浪泡沫演化比较](https://agupubs.onlinelibrary.wiley.com/doi/full/10.1029/2023JC020193) 表明泡沫衰退同时受水下气泡逸出及表面活性物质稳定作用影响，寿命不能只由水流速度确定。本项目选择 0.3 /秒作为秒级衰退的校准起点，并不是把论文的可见面积寿命直接转换为本项目面密度衰减系数。

本实现假设泡沫量正比于有效质量，使用无压力的单速度层；碰撞后的格内混合保持总动量，耗散相对动能。尚未模拟含液率变化、排液、泡径分布、膜破裂、压力、黏性、内聚力、风力或泡沫对水体的反作用。因此能表达“追赶—汇聚—衰退”，但不是完整的定量海水泡沫模型。前沿生成阈值和生成率仍是模型／分辨率相关参数，缺少当地实测输入时不称为物理材料常数。

## 泡沫形态输送对照

新增独立离线工具 `foam-lab`：固定平移、剪切、差速旋流，直接调用生产 GPU 泡沫输送，默认在相同 RK2 和 CFL=0.2 下对比 128／256／512 格的一阶与二阶空间输送，在 0、2、4、8 模拟秒输出密度快照、解析参考、误差指标及设备支持时的真实 GPU timestamp 耗时。默认保形实验不读取个人配置、不运行完整 SWE、不使用泡沫材质。另有 `--benchmark` 模式，调用完整生产 SWE 内核，以固定稀疏／密集／合成模型地形／8 障碍物输入比较三种阶数组合，预热后报告重复 GPU 子步计时并导出单步状态。该内核基准不包含正常帧的参数／状态拷贝及渲染，不代表界面 FPS。使用方式、参数与结果边界见 [README-foam-lab.md](README-foam-lab.md)。

```powershell
cargo run --release --features ocean --bin foam-lab -- --output target/foam-lab
# 完成后打开 target/foam-lab/index.html；输出目录必须不存在。
```

## 泡沫高度：独立的外观偏移

在 **浅水模拟（SWE）→ 泡沫高度（仅外观）** 中设置：

| 参数 | 默认值 | 范围 | 作用 |
| --- | --- | --- | --- |
| 最大高度（米） | 0.05 | 0～1 | 泡沫最多向上抬高多少；0 关闭 |
| 起始密度 | 0.1 | 0～20 | 低于或等于此密度时高度为 0 |
| 密度过渡范围 | 1.9 | 0.001～20 | 从起始密度到“起始密度＋过渡范围”逐渐达到最大高度 |

使用模拟中的水上泡沫量／面积作为密度，不先截断到 0～1，也不使用密度颜色参考值或孔隙纹理。充分有水处的计算为：

```text
高度权重 = smoothstep(起始密度, 起始密度 + 密度过渡范围, 泡沫密度)
外观高度偏移 = 最大高度 × 高度权重
渲染顶点高度 = 原水面高度 + 外观高度偏移
```

例如使用默认值：密度 0.1 时不抬高，密度 1.05 时抬高 0.025 米，密度达到 2.0 及以上时抬高 0.05 米。这里的密度是现有模拟量，不是以 kg/m³ 表示的泡沫材料密度；该映射用于表现厚度，没有引入含液率或体积求解。

偏移只发生在渲染顶点中，不写回 FFT/SWE，不改变水位、水深、动量、泡沫输送或生成。泡沫明暗使用抬高后表面的法线，水体光学和水体法线继续使用原水面数据。参数与水面振幅增益独立，实时生效、自动保存，暂停时也可调整，不清空历史；旧配置缺少这些字段时补上述默认值。

目前只抬高近岸水上泡沫；远海 FFT 泡沫不受影响。按模拟水深在 8～20 毫米间平滑开启，8 毫米及以下为零，防止干地被顶起；关闭“显示泡沫”或 SWE 时同样关闭偏移。红绿密度调试视图可观察高度变化。仍复用现有水面网格，孔隙处没有另建泡沫壳层，细窄泡沫带的几何细节受渲染网格分辨率限制。

`--verify` 调用实际顶点函数检查 smoothstep 数值节点、饱和高度、干湿边界、显示开关、颜色参考值和振幅独立性，以及抬高后泡沫法线与未抬高水面数据的分离。另验证一阶／二阶中只改变高度参数时全部模拟状态一致，并在固定海岸历史、相机、时间下验证实际渲染像素变化。

## 静态碰撞体

右侧 Dock 新增 **碰撞体** 页签，可添加最多 8 个圆柱或长方体。默认没有障碍物；先在 SWE 面板开启“倾斜沙滩”和“启用浅水模拟”，然后添加。面板沿用统一的两列表格，详细说明放在名称 hover 中。

| 控件 | 含义 |
| --- | --- |
| 启用／删除 | 单独开关或移除一个碰撞体 |
| 形状 | 圆柱或长方体 |
| 位置 X/Z（米） | 世界坐标；X 增大朝海，Z 沿岸 |
| 半径（米） | 圆柱半径，0.25～30 |
| 宽度／长度（米） | 长方体完整尺寸，0.5～60 |
| 旋转（度） | 长方体绕竖直轴旋转，±180 |
| 高度（米） | 从中心处原海底到水平顶面的高度，0.1～30 |
| 顶面海拔（米） | 自动计算的顶面高度；静止海平面为 0 |
| 模拟状态 | 是否启用、是否处于有效 SWE 区域 |

第一次添加放在岸线向海 8 米处，后续槽位沿 Z 错开 8 米。默认高度 4 米；默认 0.18 的沙滩坡度下，顶面高于海平面 2.56 米，适合先观察挡水与绕流。降低高度使顶面接近或低于海平面，可观察漫顶或水下障碍的影响。面板显示实际网格间距和有效范围；建议两个方向至少覆盖 3 格，圆形和旋转边界在模拟中仍按网格近似。

碰撞体作为固定海底地形参与 SWE：每格有效底面取原沙滩与覆盖该格的各碰撞体顶面的最高值。一阶与二阶求解都使用同一底面，通过原有静水重构、重力与干湿边界产生阻挡、反射和绕流；水面超过顶面时允许流过。多个形状重叠时取最高顶面，不叠加高度。水上泡沫沿用独立动量输送，在未被淹没的障碍边界关闭跨面通量，避免穿过侧壁；不额外凭空制造碰撞泡沫，泡沫仍由现有生成模式判断水流变化。

圆柱与长方体同时写入不透明颜色和深度，水下部分参与水体吸收与散射，水上部分遮挡水面。水面插值忽略干燥障碍顶面，避免在物体周围拉出假的水丘。远海 FFT 保持单向输入；碰撞体附近 2 米内显示 SWE，2～8 米恢复原 FFT 混合，并在计算域边缘淡出这一覆盖。

交互限于局部 SWE。整个形状必须位于模拟区域内，远海一侧还须避开最后 20% 的 FFT 输入松弛带；实现预留了额外边界余量。超出范围时保留参数，但不显示、不参与碰撞，面板给出状态。碰撞响应不会反向写回远海 FFT。

有效碰撞体的添加、移动、尺寸／高度／旋转编辑、启用和删除即时生效，暂停时也会应用，不重新初始化 SWE、不重启近岸输入，也不清空全域泡沫和沙地历史。只在有效底面实际变化的格子调整状态：抬高底面时裁除新占据的水体，按剩余水深缩放水流动量，避免把原有水面直接抬到物体顶上；降低底面时保留原有水量，由周围水流在继续运行后回填，不凭空补水。新变干格子的水上泡沫、泡沫动量和纹理位移清除，不在物体顶面产生假沉积；其他格子的历史保持不变。编辑会改变局部水量，不做守恒排水或移动刚体推水。参数进入自动保存和命名配置，旧配置缺少碰撞体字段时按空场景加载。这是静态场景编辑，不是移动刚体：未模拟浮力、刚体运动、流体对物体的反作用、悬空物体下穿流或三维飞溅。

`--verify` 在两种求解阶数下检查圆柱／旋转长方体的静水平衡、露出水面的高墙反射、水与泡沫不穿墙及总量守恒、圆柱两侧绕流、低障碍漫顶，以及暂停编辑的局部状态调整、未变化区域的历史保留和继续运行；渲染验证覆盖圆柱／旋转长方体的水上遮挡和水下场景颜色读取。可用环境变量 `OCEAN_COLLIDER_PREVIEW` 指定验证预览 PNG 的输出路径；普通验证不会额外写图。


### SWE 网格精度

在 **浅水模拟（SWE） → 每轴网格数** 选择 64、128、256、512 或 1024，默认 256。该参数独立于 FFT 分辨率和渲染网格，自动保存，也包含在命名配置中；旧配置默认使用 256。

模拟覆盖范围不变，X/Z 每格大小分别为区域宽度/格数、区域长度/格数，SWE 和碰撞体面板均显示实际网格间距。切换立即重建近岸缓存，并用当前 FFT 重新初始化水体，清空近岸泡沫与湿沙历史；FFT 时间和远海泡沫继续运行。碰撞体边缘预留的一格也随精度变化。

每轴格数翻倍，单元数和主要状态显存约为原来的 4 倍，CFL 稳定步长通常减半，相同模拟时间的运算量可能约为 8 倍。三份状态缓存在 256/512/1024 时分别约为 15/60/240 MiB。可结合 SWE GPU 耗时与实际推进时间比较，默认每帧最多推进 128 个子步的限制仍生效。

## NSH 泡沫纹理与材质

在 **浅水模拟（SWE）→ 泡沫纹理与材质** 中选择原灰度效果、Shorewaves NSH、Crest Foam NSH 或 Crest Foam2 NSH。默认使用 Shorewaves NSH；旧配置缺少字段时补此默认值，其余已有参数正常加载。切换纹理、尺寸、微法线、不透明度密度尺度及粗糙度实时生效并保存，暂停时也可以比较，不重置模拟或纹理位移历史。旧配置中的 `foam_soft_opacity`、`foam_thick_start` 被忽略，新字段 `foam_opacity_scale` 缺失时使用默认值 2，其余参数正常加载。

三张 NSH 都由仓库内的源图离线烘焙，统一为 1024×1024：RG 为切线法线 XY，B 为 Soft，A 为相对 Height。它们作为线性 `Rgba8Unorm` 纹理数组上传，使用 Repeat 和完整 mip 链；法线按三维方向平均后归一化，Soft／Height 单独做线性过滤。Crest 源图按原导入设置解码 sRGB 后烘焙；源图、固定版本、SHA-256 和 MIT 许可见 [assets/foam/SOURCES.md](assets/foam/SOURCES.md)。Height 是为着色设计的形态，不是实测泡沫厚度。

材质保留现有两套位移场与 2 秒交替重置。每套位移分别采样粗细两种尺度，细尺度为粗尺度的 1/10，权重 0.72／0.28。每个相位保留原有密度和 Soft 决定的孔隙覆盖，但不再划分薄／厚两种着色；覆盖内使用连续密度响应：

```text
不透明度 = 孔隙覆盖率 × (1 − exp(−原始泡沫密度／不透明度密度尺度))
```

原始密度是模拟泡沫量／面积，不先归一化截断，不是实测质量密度。完整覆盖且尺度为默认 2 时，密度 2、4、6 的不透明度约为 63%、86%、95%；增大尺度使同一密度更透水，不改变孔隙阈值。密度颜色参考值仍控制调试色标和孔隙填充范围，但不再作为 NSH 浓淡的饱和上限；高密度仍可填满原有孔洞。这是外观映射，不增加新的形态或扭曲。

每个相位分别完成光照后混合预乘颜色与不透明度，避免先混原始图案再阈值导致孔洞跳变。微法线随流动坐标的局部旋转／剪切调整方向，叠在现有宏观泡沫厚度法线上。Height 仍可调试查看，且是离线微法线的来源，不再用于运行时薄／厚门控。

| 参数 | 默认值 | 含义 |
| --- | --- | --- |
| 纹理尺寸 | 12 米 | 粗层每块纹理的世界长度，范围 0.25～64 米 |
| 泡沫微法线强度 | 0.65 | 无量纲，范围 0～3；0 保留宏观法线，关闭纹理微法线 |
| 不透明度密度尺度 | 2 | 模拟泡沫量／面积，范围 0.01～20；完整覆盖处此密度对应约 63% 不透明度，越大越透水 |
| 泡沫粗糙度 | 0.85 | 范围 0.15～1，控制简化泡沫高光的宽度 |

材质调试提供 Soft、Height、世界法线、最终不透明度四种视图。编号 4 原为薄绿／厚红，现在显示含孔隙与双相位混合的不透明度：黑色为 0，白色为 1。它只在浅水正常着色且开启泡沫显示时生效；现有红绿密度调试仍直接显示模拟密度。连续不透明度只作用于近岸 NSH 路径，原灰度效果和远海 FFT 泡沫保持原样；材质参数本身不改变密度、动量、生成、衰减、湿沙或泡沫外观高度。

```powershell
# 重建三张 NSH 与制作配方；预览写入被忽略的 target 目录。
cargo run --features ocean --bin foam-bake
# 查看自定义源图、颜色空间、曲线、模糊和法线参数。
cargo run --features ocean --bin foam-bake -- --help

# 固定默认种子、场景和时间，对比来源 0～3（输出路径需不存在）。
cargo run --release --bin ocean -- --beach --no-settings --foam-texture 2 --time 6 --snapshot target/crest-foam.png
# 调试 NSH Height。
cargo run --release --bin ocean -- --beach --no-settings --foam-texture 2 --foam-material-view 2 --frames 600
# 扩大浓淡响应范围，并查看最终不透明度；场景预设应放在参数前面。
cargo run --release --bin ocean -- --beach --no-settings --foam-opacity-scale 4 --foam-material-view 4 --frames 600
```

烘焙流程及预览面板顺序见 [assets/foam/README.md](assets/foam/README.md)。输出图在编译时嵌入，重新烘焙后需要重新构建 ocean。当前工具从现成灰度图加工 NSH，尚未实现从随机种子生成泡径／网膜形态的生成器，也未改成三相位。

`--verify` 检查生产 WGSL 的零密度／干水边界、超出旧参考值后的指数不透明度、密度与尺度单调性、孔隙保持及法线方向与归一化，以及实际 GPU 三层纹理采样、Repeat 和 mip 标量平均。已有一阶／二阶对照同时检查材质参数不影响模拟状态。

近岸泡沫保留背景与前沿生成，两者生成率相加后应用薄水抑制和入口淡出。流场融合生成及其参数面板、`--foam-source-mode` 参数已移除；旧配置中的相关字段会被忽略，其余参数正常加载。水上泡沫随水流输送、汇聚与拖曳不变。
