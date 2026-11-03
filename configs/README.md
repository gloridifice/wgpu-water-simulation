# 仓库配置与礁石前沿实验

## 配置文件

- `ocean-settings.json`：从原个人文件完整复制的当前会话与命名配置库，包含“常用”和“礁石”，纳入 Git。原文件仍留在用户目录，未被实验改写。从仓库根目录启动时，此文件是默认手动保存目标；点击顶部保存或按 Ctrl+S 后，参数调整才会成为 Git 差异。停止编辑、失焦和退出不再保存会话。它不是永远冻结的备份，也不会与用户目录里的旧文件自动同步。
- `experiments/reef-front-ridge.json`：从命名配置“礁石”派生，不是从顶层当前相机状态派生。保持原 FFT、地形、水位、相机、水体／泡沫二阶输送、拖曳、衰减、颜色与材质；启用压缩脊线、带宽 1 米、相对显著度 0.02，背景倍率设零。关闭浪花粒子与区域线框，取消暂停，便于单独观察水面泡沫。

默认路径以**启动工作目录**为准，不使用开发者机器的绝对路径。没有 `configs/ocean-settings.json` 时回退到原用户目录；`--settings PATH` 显式指定可写目标。配置损坏备份与原子写入临时文件不纳入 Git。

`--config PATH` 只读加载顶层当前会话，保留相机与远海泡沫参数，不切换配置库里的命名项，不自动保存；缺文件或不合法时直接报错。窗口里仍能临时调参，但关闭后不会写回。不要额外加 `--model` 或 `--beach`，否则会覆盖这份配置的模型摆放和相机。

```powershell
# 恢复并继续保存自己的当前工作状态。
cargo run --release

# 只读预览实验；自动结束，不改写配置。
cargo run --release --bin ocean -- --config configs/experiments/reef-front-ridge.json --frames 600

# 如需将实验中的手动调参保存到实验文件，显式选择可写目标。
cargo run --release --bin ocean -- --settings configs/experiments/reef-front-ridge.json
```

## 固定条件对照

以下对照只切换新旧前沿源，两边背景倍率都为零，不是拿全部旧背景源与新前沿直接比较。固定种子 42、FFT 128²、128 米域、波高增益 0.3、SWE 256²、SWE／泡沫二阶、原“礁石”相机。快照采用模拟时间，不受保存的暂停／播放倍率影响；输出文件必须不存在。

```powershell
cargo build --release --bin ocean
$exe = "target/release/ocean.exe"
$cfg = "configs/experiments/reef-front-ridge.json"

# 最近子步实际发射率，黑色为零，白色为前沿满生成率。
& $exe --config $cfg --time 12 --shore-view 7 --snapshot target/reef-ridge-source-12.png
& $exe --config $cfg --no-foam-ridges --time 12 --shore-view 7 --snapshot target/reef-broad-source-12.png

# 同一模拟时刻的最终着色。可改用 --shore-view 6 排除材质影响。
& $exe --config $cfg --time 12 --snapshot target/reef-ridge-shaded-12.png
& $exe --config $cfg --no-foam-ridges --time 12 --snapshot target/reef-broad-shaded-12.png

# 仅在 (8,9] 秒发射，观察停发后的输送与衰减。
& $exe --config $cfg --foam-emission-window 8,9 --time 9 --snapshot target/reef-pulse-9.png
& $exe --config $cfg --foam-emission-window 8,9 --time 12 --snapshot target/reef-pulse-12.png
& $exe --config $cfg --foam-emission-window 8,9 --time 16 --snapshot target/reef-pulse-16.png
```

时间窗按约 1/60 秒暖机帧切换发射开关，不能把它理解为自动追踪同一条波前。允许窗口位于快照的最近 60 秒暖机范围内；已有泡沫不在停发时清除。发射率仍由生产子步按模拟步长积分。`--shore-view 8` 是峰值权重，不是已输送密度；停发时权重可非零，但实际源项为零。暂停保留最近子步诊断。

## 已观察到的结果与限制

RTX 3070 / Vulkan，在上述固定配置和时刻，读取的是完整 65,536 单元场，不是截图白色像素计数。发射单元统计阈值为生成率大于 0.001/秒；泡沫单元统计阈值为面密度大于 0.02。

| 情况 | 时刻 | 发射单元 | 泡沫单元 | 最大泡沫密度 |
| --- | ---: | ---: | ---: | ---: |
| 旧前沿源，持续发射 | 12 秒 | 5167 | 10368 | 17.650 |
| 压缩脊线，持续发射 | 12 秒 | 1186 | 6316 | 7.147 |
| 压缩脊线，只在 8～9 秒发射 | 9 秒 | 1027 | 2824 | 5.633 |
| 同上，停发后 | 12 秒 | 0 | 2233 | 1.192 |
| 同上，停发后 | 16 秒 | 0 | 823 | 0.230 |

实际生成区收窄了，原先贴岩的大块白带变成较分散的小片与短条。停发后仍有泡沫状态；单次发射在原衰减率 0.45/秒与原材质下很快变淡。**这些结果尚未显示参考图那样的连通网状结构，不能据此宣称形态问题已解决。** 没有为了增加可见性去修改不透明度或衰减。

当前限制：

- 局部流向只是波前法线近似，礁石绕流和交叉波中可能失准；接近静止的单元不发射。
- 五点局部拟合只支持一至两格的有效窄带，存在网格依赖；更高分辨率并不保持同一物理带宽。
- 没有波前追踪、时间滞回或自动单次发射。停驻的强压缩峰仍能连续产泡。
- 没有加入黏性、凝聚力或细尺度湍流，也没有预制网纹替代动力学。FFT/J 独立诊断工具未改动。

完整 `ocean --verify` 已通过，包括新增真实 GPU 检查：解析压缩峰沿切向连续、均匀压缩不当作峰、矩形网格与旋转、扩张／干区保护、两阶每子步一次注入、源项不改变水体，以及停发后大于 1 的密度守恒前移。CPU 库测试、Clippy 和无 ocean 的 CPU 构建也通过。另运行了 900 帧只读窗口，并用窗口截图检查礁石渲染及可见的新控件；未逐项操作全部控件。窗口退出后，两份仓库配置和原用户文件的 SHA-256 均未变化。生成图片、日志留在 `target/`，不纳入 Git。
