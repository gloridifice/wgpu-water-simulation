# Execution tasks

- [x] 1.1 扩充模型双浓度与非零 FFT 固定输入基准，保存未优化基线
  - Design: [implementation](implement.md#1-baseline-and-goals)
  - Acceptance: 原四场景输入不变；新增场景覆盖双浓度、造泡与输入带；记录参数和 FFT 指纹，实际运行 256²／512² 并保存输出。

- [x] 2.1 按轴复用水体邻域、三份重构和床面源项
  - Dependencies: 1.1
  - Acceptance: 保留一阶与二阶公式、退阶阈值、更新顺序；独立 GPU 计时及五块状态对照满足设计要求。

- [x] 2.2 单元中心直接读取模型床面，复用碰撞体抬高逻辑
  - Dependencies: 2.1
  - Acceptance: 无新缓存和 ABI 改动，渲染任意坐标采样不变；地形编辑与求解一致；扩展现有非整数矩形域验证并通过 GPU 检查、基准对照。

- [x] 2.3 正常推进仅在松弛带计算 FFT 输入
  - Dependencies: 2.2
  - Acceptance: 重置全域初始化不变，水位／动量成对耦合；非零 FFT 固定输入及既有初始化／输入验证通过，记录独立计时。

- [x] 3.1 同步当前架构和基准使用说明
  - Dependencies: 2.3
  - Acceptance: 文档只描述已保留实现；明确新场景、固定内核与生产 SWE 计时差异，不声称个人场景帧率收益。

- [x] 3.2 整体验证、重复性能对照和差异检查
  - Dependencies: 2.1, 2.2, 2.3, 3.1
  - Acceptance: 格式、Clippy、库测试及完整 GPU --verify 通过；二阶模型或障碍物收益可重复，记录输出误差和未测限制；个人设置未改，不提交、不自动完成或归档。

## Verification

执行环境：RTX 3070、Vulkan、release。以下产物均在忽略的 target 中，不纳入版本管理；命令从仓库根目录执行。

### 逐项对照

- 1.1：扩充基准后、改着色器前执行 `cargo build --release --features ocean --bin foam-lab --bin ocean` 及 target/release/foam-lab.exe --benchmark --resolutions 256,512 --output target/swe-opt-baseline。30 组输出有效；与调研基准对照，原四场景的 24 组输入、地形、步长和输出指纹全部不变。基线 `ocean --verify-terrain` 通过。
- 2.1：相同命令输出到 target/swe-opt-stencil。256² 模型双浓度二阶从 0.564640 降至 0.247968 ms／子步，障碍物从 1.506784 降至 0.484416；五块输出最大缩放差异 2.384e-7，一阶全部逐位一致。
- 2.2：输出到 target/swe-opt-bed。模型双浓度二阶进一步降至 0.124128 ms／子步，模型单浓度为 0.103136；输出误差不增。target/release/ocean.exe --verify-terrain 通过，包括非二进制精确原点／矩形尺寸、暂停重建及四边开放静水；未放宽阈值。
- 2.3：输出到 target/swe-opt-inlet；复跑到 target/swe-opt-repeat。保留区域裁剪：模型单浓度 256² 从床面优化后的 0.103136 降至 0.095904／0.096192 ms，双浓度从 0.124128 降至 0.120864／0.121056。其他路径收益较小且有噪声，不逐项宣称固定百分比。
- 所有新基准 A/B 的初始状态、地形、完整阶段参数及 FFT 两帧指纹一致。临时比较工具 target/swe-opt-compare.py 逐格检查全部五块，30 组最大绝对／缩放差均为 2.384e-7，低于预设 5e-5；10 组一阶输出逐位一致。最终两次运行的 30 组输出指纹全部相同。

二阶水体＋一阶泡沫管线；双浓度场景不使用空间泡沫阶数。最终两次中位数范围如下，单位 GPU ms／完整子步：

| 网格 | 场景 | 基线 | 最终 |
| --- | --- | ---: | ---: |
| 256² | beach-dense | 0.582848 | 0.084832～0.084896 |
| 256² | model-dense | 0.596064 | 0.095904～0.096192 |
| 256² | obstacles-dense | 1.506784 | 0.236192～0.247648 |
| 256² | model-dual-inlet | 0.564640 | 0.120864～0.121056 |
| 512² | beach-dense | 2.190496 | 0.252864～0.254400 |
| 512² | model-dense | 2.469984 | 0.292256～0.292608 |
| 512² | obstacles-dense | 6.345504 | 0.826720～0.856224 |
| 512² | model-dual-inlet | 2.485824 | 0.409472～0.409536 |

固定 dt 分别为 1.315542／0.657771 模拟毫秒，未减少推进量。以最终复跑计，模型双浓度的等效每模拟秒成本分别从 429.2 → 92.0 GPU ms、3779.2 → 622.6 GPU ms。该指标由固定输入单步外推，不是连续真实海岸演化。

### 整体验证

- target/release/ocean.exe --verify：通过。日志 target/swe-opt-inlet/verify.log，涵盖 FFT、水体两阶、两种泡沫模式、碰撞体、浪花和渲染。4 米行波相对 L2 误差一阶 0.720、二阶 0.178；静水最大误差二阶 3.43e-5，均满足原阈值。
- `cargo fmt --all --check`：通过。
- `cargo clippy --features ocean --all-targets -- -D warnings`：通过。
- `cargo test --features ocean --lib`：64 项通过。
- 最终 release 二进制重建通过；最后一次 Rust 改动仅更新 benchmark 帮助文本，着色器与上述 GPU 验证相同。
- `git diff --check`：通过。已检查实现差异，无参数／ABI／调度／CFL 变动。只扩展已有地形行为验证，没有新增测试专用生产接口；个人配置保留原有未提交修改。
- 当前架构、Ocean 和 foam-lab 指南已同步。已核对其他活动变更，未改其任务或引入 UI／浪花调度变更。

### 限制

仅在 RTX 3070 / Vulkan 实测；未测其他 GPU／后端，也未测个人模型实景 FPS、逐阶段硬件计数器和实际显示窗口。固定内核基准不含正常状态复制和浪花源，不能按以上倍数推断完整帧率。CPU 包、workspace 和依赖未改，本轮未重复 CPU 专用测试。变更保持 active，未提交、完成或归档。
